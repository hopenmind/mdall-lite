//! Cross-reference resolution: numbering for sections, display equations and
//! figures, plus resolution of `\ref` / `\eqref` / `\cref` against `\label`s.
//!
//! Runs as an EXPORT pre-pass (on the markdown handed to the exporters), so the
//! editor source is never mutated with literal numbers - numbering stays
//! automatic and reorder-safe. Labels can be written `\label{key}` anywhere on a
//! heading/figure line or inside a `$$...$$` block, or `{#key}` on a heading.
//!
//! Numbering: sections are dotted by ATX level (1, 1.1, 1.1.1); display
//! equations and figures are counted sequentially. `\eqref` renders `(N)`,
//! `\ref`/`\cref` render `N`.

use std::collections::HashMap;

/// Resolve cross-references in `md`. Returns the markdown unchanged if it has no
/// `\ref`/`\eqref`/`\cref`/`\label`/`{#...}` markers (cheap no-op for plain docs).
pub fn resolve_crossrefs(md: &str) -> String {
    if !md.contains("\\ref{")
        && !md.contains("\\eqref{")
        && !md.contains("\\cref{")
        && !md.contains("\\Cref{")
        && !md.contains("\\label{")
        && !md.contains("{#")
    {
        return md.to_string();
    }

    let labels = build_labels(md);
    rewrite(md, &labels)
}

/// First pass: assign numbers and map each label to its number string.
fn build_labels(md: &str) -> HashMap<String, String> {
    let mut labels = HashMap::new();
    let mut counters = [0u32; 6];
    let mut eqno = 0u32;
    let mut figno = 0u32;
    let mut in_fence = false;
    let mut in_eq_block = false;
    let mut eq_pending_label: Option<String> = None;

    for line in md.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        // Display-equation block boundaries / single-line `$$ ... $$`.
        let dollar_pairs = t.matches("$$").count();
        if in_eq_block {
            if let Some(k) = extract_label(line) {
                eq_pending_label = Some(k);
            }
            if dollar_pairs % 2 == 1 {
                // closing $$
                in_eq_block = false;
                if let Some(k) = eq_pending_label.take() {
                    labels.insert(k, format!("{}", eqno));
                }
            }
            continue;
        }
        if t.starts_with("$$") {
            eqno += 1;
            let lbl = extract_label(line);
            if dollar_pairs >= 2 {
                // single-line equation
                if let Some(k) = lbl {
                    labels.insert(k, format!("{}", eqno));
                }
            } else {
                in_eq_block = true;
                eq_pending_label = lbl;
            }
            continue;
        }

        // Headings.
        if let Some(level) = atx_level(t) {
            counters[level - 1] += 1;
            for c in counters.iter_mut().skip(level) {
                *c = 0;
            }
            let num = counters[..level]
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(".");
            if let Some(k) = extract_label(line).or_else(|| extract_hash_id(line)) {
                labels.insert(k, num);
            }
            continue;
        }

        // Figures: a line that is (mostly) a single image.
        if is_figure_line(t) {
            figno += 1;
            if let Some(k) = extract_label(line) {
                labels.insert(k, format!("{}", figno));
            }
        }
    }
    labels
}

/// Second pass: inject section numbers, strip labels, resolve references.
fn rewrite(md: &str, labels: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    let mut counters = [0u32; 6];
    let mut in_fence = false;

    for (i, line) in md.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }

        let mut s = line.to_string();

        // Number headings by re-deriving the counter (same walk as pass 1).
        if let Some(level) = atx_level(t) {
            counters[level - 1] += 1;
            for c in counters.iter_mut().skip(level) {
                *c = 0;
            }
            let num = counters[..level]
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(".");
            s = number_heading(&s, level, &num);
        }

        // Strip label / hash-id markers (consumed; never shown).
        s = strip_command_arg(&s, "\\label");
        s = strip_hash_id(&s);

        // Resolve references.
        s = resolve_refs(&s, labels);

        out.push_str(&s);
    }
    out
}

/// ATX heading level (1-6) of a left-trimmed line, or None.
fn atx_level(t: &str) -> Option<usize> {
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    if hashes >= 1 && hashes <= 6 {
        // Must be followed by a space (CommonMark) to be a heading.
        if t.as_bytes().get(hashes) == Some(&b' ') {
            return Some(hashes);
        }
    }
    None
}

/// Inject `num ` right after the heading hashes (and the single space).
fn number_heading(line: &str, level: usize, num: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, body) = line.split_at(indent_len);
    // body = "## Title..."; insert after `level` hashes + 1 space.
    let prefix_len = level + 1;
    let (hashes, title) = body.split_at(prefix_len);
    format!("{}{}{} {}", indent, hashes, num, title.trim_start())
}

