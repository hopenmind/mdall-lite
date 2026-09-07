//! Offline spell checking, pure-Rust and Hunspell-compatible (via `spellbook`).
//!
//! A [`SpellChecker`] wraps one loaded `.aff`/`.dic` pair (one language). It is
//! the first "engine module" of the Module system: the UI loads dictionaries
//! (bundled, downloaded, or user `.dic`/`.aff`) and asks this engine to check a
//! document. No C dependency, no network at runtime.

/// A loaded dictionary for one language, plus the user's personal additions.
pub struct SpellChecker {
    dict: spellbook::Dictionary,
    /// BCP-47-ish language tag of the loaded dictionary (e.g. "en_US", "fr_FR").
    lang: String,
}

/// A misspelled word located in a document, by byte range in the source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Misspelling {
    pub word: String,
    pub start: usize,
    pub end: usize,
}

impl SpellChecker {
    /// Build a checker from raw `.aff` and `.dic` contents.
    pub fn from_aff_dic(aff: &str, dic: &str, lang: &str) -> Result<Self, String> {
        let dict = spellbook::Dictionary::new(aff, dic)
            .map_err(|e| format!("dictionary parse error: {e}"))?;
        Ok(Self { dict, lang: lang.to_string() })
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// True when `word` is in the dictionary (or the personal additions).
    pub fn check(&self, word: &str) -> bool {
        self.dict.check(word)
    }

    /// Correction suggestions for a misspelled word (best first, capped).
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(word, &mut out);
        out.truncate(8);
        out
    }

    /// Add a word to the personal dictionary for this session.
    pub fn add_word(&mut self, word: &str) -> bool {
        self.dict.add(word).is_ok()
    }

    /// Spell-check a whole Markdown document, returning every misspelled word
    /// with its byte range in `md`. Code (fenced + inline), math (`$...$`/`$$...$$`),
    /// link URLs, HTML tags and LaTeX commands are skipped so only prose is
    /// checked.
    pub fn check_document(&self, md: &str) -> Vec<Misspelling> {
        let mut out = Vec::new();
        let mut in_fence = false;
        let mut line_start = 0usize;
        for line in md.split_inclusive('\n') {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                line_start += line.len();
                continue;
            }
            if !in_fence {
                self.check_line(line, line_start, &mut out);
            }
            line_start += line.len();
        }
        out
    }

    fn check_line(&self, line: &str, base: usize, out: &mut Vec<Misspelling>) {
        let b = line.as_bytes();
        let len = line.len();
        let mut i = 0;
        let mut code = false;
        while i < len {
            let c = b[i];
            if c == b'`' {
                code = !code;
                i += 1;
                continue;
            }
            if code {
                i += 1;
                continue;
            }
            match c {
                // Inline / display math: skip to the next '$'.
                b'$' => {
                    i += 1;
                    while i < len && b[i] != b'$' {
                        i += 1;
                    }
                    i += 1;
                }
                // HTML tag: skip to '>'.
                b'<' => {
                    while i < len && b[i] != b'>' {
                        i += 1;
                    }
                    i += 1;
                }
                // LaTeX command / escape: skip backslash + following letters.
                b'\\' => {
                    i += 1;
                    while i < len && b[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                }
                // Link target: `](url)` - skip the URL in parentheses.
                b']' if i + 1 < len && b[i + 1] == b'(' => {
                    i += 2;
                    let mut depth = 1;
                    while i < len && depth > 0 {
                        match b[i] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => {
                    let ch = line[i..].chars().next().unwrap();
                    if ch.is_alphabetic() {
                        let start = i;
                        let mut j = i;
                        while j < len {
                            let cj = line[j..].chars().next().unwrap();
                            if cj.is_alphabetic() || cj == '\'' || cj == '\u{2019}' {
                                j += cj.len_utf8();
                            } else {
                                break;
                            }
                        }
                        // Trim trailing apostrophes (e.g. a possessive "dogs'").
                        let raw = &line[start..j];
                        let word = raw.trim_end_matches(['\'', '\u{2019}']);
                        if word.chars().count() >= 2 && !self.dict.check(word) {
                            out.push(Misspelling {
                                word: word.to_string(),
                                start: base + start,
                                end: base + start + word.len(),
                            });
                        }
                        i = j;
                    } else {
                        i += ch.len_utf8();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal Hunspell dictionary covering the test prose.
    const AFF: &str = "SET UTF-8\n";
    const DIC: &str = "6\nhello\nworld\nquantum\nadvantage\nthe\nis\n";

    fn checker() -> SpellChecker {
        SpellChecker::from_aff_dic(AFF, DIC, "en_TEST").unwrap()
    }

    #[test]
    fn check_and_suggest() {
        let sc = checker();
        assert!(sc.check("hello"));
        assert!(!sc.check("helllo"));
        let sug = sc.suggest("helllo");
        assert!(sug.iter().any(|s| s == "hello"), "suggestions: {sug:?}");
    }

    #[test]
    fn add_word_is_accepted() {
        let mut sc = checker();
        assert!(!sc.check("qubit"));
        assert!(sc.add_word("qubit"));
        assert!(sc.check("qubit"));
    }

    #[test]
    fn document_flags_only_prose_misspellings() {
        let sc = checker();
        let md = "\
The quantum advantage is real.

The wrold is `helllo` in code stays.

```
helllo inside a fence is ignored
```

Math $helllo$ and a [link](http://helllo.example) are skipped.

But hello wrld here.";
        let bad = sc.check_document(md);
        let words: Vec<&str> = bad.iter().map(|m| m.word.as_str()).collect();
        // "wrold" (prose typo) and "wrld" (prose typo) are flagged.
        assert!(words.contains(&"wrold"), "got {words:?}");
        assert!(words.contains(&"wrld"), "got {words:?}");
        // Code / fence / math / URL "helllo" are NOT flagged.
        assert!(!words.contains(&"helllo"), "code/math/url must be skipped: {words:?}");
        // Correct words are not flagged.
        assert!(!words.contains(&"quantum"));
        assert!(!words.contains(&"hello"));
        // Byte ranges point at the actual word.
        let first = bad.iter().find(|m| m.word == "wrold").unwrap();
        assert_eq!(&md[first.start..first.end], "wrold");
    }
}
