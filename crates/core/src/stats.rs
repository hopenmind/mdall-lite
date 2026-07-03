//! Document statistics for the editor status bar (word count, ...).

/// Count words in a Markdown document the way a writer expects: fenced code
/// blocks are skipped, and only whitespace-separated tokens containing at least
/// one alphanumeric character are counted (so bare markup like `**`, `---` or a
/// lone `#` is not miscounted as a word).
pub fn word_count(md: &str) -> usize {
    let mut count = 0usize;
    let mut in_fence = false;
    for line in md.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for tok in line.split_whitespace() {
            if tok.chars().any(|c| c.is_alphanumeric()) {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_prose_words_ignoring_markup() {
        assert_eq!(word_count("Hello brave new world"), 4);
        // Pure-markup tokens are not words.
        assert_eq!(word_count("# Title"), 1);
        assert_eq!(word_count("**bold** and *italic*"), 3); // bold, and, italic
        assert_eq!(word_count("---"), 0);
    }

    #[test]
    fn skips_fenced_code_blocks() {
        let md = "Real prose here.\n\n```rust\nfn main() { let x = 1; }\n```\n\nMore prose.";
        // "Real prose here." = 3, "More prose." = 2 → 5; code body excluded.
        assert_eq!(word_count(md), 5);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("   \n\n  "), 0);
    }
}
