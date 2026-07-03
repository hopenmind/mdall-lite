// inline_math.rs - Inline equation splitting for the WYSIWYG editor.
//
// Splits a paragraph's raw Markdown text into alternating Text / Equation runs.
// Supports both $...$ and \(...\) delimiters, preserving the original syntax
// so the source is never modified on round-trip (non-destructive).
//
// Rules for inline math (mirrors CommonMark / pulldown-cmark behaviour):
//   - Opening delimiter: not followed by space or newline.
//   - Closing delimiter: not preceded by space or newline.
//   - $$ is treated as literal (display equations are already parsed as
//     separate DocumentBlock::DisplayEquation entries).
//   - \$ is an escaped dollar - skipped.

/// A text run inside a Paragraph block.
#[derive(Clone, Debug)]
pub enum InlineRun {
    /// Editable plain-text segment (may still contain Markdown inline markup).
    Text(String),
    /// Inline equation - rendered as a Typst PNG.
    Equation {
        latex:       String,
        delim_open:  String, // "$" or "\\("
        delim_close: String, // "$" or "\\)"
    },
}

/// Quick check: does `s` contain any potential inline-math delimiter?
/// Used to decide whether to run the full splitter.
#[inline]
pub fn needs_reparse(s: &str) -> bool {
    s.contains('$') || s.contains("\\(")
}

/// Split a paragraph text into alternating `Text` and `Equation` runs.
///
/// Handles `$...$` and `\(...\)`. Preserves delimiters for lossless
/// round-tripping via [`serialize_runs`].
pub fn split_inline(text: &str) -> Vec<InlineRun> {
    let mut runs: Vec<InlineRun> = Vec::new();
    let bytes  = text.as_bytes();
    let len    = bytes.len();
    let mut i  = 0usize;
    let mut text_start = 0usize;

    macro_rules! flush_text {
        ($end:expr) => {
            if text_start < $end {
                runs.push(InlineRun::Text(text[text_start..$end].to_string()));
            }
        };
    }

    while i < len {
        // ── \( ... \) ─────────────────────────────────────────────────────
        if i + 1 < len && bytes[i] == b'\\' && bytes[i + 1] == b'(' {
            let eq_start = i + 2;
            // Find matching \)
            let mut j = eq_start;
            let mut found = None;
            while j + 1 < len {
                if bytes[j] == b'\\' && bytes[j + 1] == b')' {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(end) = found {
                let content = &text[eq_start..end];
                if !content.is_empty()
                    && !content.starts_with(' ')
                    && !content.ends_with(' ')
                {
                    flush_text!(i);
                    runs.push(InlineRun::Equation {
                        latex:       content.to_string(),
                        delim_open:  "\\(".to_string(),
                        delim_close: "\\)".to_string(),
                    });
                    i          = end + 2;
                    text_start = i;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        // ── $ ... $ ───────────────────────────────────────────────────────
        if bytes[i] == b'$' {
            // Skip \$
            if i > 0 && bytes[i - 1] == b'\\' {
                i += 1;
                continue;
            }
            // Skip $$ (display math - already a separate block)
            if i + 1 < len && bytes[i + 1] == b'$' {
                i += 2;
                continue;
            }
            let eq_start = i + 1;
            // Opening: next char must not be space/newline
            if eq_start < len
                && bytes[eq_start] != b' '
                && bytes[eq_start] != b'\n'
                && bytes[eq_start] != b'\r'
            {
                // Find closing $
                let mut j     = eq_start;
                let mut found = None;
                while j < len {
                    if bytes[j] == b'$' {
                        // Closing: prev char must not be space/newline
                        if j > eq_start
                            && bytes[j - 1] != b' '
                            && bytes[j - 1] != b'\n'
                        {
                            found = Some(j);
                        }
                        break;
                    }
                    j += 1;
                }
                if let Some(end) = found {
                    let content = &text[eq_start..end];
                    if !content.is_empty() {
                        flush_text!(i);
                        runs.push(InlineRun::Equation {
                            latex:       content.to_string(),
                            delim_open:  "$".to_string(),
                            delim_close: "$".to_string(),
                        });
                        i          = end + 1;
                        text_start = i;
                        continue;
                    }
                }
            }
            i += 1;
            continue;
        }

        i += 1;
    }

    // Remaining text
    if text_start < len {
        runs.push(InlineRun::Text(text[text_start..].to_string()));
    }

    // Degenerate: if no equations were found, return a single Text run
    if runs.iter().all(|r| matches!(r, InlineRun::Text(_))) && runs.len() == 1 {
        return runs;
    }

    runs
}

/// Reconstruct the Markdown source from a run list.
///
/// Output is byte-for-byte identical to the input when nothing was edited
/// (equations preserve their original delimiter style).
pub fn serialize_runs(runs: &[InlineRun]) -> String {
    let mut out = String::new();
    for run in runs {
        match run {
            InlineRun::Text(s) => out.push_str(s),
            InlineRun::Equation { latex, delim_open, delim_close } => {
                out.push_str(delim_open);
                out.push_str(latex);
                out.push_str(delim_close);
            }
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dollar_inline() {
        let runs = split_inline("La précision est $0.514$ avec un intervalle.");
        assert_eq!(runs.len(), 3);
        assert!(matches!(&runs[1], InlineRun::Equation { latex, .. } if latex == "0.514"));
    }

    #[test]
    fn test_backslash_paren() {
        let runs = split_inline("Energy \\(E=mc^2\\) everywhere.");
        assert_eq!(runs.len(), 3);
        if let InlineRun::Equation { latex, delim_open, delim_close } = &runs[1] {
            assert_eq!(latex, "E=mc^2");
            assert_eq!(delim_open, "\\(");
            assert_eq!(delim_close, "\\)");
        } else {
            panic!("expected Equation");
        }
    }

    #[test]
    fn test_serialize_roundtrip() {
        let src = "La précision est $0.514$ avec \\(x^2\\).";
        let runs = split_inline(src);
        assert_eq!(serialize_runs(&runs), src);
    }

    #[test]
    fn test_no_space_rule() {
        // "$ x $" should NOT be parsed as inline math
        let runs = split_inline("test $ x $ done");
        assert_eq!(runs.len(), 1, "space after $ should not match");
    }

    #[test]
    fn test_double_dollar_skipped() {
        let runs = split_inline("text $$not inline$$ here");
        assert!(runs.iter().all(|r| matches!(r, InlineRun::Text(_))),
            "double-dollar must not be parsed as inline");
    }

    #[test]
    fn test_needs_reparse() {
        assert!(needs_reparse("has $math$ here"));
        assert!(needs_reparse("has \\(math\\) here"));
        assert!(!needs_reparse("no math here"));
    }
}
