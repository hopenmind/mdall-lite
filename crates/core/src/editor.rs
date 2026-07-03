/// Semantic document block model for the rich WYSIWYG editor.
///
/// The document is split into typed blocks, each carrying its exact byte range
/// inside the full Markdown source.  This lets the editor render each block
/// with appropriate visual styling while keeping a single source-of-truth
/// (`MdApp::source`) that both the raw TextEdit and the rich view modify.

use std::ops::Range;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DocumentBlock {
    pub kind: BlockKind,
    /// Byte range inside the full Markdown source string.
    pub source_range: Range<usize>,
}

#[derive(Clone, Debug)]
pub enum BlockKind {
    /// ATX heading `# ... ######`. `level` is 1-6.
    Heading(u8),
    /// Regular text paragraph (may contain inline bold / italic / code / inline-math).
    Paragraph,
    /// Display equation `$$ ... $$` or `\[ ... \]`.
    DisplayEquation { latex: String, index: usize },
    /// Fenced code block ` ``` ... ``` `.
    FencedCode { lang: String },
    /// Bullet list (`-` / `*` / `+`).
    BulletList,
    /// Ordered list (`1.` / `2.` ...).
    OrderedList,
    /// Blockquote (`> ...`).
    BlockQuote,
    /// Thematic break (`---` / `***` / `___`).
    HorizontalRule,
    /// Pipe table.
    Table,
    /// Raw HTML block / alignment `<div>` etc.
    HtmlBlock,
}

impl DocumentBlock {
    /// Slice the raw Markdown source for this block.
    #[inline]
    pub fn raw_source<'a>(&self, full: &'a str) -> &'a str {
        let end   = self.source_range.end.min(full.len());
        let start = self.source_range.start.min(end);
        &full[start..end]
    }
}

// ── Block-type predicates ─────────────────────────────────────────────────────

/// Return the ATX heading level (1-6) if this line is an ATX heading.
pub fn heading_level(line: &str) -> Option<u8> {
    let t = line.trim_start();
    if !t.starts_with('#') { return None; }
    let level = t.bytes().take_while(|&b| b == b'#').count() as u8;
    if level > 6 { return None; }
    let after = &t[level as usize..];
    if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
        Some(level)
    } else {
        None
    }
}

fn is_horizontal_rule(t: &str) -> bool {
    if t.len() < 3 { return false; }
    let ch = t.chars().find(|c| !c.is_whitespace());
    matches!(ch, Some('-') | Some('*') | Some('_'))
        && t.chars().all(|c| c == ch.unwrap() || c == ' ')
        && t.chars().filter(|&c| !c.is_whitespace()).count() >= 3
}

fn is_bullet_item(t: &str) -> bool {
    matches!(t.get(..2), Some("- ") | Some("* ") | Some("+ "))
        || matches!(t, "-" | "*" | "+")
}

fn is_ordered_item(t: &str) -> bool {
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() { return false; }
    let rest = &t[digits.len()..];
    rest.starts_with(". ") || rest.starts_with(") ")
}

fn is_table_row(t: &str) -> bool {
    t.starts_with('|')
}

/// If `t` opens a LaTeX math environment (`\begin{equation}`, `\begin{align}`,
/// `\begin{aligned}`, ...), return the environment name (with any trailing `*`).
/// Non-math environments (itemize, figure, table, ...) return `None`.
pub fn math_env_begin(t: &str) -> Option<String> {
    let rest = t.trim_start().strip_prefix("\\begin{")?;
    let end = rest.find('}')?;
    let env = &rest[..end];
    let base = env.trim_end_matches('*');
    const MATH_ENVS: &[&str] = &[
        "equation", "align", "aligned", "gather", "gathered", "multline",
        "eqnarray", "displaymath", "math", "split", "alignat", "flalign", "cases",
    ];
    if MATH_ENVS.contains(&base) { Some(env.to_string()) } else { None }
}

/// Returns true if `line` starts a new block type that should break
/// an ongoing paragraph accumulation.
fn is_block_starter(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() { return true; }
    if heading_level(line).is_some() { return true; }
    if t.starts_with("$$") || t.starts_with("\\[") { return true; }
    if math_env_begin(t).is_some() { return true; }
    if t.starts_with("```") { return true; }
    if is_horizontal_rule(t) { return true; }
    if is_bullet_item(t) { return true; }
    if is_ordered_item(t) { return true; }
    if is_table_row(t) { return true; }
    if t.starts_with('>') { return true; }
    false
}

