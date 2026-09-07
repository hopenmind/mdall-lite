//! Minimal, dependency-free BibTeX citation engine.
//!
//! Resolves Pandoc-style `[@key]` and LaTeX `\cite{key}` citations against a
//! `.bib` database: citations are numbered in order of first appearance, the
//! in-text marker becomes `[n]`, and a numbered reference list is appended. This
//! is the piece that lets a manuscript be finished here instead of in LaTeX.
//!
//! Numeric style only for now (`[1]`, `[2]`); author-year / CSL can layer on top
//! of the same `BibEntry` model later.

use std::collections::HashMap;

/// One parsed BibTeX record (only the fields we render are kept; the rest are
/// available in `fields` for future citation styles).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BibEntry {
    pub key: String,
    pub entry_type: String,
    pub fields: HashMap<String, String>,
}

impl BibEntry {
    fn get(&self, name: &str) -> &str {
        self.fields.get(name).map(|s| s.as_str()).unwrap_or("")
    }

    /// Format a numeric-style reference line: "Authors. Title. Journal, Year."
    pub fn format_numeric(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let author = self.get("author");
        if !author.is_empty() {
            parts.push(format!("{}.", author.replace(" and ", ", ")));
        }
        let title = self.get("title");
        if !title.is_empty() {
            parts.push(format!("{}.", title));
        }
        let venue = if !self.get("journal").is_empty() {
            self.get("journal")
        } else if !self.get("booktitle").is_empty() {
            self.get("booktitle")
        } else {
            self.get("publisher")
        };
        let year = self.get("year");
        match (venue.is_empty(), year.is_empty()) {
            (false, false) => parts.push(format!("{}, {}.", venue, year)),
            (false, true) => parts.push(format!("{}.", venue)),
            (true, false) => parts.push(format!("{}.", year)),
            (true, true) => {}
        }
        parts.join(" ")
    }
}

/// Parse a `.bib` string into a key→entry map. Tolerant: skips malformed records,
/// accepts `{...}` or `"..."` field values, ignores `@comment`/`@string`.
pub fn parse_bibtex(src: &str) -> HashMap<String, BibEntry> {
    let mut out = HashMap::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        // entry type
        let type_start = i + 1;
        let mut j = type_start;
        while j < bytes.len() && (bytes[j].is_ascii_alphabetic()) {
            j += 1;
        }
        let entry_type = src[type_start..j].to_ascii_lowercase();
        // skip whitespace, expect '{'
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'{' {
            i = j;
            continue;
        }
        // Find matching closing brace for the whole record.
        let body_start = j + 1;
        let Some(body_end) = matching_brace(src, j) else { break };
        let body = &src[body_start..body_end];
        i = body_end + 1;

        if entry_type == "comment" || entry_type == "string" || entry_type == "preamble" {
            continue;
        }
        if let Some(entry) = parse_entry_body(&entry_type, body) {
            out.insert(entry.key.clone(), entry);
        }
    }
    out
}

/// Parse the inside of `@type{ ... }`: `key, field = value, field = value`.
fn parse_entry_body(entry_type: &str, body: &str) -> Option<BibEntry> {
    let comma = body.find(',')?;
    let key = body[..comma].trim().to_string();
    if key.is_empty() {
        return None;
    }
    let mut fields = HashMap::new();
    let rest = &body[comma + 1..];
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // field name
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if name_start == i {
            break;
        }
        let name = rest[name_start..i].to_ascii_lowercase();
        // skip to '='
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        i += 1; // past '='
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // value: {...}, "...", or bareword
        let value = match bytes[i] {
            b'{' => {
                let end = matching_brace(rest, i)?;
                let v = rest[i + 1..end].to_string();
                i = end + 1;
                v
            }
            b'"' => {
                let start = i + 1;
                let mut k = start;
                while k < bytes.len() && bytes[k] != b'"' {
                    k += 1;
                }
                let v = rest[start..k].to_string();
                i = k + 1;
                v
            }
            _ => {
                let start = i;
                while i < bytes.len() && bytes[i] != b',' && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                rest[start..i].to_string()
            }
        };
        fields.insert(name, normalize_ws(&value));
    }
    Some(BibEntry { key, entry_type: entry_type.to_string(), fields })
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Byte index of the `}` matching the `{` at `open`. None if unbalanced.
fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Resolve every `[@key]`, `[@k1; @k2]` and `\cite{k1,k2}` in `md` against `bib`,
/// replacing markers with `[n]` (numbered by first appearance) and appending a
/// `## References` list of the cited works. Unknown keys are left untouched.
/// Returns the markdown unchanged when there are no resolvable citations.
pub fn process_citations(md: &str, bib: &HashMap<String, BibEntry>) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut number: HashMap<String, usize> = HashMap::new();

    let mut assign = |key: &str| -> Option<usize> {
        if !bib.contains_key(key) {
            return None;
        }
        if let Some(n) = number.get(key) {
            return Some(*n);
        }
        order.push(key.to_string());
        let n = order.len();
        number.insert(key.to_string(), n);
        Some(n)
    };

    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    loop {
        // Next citation token: a Pandoc `[@` or a LaTeX `\cite{`.
        let pa = rest.find("[@");
        let la = rest.find("\\cite{");
        let next = match (pa, la) {
            (None, None) => break,
            (Some(a), None) => (a, true),
            (None, Some(b)) => (b, false),
            (Some(a), Some(b)) => if a < b { (a, true) } else { (b, false) },
        };
        let (pos, is_pandoc) = next;
        out.push_str(&rest[..pos]);

        if is_pandoc {
            // [@k], [@k1; @k2]
            if let Some(close_rel) = rest[pos..].find(']') {
                let inner = &rest[pos + 1..pos + close_rel]; // "@k1; @k2"
                if let Some(repl) = render_keys(inner, "; ", '@', &mut assign) {
                    out.push_str(&repl);
                    rest = &rest[pos + close_rel + 1..];
                    continue;
                }
            }
            // Not a resolvable citation - emit the literal `[` and move on.
            out.push_str("[@");
            rest = &rest[pos + 2..];
        } else {
            // \cite{k1,k2}
            let open = pos + "\\cite".len();
            if let Some(end_rel) = rest[open..].find('}') {
                let inner = &rest[open + 1..open + end_rel]; // "k1,k2"
                if let Some(repl) = render_keys(inner, ",", '\0', &mut assign) {
                    out.push_str(&repl);
                    rest = &rest[open + end_rel + 1..];
                    continue;
                }
            }
            out.push_str("\\cite{");
            rest = &rest[pos + "\\cite{".len()..];
        }
    }
    out.push_str(rest);

    if order.is_empty() {
        return md.to_string();
    }

    // Append the reference list in citation order.
    out.push_str("\n\n## References\n\n");
    for (idx, key) in order.iter().enumerate() {
        let entry = &bib[key];
        out.push_str(&format!("[{}] {}\n\n", idx + 1, entry.format_numeric()));
    }
    out.trim_end().to_string()
}