fn is_figure_line(t: &str) -> bool {
    let t = t.trim();
    t.starts_with("![") && t.contains("](")
}

/// Extract the key from the first `\label{key}` in the line.
fn extract_label(line: &str) -> Option<String> {
    let p = line.find("\\label{")?;
    let start = p + "\\label{".len();
    let end = line[start..].find('}')? + start;
    Some(line[start..end].to_string())
}

/// Extract the key from a `{#key}` heading id.
fn extract_hash_id(line: &str) -> Option<String> {
    let p = line.find("{#")?;
    let start = p + 2;
    let end = line[start..].find('}')? + start;
    let k = line[start..end].trim();
    if k.is_empty() { None } else { Some(k.to_string()) }
}

/// Remove the first `{#id}` token from a line.
fn strip_hash_id(line: &str) -> String {
    if let Some(p) = line.find("{#") {
        if let Some(rel) = line[p..].find('}') {
            let mut s = String::with_capacity(line.len());
            s.push_str(line[..p].trim_end());
            s.push_str(&line[p + rel + 1..]);
            return s;
        }
    }
    line.to_string()
}

/// Remove every `\cmd{...}` occurrence from a line (used for `\label`).
fn strip_command_arg(line: &str, cmd: &str) -> String {
    let pat = format!("{}{{", cmd);
    let mut s = line.to_string();
    while let Some(p) = s.find(&pat) {
        if let Some(rel) = s[p + pat.len()..].find('}') {
            let end = p + pat.len() + rel + 1;
            // Also swallow a single trailing space the label left behind.
            let mut tail = end;
            if s.as_bytes().get(tail) == Some(&b' ') {
                tail += 1;
            }
            s.replace_range(p..tail, "");
        } else {
            break;
        }
    }
    s
}

/// Replace `\eqref{k}` → `(N)`, `\ref{k}`/`\cref{k}`/`\Cref{k}` → `N`. Unknown
/// labels render `?` (a visible, honest broken-reference marker).
fn resolve_refs(line: &str, labels: &HashMap<String, String>) -> String {
    let mut s = line.to_string();
    for (cmd, parens) in [("\\eqref{", true), ("\\ref{", false), ("\\cref{", false), ("\\Cref{", false)] {
        loop {
            let Some(p) = s.find(cmd) else { break };
            let key_start = p + cmd.len();
            let Some(rel) = s[key_start..].find('}') else { break };
            let key = s[key_start..key_start + rel].to_string();
            let num = labels.get(&key).cloned().unwrap_or_else(|| "?".to_string());
            let repl = if parens { format!("({})", num) } else { num };
            s.replace_range(p..key_start + rel + 1, &repl);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_numbered_and_refs_resolved() {
        let md = "\
# Introduction {#sec:intro}

## Background

See \\ref{sec:intro} for context.

# Methods \\label{sec:methods}

As in \\cref{sec:methods}.";
        let out = resolve_crossrefs(md);
        assert!(out.contains("# 1 Introduction"), "{out}");
        assert!(out.contains("## 1.1 Background"), "{out}");
        assert!(out.contains("# 2 Methods"), "{out}");
        assert!(out.contains("See 1 for context."));
        assert!(out.contains("As in 2."));
        assert!(!out.contains("\\label"), "labels stripped");
        assert!(!out.contains("{#"), "hash ids stripped");
    }

    #[test]
    fn equations_numbered_and_eqref() {
        let md = "\
Energy below:

$$ E = mc^2 \\label{eq:emc} $$

and a multi-line one:

$$
\\int_0^1 x\\,dx = \\tfrac12
\\label{eq:int}
$$

By \\eqref{eq:emc} and \\eqref{eq:int}.";
        let out = resolve_crossrefs(md);
        assert!(out.contains("By (1) and (2)."), "{out}");
        assert!(!out.contains("\\label"));
    }

    #[test]
    fn unknown_ref_is_marked() {
        let md = "See \\ref{nope}.";
        assert_eq!(resolve_crossrefs(md), "See ?.");
    }

    #[test]
    fn plain_doc_is_unchanged() {
        let md = "# Title\n\nNo refs here, just **prose**.";
        assert_eq!(resolve_crossrefs(md), md);
    }

    #[test]
    fn refs_inside_code_fence_untouched() {
        let md = "\
# Heading {#h}

```text
\\ref{h} stays literal in code
```

Out here \\ref{h}.";
        let out = resolve_crossrefs(md);
        assert!(out.contains("\\ref{h} stays literal in code"));
        assert!(out.contains("Out here 1."));
    }
}