/// True if a line begins with an INLINE-formatting HTML tag, with or without
/// attributes: `<span style="color:#...">`, `<mark style=...>`, `<u>`, `<sup>`,
/// `<b>`, `<a href=...>`, ... These are inline runs the editor renders in place
/// (tag hidden, style applied), so they must NOT be captured as block-level HTML,
/// which would leak the raw tag and split the paragraph. Block containers like
/// `<div>`, `<table>`, `<figure>` are NOT inline and stay block-level.
fn starts_inline_html(line: &str) -> bool {
    let low = line.trim_start().to_ascii_lowercase();
    const INLINE: &[&str] = &[
        "span", "mark", "u", "sup", "sub", "b", "i", "s",
        "em", "strong", "del", "ins", "a", "code", "small", "abbr", "kbd",
    ];
    for name in INLINE {
        for opener in [format!("<{name}"), format!("</{name}")] {
            if let Some(rest) = low.strip_prefix(opener.as_str()) {
                // Only a real tag boundary counts, so `<u>` matches but `<ul>` does not.
                if rest.is_empty()
                    || rest.starts_with(' ')
                    || rest.starts_with('>')
                    || rest.starts_with('/')
                {
                    return true;
                }
            }
        }
    }
    false
}

// ── Parser ───────────────────────────────────────────────────────────────────