/// Render a group of citation keys ("@k1; @k2" or "k1,k2") as `[n]` or `[n, m]`.
/// Returns None if NO key in the group resolves (so the caller leaves it literal).
fn render_keys(
    inner: &str,
    sep: &str,
    strip: char,
    assign: &mut impl FnMut(&str) -> Option<usize>,
) -> Option<String> {
    let mut nums: Vec<usize> = Vec::new();
    for raw in inner.split(sep) {
        let mut k = raw.trim();
        if strip != '\0' {
            k = k.trim_start_matches(strip).trim();
        }
        if k.is_empty() {
            continue;
        }
        if let Some(n) = assign(k) {
            nums.push(n);
        } else {
            return None; // unknown key → leave the whole marker untouched
        }
    }
    if nums.is_empty() {
        return None;
    }
    let joined = nums.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    Some(format!("[{}]", joined))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIB: &str = r#"
@article{shor1997,
  author = {Peter W. Shor},
  title = {Polynomial-Time Algorithms for Prime Factorization},
  journal = {SIAM Journal on Computing},
  year = {1997}
}
@book{nielsen2010,
  author = {Michael A. Nielsen and Isaac L. Chuang},
  title = {Quantum Computation and Quantum Information},
  publisher = {Cambridge University Press},
  year = {2010}
}
"#;

    #[test]
    fn parses_bibtex_fields() {
        let db = parse_bibtex(BIB);
        assert_eq!(db.len(), 2);
        assert_eq!(db["shor1997"].get("year"), "1997");
        assert_eq!(db["nielsen2010"].entry_type, "book");
        assert!(db["nielsen2010"].get("title").starts_with("Quantum Computation"));
    }

    #[test]
    fn resolves_pandoc_and_latex_citations_and_appends_refs() {
        let db = parse_bibtex(BIB);
        let md = "Factoring is hard [@shor1997]. See also \\cite{nielsen2010} and [@shor1997] again.";
        let out = process_citations(md, &db);
        assert!(out.contains("hard [1]."), "first citation numbered [1]: {out}");
        assert!(out.contains("\\cite") == false, "latex cite resolved");
        assert!(out.contains("[2]"), "second distinct work is [2]");
        assert!(out.contains("again."));
        // Repeated key keeps its number (count in the body, excluding the
        // reference list which also contains "[1]").
        let body = out.split("## References").next().unwrap();
        assert_eq!(body.matches("[1]").count(), 2);
        assert!(out.contains("## References"));
        assert!(out.contains("[1] Peter W. Shor."));
        assert!(out.contains("[2] Michael A. Nielsen, Isaac L. Chuang."));
    }

    #[test]
    fn grouped_citation() {
        let db = parse_bibtex(BIB);
        let md = "Both works [@shor1997; @nielsen2010] are seminal.";
        let out = process_citations(md, &db);
        assert!(out.contains("[1, 2]"), "grouped → [1, 2]: {out}");
    }

    #[test]
    fn unknown_key_left_untouched_and_no_refs_when_none() {
        let db = parse_bibtex(BIB);
        let md = "Mystery [@unknown2020] reference.";
        let out = process_citations(md, &db);
        assert_eq!(out, md, "unknown key untouched, no reference list");
    }
}