pub fn parse_document(markdown: &str) -> Vec<DocumentBlock> {
    let mut blocks: Vec<DocumentBlock> = Vec::new();

    // Build a list of (line_content, byte_start, byte_end_including_newline)
    let mut lines: Vec<(String, usize, usize)> = Vec::new();
    {
        let mut pos = 0;
        let bytes = markdown.as_bytes();
        while pos < markdown.len() {
            let start = pos;
            while pos < markdown.len() && bytes[pos] != b'\n' { pos += 1; }
            let content = markdown[start..pos].trim_end_matches('\r').to_string();
            let next = if pos < markdown.len() { pos + 1 } else { pos };
            lines.push((content, start, next));
            pos = next;
        }
    }

    let mut i = 0;
    let mut eq_index = 0;

    while i < lines.len() {
        let line = lines[i].0.as_str();
        let lbs  = lines[i].1;
        let lbe  = lines[i].2;
        let t    = line.trim();

        // ── Blank line - block boundary, no output ──────────────────────────
        if t.is_empty() { i += 1; continue; }

        // ── ATX Heading ─────────────────────────────────────────────────────
        if let Some(level) = heading_level(line) {
            blocks.push(DocumentBlock { kind: BlockKind::Heading(level), source_range: lbs..lbe });
            i += 1;
            continue;
        }

        // ── LaTeX math environment (\begin{align} ... \end{align}) ────────────
        // Real papers (.tex / converted HTML) wrap display math in environments,
        // not just $$...$$. Without this they fall to a paragraph and show raw LaTeX.
        if let Some(env) = math_env_begin(t) {
            let eq_start = lbs;
            let end_tag = format!("\\end{{{}}}", env);
            let mut eq_end = lbe;
            i += 1;
            while i < lines.len() {
                let nlbe = lines[i].2;
                let has_end = lines[i].0.contains(&end_tag);
                eq_end = nlbe;
                i += 1;
                if has_end { break; }
            }
            let safe = eq_end.min(markdown.len());
            let latex = markdown[eq_start..safe].trim().to_string();
            blocks.push(DocumentBlock {
                kind: BlockKind::DisplayEquation { latex, index: eq_index },
                source_range: eq_start..eq_end,
            });
            eq_index += 1;
            continue;
        }

        // ── Display equation ─────────────────────────────────────────────────
        {
            let is_dd = t.starts_with("$$");
            let is_lb = t.starts_with("\\[");
            if is_dd || is_lb {
                let open  = if is_dd { "$$" } else { "\\[" };
                let close = if is_dd { "$$" } else { "\\]" };
                let after = t.trim_start_matches(open).trim();

                // The closing delimiter may sit ANYWHERE on a line (e.g.
                // `...\textbf{[TRAP]}$$ (1.1)` with a trailing equation number),
                // not only at the end. Detect it with `find`, take the LaTeX up
                // to it, and bound the block there. Trailing text after the close
                // (a label like `(1.1)`) is absorbed into the block range but kept
                // out of the rendered LaTeX. Without this, an inline-closed `$$`
                // is never matched and the equation swallows the rest of the doc.
                if let Some(pos) = after.find(close) {
                    // Single-line: $$ content $$ [trailing]
                    let latex = after[..pos].trim().to_string();
                    blocks.push(DocumentBlock {
                        kind: BlockKind::DisplayEquation { latex, index: eq_index },
                        source_range: lbs..lbe,
                    });
                    eq_index += 1;
                    i += 1;
                } else {
                    // Multi-line: accumulate until a line that CONTAINS the close.
                    // GUARD: a display equation is contiguous math; it never spans a
                    // blank line or a heading. If we reach one before the close, the
                    // `$$` is unclosed / malformed -> bail and treat the opening line
                    // as a normal paragraph, so a single stray `$$` cannot swallow the
                    // rest of the document (headings, tables) into one giant equation
                    // that then breaks the math renderer.
                    let eq_start = lbs;
                    let mut latex = if after.is_empty() { String::new() } else { after.to_string() };
                    let mut j = i + 1;
                    let mut eq_end = lbe;
                    let mut closed = false;
                    while j < lines.len() {
                        let nl   = lines[j].0.as_str();
                        let nt   = nl.trim();
                        let nlbe = lines[j].2;
                        if let Some(pos) = nt.find(close) {
                            let pre = nt[..pos].trim();
                            if !pre.is_empty() { if !latex.is_empty() { latex.push('\n'); } latex.push_str(pre); }
                            eq_end = nlbe;
                            j += 1;
                            closed = true;
                            break;
                        }
                        if nt.is_empty() || heading_level(nl).is_some() {
                            break; // unclosed: do not swallow past a blank line / heading
                        }
                        if !latex.is_empty() { latex.push('\n'); }
                        latex.push_str(nt);
                        eq_end = nlbe;
                        j += 1;
                    }
                    if closed {
                        blocks.push(DocumentBlock {
                            kind: BlockKind::DisplayEquation { latex, index: eq_index },
                            source_range: eq_start..eq_end,
                        });
                        eq_index += 1;
                        i = j;
                    } else {
                        // Unclosed `$$`: keep only the opening line, as a paragraph.
                        blocks.push(DocumentBlock {
                            kind: BlockKind::Paragraph,
                            source_range: lbs..lbe,
                        });
                        i += 1;
                    }
                }
                continue;
            }
        }

        // ── Fenced code block ─────────────────────────────────────────────────
        if t.starts_with("```") {
            let lang = t.trim_start_matches('`').trim().to_string();
            let code_start = lbs;
            let mut code_end = lbe;
            i += 1;
            while i < lines.len() {
                let nl   = lines[i].0.as_str();
                let nlbe = lines[i].2;
                let nt   = nl.trim();
                if nt.starts_with("```") {
                    code_end = nlbe;
                    i += 1;
                    break;
                }
                code_end = nlbe;
                i += 1;
            }
            blocks.push(DocumentBlock { kind: BlockKind::FencedCode { lang }, source_range: code_start..code_end });
            continue;
        }

        // ── Horizontal rule ───────────────────────────────────────────────────
        if is_horizontal_rule(t) {
            blocks.push(DocumentBlock { kind: BlockKind::HorizontalRule, source_range: lbs..lbe });
            i += 1;
            continue;
        }

        // ── Table ─────────────────────────────────────────────────────────────
        if is_table_row(t) {
            let tbl_start = lbs;
            let mut tbl_end = lbe;
            i += 1;
            while i < lines.len() {
                let nl = lines[i].0.as_str();
                let nlbe = lines[i].2;
                if is_table_row(nl.trim()) { tbl_end = nlbe; i += 1; } else { break; }
            }
            blocks.push(DocumentBlock { kind: BlockKind::Table, source_range: tbl_start..tbl_end });
            continue;
        }

        // ── Blockquote ─────────────────────────────────────────────────────────
        if t.starts_with('>') {
            let bq_start = lbs;
            let mut bq_end = lbe;
            i += 1;
            while i < lines.len() {
                let nl = lines[i].0.as_str();
                let nlbe = lines[i].2;
                let nt = nl.trim();
                if nt.starts_with('>') || (!nt.is_empty() && !is_block_starter(nl)) {
                    bq_end = nlbe; i += 1;
                } else { break; }
            }
            blocks.push(DocumentBlock { kind: BlockKind::BlockQuote, source_range: bq_start..bq_end });
            continue;
        }

        // ── Bullet list ─────────────────────────────────────────────────────────
        if is_bullet_item(t) {
            let ls = lbs;
            let mut le = lbe;
            i += 1;
            while i < lines.len() {
                let nl = lines[i].0.as_str();
                let nlbe = lines[i].2;
                let nt = nl.trim();
                if nt.is_empty() {
                    if i + 1 < lines.len() && is_bullet_item(lines[i+1].0.trim()) { le = nlbe; i += 1; } else { break; }
                } else if is_bullet_item(nt) || nl.starts_with("  ") || nl.starts_with('\t') {
                    le = nlbe; i += 1;
                } else { break; }
            }
            blocks.push(DocumentBlock { kind: BlockKind::BulletList, source_range: ls..le });
            continue;
        }

        // ── Ordered list ────────────────────────────────────────────────────────
        if is_ordered_item(t) {
            let ls = lbs;
            let mut le = lbe;
            i += 1;
            while i < lines.len() {
                let nl = lines[i].0.as_str();
                let nlbe = lines[i].2;
                let nt = nl.trim();
                if nt.is_empty() {
                    if i + 1 < lines.len() && is_ordered_item(lines[i+1].0.trim()) { le = nlbe; i += 1; } else { break; }
                } else if is_ordered_item(nt) || nl.starts_with("  ") || nl.starts_with('\t') {
                    le = nlbe; i += 1;
                } else { break; }
            }
            blocks.push(DocumentBlock { kind: BlockKind::OrderedList, source_range: ls..le });
            continue;
        }

        // ── HTML block (`<div>`, `<p>`, alignment wrappers, ...) ──────────────────
        if t.starts_with('<') && !starts_inline_html(t) {
            let hs = lbs;
            let mut he = lbe;
            if t.to_ascii_lowercase().starts_with("<div") {
                // A <div> container is ONE block up to its matching </div>, even
                // across blank lines (the styled-box render needs the blank lines so
                // the interior is parsed as Markdown, yet must stay one block so it
                // renders as a box instead of splitting into raw fragments + gaps).
                let depth_of = |s: &str| {
                    let l = s.to_ascii_lowercase();
                    l.matches("<div").count() as i32 - l.matches("</div").count() as i32
                };
                let mut depth = depth_of(line);
                let mut j = i + 1;
                let mut end = lbe;
                while depth > 0 && j < lines.len() {
                    end = lines[j].2;
                    depth += depth_of(lines[j].0.as_str());
                    j += 1;
                }
                if depth <= 0 {
                    he = end;
                    i = j;
                } else {
                    // Unclosed div: keep just the opening line, don't swallow the doc.
                    i += 1;
                }
            } else {
                i += 1;
                while i < lines.len() {
                    let nt = lines[i].0.trim();
                    he = lines[i].2;
                    i += 1;
                    if nt.is_empty() || (nt.starts_with("</") && nt.ends_with('>')) { break; }
                }
            }
            blocks.push(DocumentBlock { kind: BlockKind::HtmlBlock, source_range: hs..he });
            continue;
        }

        // ── Paragraph - accumulate until a block starter or blank line ───────────
        {
            let ps = lbs;
            let mut pe = lbe;
            i += 1;
            while i < lines.len() {
                let nl = lines[i].0.as_str();
                let nlbe = lines[i].2;
                if is_block_starter(nl) { break; }
                pe = nlbe;
                i += 1;
            }
            blocks.push(DocumentBlock { kind: BlockKind::Paragraph, source_range: ps..pe });
        }
    }

    blocks
}

// ── Code-block content extractor ─────────────────────────────────────────────

/// Strip the opening ` ```lang ` and closing ` ``` ` fences and return the
/// bare code content.
pub fn code_block_content(raw: &str) -> (String, String) {
    let mut lines = raw.lines();
    let lang = lines.next()
        .map(|l| l.trim().trim_start_matches('`').trim().to_string())
        .unwrap_or_default();
    let mut code = lines
        .take_while(|l| !l.trim().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n");
    if code.ends_with('\n') { code.pop(); }
    (lang, code)
}

#[cfg(test)]
mod eq_parse_tests {
    use super::*;

    #[test]
    fn display_eq_inline_close_does_not_swallow_rest() {
        // Closing `$$` mid-line, followed by a `(1.1)` label, then more content.
        let md = "$$\\frac{a}{b} = c\n\\quad x$$ (1.1)\n\nNext paragraph here.\n\n## Heading\n";
        let blocks = parse_document(md);
        let eqs: Vec<_> = blocks.iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayEquation { .. })).collect();
        assert_eq!(eqs.len(), 1, "exactly one display equation");
        if let BlockKind::DisplayEquation { latex, .. } = &eqs[0].kind {
            assert!(latex.contains("\\frac{a}{b}"), "latex captured: {latex:?}");
            assert!(!latex.contains("Next paragraph"), "equation must not swallow following text");
            assert!(!latex.contains("Heading"));
        }
        assert!(blocks.iter().any(|b|
            matches!(b.kind, BlockKind::Paragraph) && b.raw_source(md).contains("Next paragraph")),
            "following paragraph must be its own block");
        assert!(blocks.iter().any(|b| matches!(b.kind, BlockKind::Heading(2))));
    }

    #[test]
    fn display_eq_single_line_with_trailing_label() {
        let md = "$$E = mc^2$$ (1.1)\n\nAfter.\n";
        let blocks = parse_document(md);
        let eqs: Vec<_> = blocks.iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayEquation { .. })).collect();
        assert_eq!(eqs.len(), 1);
        if let BlockKind::DisplayEquation { latex, .. } = &eqs[0].kind {
            assert_eq!(latex, "E = mc^2");
        }
        assert!(blocks.iter().any(|b|
            matches!(b.kind, BlockKind::Paragraph) && b.raw_source(md).contains("After")));
    }

    #[test]
    fn two_inline_closed_equations_stay_separate() {
        let md = "$$a$$ (1)\n\ntext\n\n$$b$$ (2)\n";
        let eqs = parse_document(md).into_iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayEquation { .. })).count();
        assert_eq!(eqs, 2, "each inline-closed equation is its own block");
    }

    #[test]
    fn latex_math_environment_is_one_equation() {
        let md = "\\begin{equation}\nE = mc^2\n\\end{equation}\n\nAfter text.\n";
        let blocks = parse_document(md);
        let eqs: Vec<_> = blocks.iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayEquation { .. })).collect();
        assert_eq!(eqs.len(), 1);
        if let BlockKind::DisplayEquation { latex, .. } = &eqs[0].kind {
            assert!(latex.contains("E = mc^2"), "latex: {latex:?}");
            assert!(latex.contains("\\begin{equation}"));
        }
        assert!(blocks.iter().any(|b|
            matches!(b.kind, BlockKind::Paragraph) && b.raw_source(md).contains("After text")),
            "text after the environment must be a separate block");
    }

    #[test]
    fn align_star_environment_recognized_not_swallowed() {
        let md = "\\begin{align*}\na &= b \\\\\nc &= d\n\\end{align*}\n\n## Next\n";
        let blocks = parse_document(md);
        let eqs = blocks.iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayEquation { .. })).count();
        assert_eq!(eqs, 1);
        assert!(blocks.iter().any(|b| matches!(b.kind, BlockKind::Heading(2))));
    }

    #[test]
    fn non_math_environment_is_not_an_equation() {
        let md = "\\begin{itemize}\n\\item x\n\\end{itemize}\n";
        let eqs = parse_document(md).iter()
            .filter(|b| matches!(b.kind, BlockKind::DisplayEquation { .. })).count();
        assert_eq!(eqs, 0, "itemize is not a math environment");
    }
}
