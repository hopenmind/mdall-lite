// Pure-Rust multi-format export - zero external tools, zero user setup.
// Every format is either generated inline (TXT/TeX/RTF/Typst) or via
// embedded Rust crates (zip for DOCX/ODT, epub-builder for EPUB).

// Each exporter is a pulldown-cmark event-loop state machine. Several formats
// intentionally do not emit some tracked state (e.g. plain TXT drops bold/italic,
// headings reset their level at block end), which produces benign
// "value assigned but never read" / "variable never used" lints for that state -
// silence them at the module level.
#![allow(unused_assignments, unused_variables)]

use crate::export::PdfMetadata;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::Path;

// ── Markdown block splitter ───────────────────────────────────────────────────
// Separates `$$ ... $$` equation blocks from prose.

enum Block<'a> {
    Text(&'a str),
    Equation(&'a str), // raw LaTeX, any display/inline delimiter
}

// ── Math delimiter scanner ────────────────────────────────────────────────────
// Recognises: $$...$$  \[...\]  $...$  \(...\)
// All produce Block::Equation (display vs inline distinction handled by renderer).

#[derive(Clone, Copy)]
enum MathKind { Display, Inline }

/// Find the nearest math opener in `s`, returning (offset, kind).
fn math_next_opening(s: &str) -> Option<(usize, MathKind)> {
    let candidates: [Option<(usize, MathKind)>; 4] = [
        s.find("$$").map(|p| (p, MathKind::Display)),
        s.find("\\[").map(|p| (p, MathKind::Display)),
        s.find("\\(").map(|p| (p, MathKind::Inline)),
        find_single_dollar(s.as_bytes()).map(|p| (p, MathKind::Inline)),
    ];
    candidates.into_iter().flatten().min_by_key(|(pos, _)| *pos)
}

/// Return byte offset of the first `$` that is NOT part of `$$`
/// and NOT immediately followed by whitespace (currency markers).
fn find_single_dollar(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                i += 2; // skip $$
            } else if i + 1 < bytes.len() {
                let nxt = bytes[i + 1];
                if nxt != b' ' && nxt != b'\t' && nxt != b'\n' && nxt != b'\r' {
                    return Some(i);
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Find the closing `$` for an inline equation, skipping any `$$` sequences.
/// Returns (relative offset of `$`, length 1).
fn find_closing_dollar(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                i += 2; // skip $$ inside inline eq (unusual but safe)
            } else {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Split markdown into Text and Equation blocks, recognising all 4 delimiter
/// types.  Equations keep their raw LaTeX content (delimiters stripped).
/// Code fences (```) are passed through as Text without math scanning.
fn split_blocks(src: &str) -> Vec<Block<'_>> {
    let mut out   = Vec::new();
    let mut cursor = 0usize;
    let mut in_code = false;

    // Fast pre-pass: if no math-like characters at all, return as single Text.
    if !src.contains('$') && !src.contains("\\[") && !src.contains("\\(") {
        out.push(Block::Text(src));
        return out;
    }

    while cursor < src.len() {
        // ── Code-fence guard ─────────────────────────────────────────────────
        // Scan up to the next potential math opener, checking for ``` fences.
        let rest = &src[cursor..];

        // Find next math candidate position
        let math_pos = math_next_opening(rest).map(|(p, k)| (p, k));

        // Before the math opener, scan for code fences and skip if inside one.
        let scan_end = math_pos.map(|(p, _)| p).unwrap_or(rest.len());
        if rest[..scan_end].contains("```") {
            // Walk line by line through the pre-math text toggling code state.
            let pre = &rest[..scan_end];
            let mut text_start = 0usize;
            for line in pre.split_inclusive('\n') {
                let trimmed = line.trim_start();
                if trimmed.starts_with("```") {
                    in_code = !in_code;
                }
                text_start += line.len();
            }
            // If we end up inside a code block, emit everything up to math as Text.
            if in_code {
                // Emit everything including the math opener as Text and advance.
                let end = math_pos.map(|(p, _)| cursor + p + 4).unwrap_or(src.len());
                let end = end.min(src.len());
                out.push(Block::Text(&src[cursor..end]));
                cursor = end;
                continue;
            }
        }

        match math_pos {
            None => {
                out.push(Block::Text(&src[cursor..]));
                break;
            }
            Some((rel, MathKind::Display)) => {
                let open = cursor + rel;
                if open > cursor {
                    out.push(Block::Text(&src[cursor..open]));
                }
                // Determine delimiter: $$ or \[
                let (open_len, close_pat) = if src[open..].starts_with("$$") {
                    (2usize, "$$")
                } else {
                    (2usize, "\\]")
                };
                let after = open + open_len;
                if let Some(c) = src[after..].find(close_pat) {
                    let latex = src[after..after + c].trim();
                    if !latex.is_empty() {
                        out.push(Block::Equation(latex));
                    }
                    cursor = after + c + close_pat.len();
                } else {
                    out.push(Block::Text(&src[cursor..]));
                    break;
                }
            }
            Some((rel, MathKind::Inline)) => {
                let open = cursor + rel;
                if open > cursor {
                    out.push(Block::Text(&src[cursor..open]));
                }
                if src[open..].starts_with("\\(") {
                    // \(...\)
                    let after = open + 2;
                    if let Some(c) = src[after..].find("\\)") {
                        let latex = src[after..after + c].trim();
                        if !latex.is_empty() {
                            out.push(Block::Equation(latex));
                        }
                        cursor = after + c + 2;
                    } else {
                        out.push(Block::Text(&src[cursor..]));
                        break;
                    }
                } else {
                    // $...$
                    let after = open + 1;
                    if let Some(c) = find_closing_dollar(&src[after..]) {
                        let latex = src[after..after + c].trim();
                        if !latex.is_empty() {
                            out.push(Block::Equation(latex));
                        }
                        cursor = after + c + 1;
                    } else {
                        out.push(Block::Text(&src[cursor..]));
                        break;
                    }
                }
            }
        }
    }
    out
}

/// Like `split_blocks` but splits out ONLY display equations ($$...$$, \[...\]).
/// Inline math ($...$, \(...\)) stays inside the Text so DOCX can render it as an
/// in-line drawing within the paragraph instead of a separate block.
fn split_display_blocks(src: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut in_code = false;

    if !src.contains("$$") && !src.contains("\\[") {
        out.push(Block::Text(src));
        return out;
    }

    while cursor < src.len() {
        let rest = &src[cursor..];
        let disp: Option<(usize, &str, &str)> = {
            let dd = rest.find("$$").map(|p| (p, "$$", "$$"));
            let br = rest.find("\\[").map(|p| (p, "\\[", "\\]"));
            [dd, br].into_iter().flatten().min_by_key(|(p, _, _)| *p)
        };

        let scan_end = disp.map(|(p, _, _)| p).unwrap_or(rest.len());
        if rest[..scan_end].contains("```") {
            for line in rest[..scan_end].split_inclusive('\n') {
                if line.trim_start().starts_with("```") { in_code = !in_code; }
            }
            if in_code {
                let end = disp
                    .map(|(p, o, _)| cursor + p + o.len())
                    .unwrap_or(src.len())
                    .min(src.len());
                out.push(Block::Text(&src[cursor..end]));
                cursor = end;
                continue;
            }
        }

        match disp {
            None => { out.push(Block::Text(&src[cursor..])); break; }
            Some((rel, open_pat, close_pat)) => {
                let open = cursor + rel;
                if open > cursor { out.push(Block::Text(&src[cursor..open])); }
                let after = open + open_pat.len();
                if let Some(c) = src[after..].find(close_pat) {
                    let latex = src[after..after + c].trim();
                    if !latex.is_empty() { out.push(Block::Equation(latex)); }
                    cursor = after + c + close_pat.len();
                } else {
                    out.push(Block::Text(&src[cursor..]));
                    break;
                }
            }
        }
    }
    out
}

// ── XML/RTF escaping helpers ──────────────────────────────────────────────────

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

fn esc_tex(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\textbackslash{}"),
            '{' => o.push_str("\\{"),
            '}' => o.push_str("\\}"),
            '$' => o.push_str("\\$"),
            '#' => o.push_str("\\#"),
            '%' => o.push_str("\\%"),
            '^' => o.push_str("\\^{}"),
            '&' => o.push_str("\\&"),
            '_' => o.push_str("\\_"),
            '~' => o.push_str("\\textasciitilde{}"),
            c   => o.push(c),
        }
    }
    o
}

fn esc_rtf(s: &str) -> String {
    let mut o = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '{'  => o.push_str("\\{"),
            '}'  => o.push_str("\\}"),
            '\n' => o.push_str("\\line "),
            c if (c as u32) > 127 => o.push_str(&format!("\\u{}?", c as u32)),
            c    => o.push(c),
        }
    }
    o
}

fn esc_typst(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('#', "\\#")
     .replace('*', "\\*")
     .replace('_', "\\_")
     .replace('`', "\\`")
     .replace('$', "\\$")
     .replace('@', "\\@")
}

// ═════════════════════════════════════════════════════════════════════════════
// TXT export - plain text with unicode equation approximations
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_txt(markdown: &str, output_path: &Path) -> Result<(), String> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    let mut h_level: u32 = 0;
    let mut in_code = false;
    let mut is_ordered = false;
    let mut ord_n = 0u64;
    let mut in_image = false; let mut img_alt = String::new();

    for event in Parser::new_ext(markdown, opts) {
        match event {
            Event::Start(Tag::Image { .. }) => { in_image = true; img_alt.clear(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                let label = if img_alt.is_empty() { "image".to_string() } else { std::mem::take(&mut img_alt) };
                out.push_str(&format!("[Image: {}]", label));
                img_alt.clear();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                h_level = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, _=>4 };
            }
            Event::End(TagEnd::Heading(_)) => { out.push_str("\n\n"); h_level = 0; }
            Event::End(TagEnd::Paragraph)  => out.push_str("\n\n"),
            Event::Start(Tag::List(start)) => { is_ordered = start.is_some(); ord_n = start.unwrap_or(1).saturating_sub(1); }
            Event::End(TagEnd::List(_))    => out.push('\n'),
            Event::Start(Tag::Item) => {
                if is_ordered { ord_n += 1; out.push_str(&format!("  {}. ", ord_n)); }
                else { out.push_str("  \u{2022} "); }
            }
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::Start(Tag::CodeBlock(_)) => { in_code = true; out.push('\n'); }
            Event::End(TagEnd::CodeBlock)   => { in_code = false; out.push('\n'); }
            Event::Start(Tag::BlockQuote(_))=> out.push_str("  \u{2502} "),
            Event::End(TagEnd::BlockQuote(_)) => out.push('\n'),
            Event::Rule => { out.push_str(&"\u{2500}".repeat(60)); out.push_str("\n\n"); }
            Event::Text(t) => {
                if in_image { img_alt.push_str(&t); }
                else {
                    if h_level > 0 { out.push_str(&"#".repeat(h_level as usize)); out.push(' '); }
                    if in_code { for l in t.lines() { out.push_str("    "); out.push_str(l); out.push('\n'); } }
                    else { out.push_str(&t); }
                }
            }
            Event::Code(c) => { out.push('`'); out.push_str(&c); out.push('`'); }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }

    // Replace any remaining $$ blocks with unicode approximations
    let final_text = sub_eq_unicode(&out);
    std::fs::write(output_path, final_text.trim_end()).map_err(|e| format!("Write: {}", e))
}

fn sub_eq_unicode(text: &str) -> String {
    let mut o = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(p) = rest.find("$$") {
            o.push_str(&rest[..p]);
            let after = &rest[p+2..];
            if let Some(e) = after.find("$$") {
                o.push_str(&crate::render::latex_to_unicode(after[..e].trim()));
                rest = &after[e+2..];
            } else { o.push_str(rest); break; }
        } else { o.push_str(rest); break; }
    }
    o
}

// ── Shared table rendering for the source-text exporters ─────────────────────
// Each text exporter accumulates a cell's inline content in its own buffer, then
// hands the assembled rows here. Row 0 is the header (GFM/pulldown convention).
// Cells arrive already escaped in the target format's idiom.

enum TableStyle { Latex, Typst, Rst, Org, Adoc, Rtf }

fn tcell(row: &[String], i: usize) -> &str {
    row.get(i).map(|s| s.trim()).unwrap_or("")
}

fn render_table(rows: &[Vec<String>], style: TableStyle) -> String {
    if rows.is_empty() { return String::new(); }
    let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let cell = tcell;
    let mut s = String::new();
    match style {
        TableStyle::Latex => {
            s.push_str("\n\\begin{table}[h]\\centering\n\\begin{tabular}{|");
            for _ in 0..ncol { s.push('l'); s.push('|'); }
            s.push_str("}\n\\hline\n");
            for r in rows {
                let cells: Vec<&str> = (0..ncol).map(|i| cell(r, i)).collect();
                s.push_str(&cells.join(" & "));
                s.push_str(" \\\\ \\hline\n");
            }
            s.push_str("\\end{tabular}\n\\end{table}\n\n");
        }
        TableStyle::Typst => {
            s.push_str(&format!("\n#table(\n  columns: {},\n", ncol));
            for r in rows {
                let cells: Vec<String> = (0..ncol).map(|i| format!("[{}]", cell(r, i))).collect();
                s.push_str("  "); s.push_str(&cells.join(", ")); s.push_str(",\n");
            }
            s.push_str(")\n\n");
        }
        TableStyle::Rst => {
            s.push_str("\n.. list-table::\n   :header-rows: 1\n\n");
            for r in rows {
                for i in 0..ncol {
                    let c = cell(r, i);
                    let c = if c.is_empty() { " " } else { c };
                    s.push_str(if i == 0 { "   * - " } else { "     - " });
                    s.push_str(c); s.push('\n');
                }
            }
            s.push('\n');
        }
        TableStyle::Org => {
            for (ri, r) in rows.iter().enumerate() {
                let cells: Vec<&str> = (0..ncol).map(|i| cell(r, i)).collect();
                s.push_str(&format!("| {} |\n", cells.join(" | ")));
                if ri == 0 {
                    s.push('|');
                    for _ in 0..ncol { s.push_str("---+"); }
                    s.pop(); s.push_str("|\n");
                }
            }
            s.push('\n');
        }
        TableStyle::Adoc => {
            s.push_str("\n[options=\"header\"]\n|===\n");
            for r in rows {
                for i in 0..ncol { s.push_str(&format!("| {} ", cell(r, i))); }
                s.push('\n');
            }
            s.push_str("|===\n\n");
        }
        TableStyle::Rtf => {
            // Cells arrive already RTF-escaped. Equal-width columns across ~9000 twips.
            let colw = (9000 / ncol as u32).max(800);
            for (ri, r) in rows.iter().enumerate() {
                s.push_str("\\trowd\\trgaph108");
                for c in 1..=ncol { s.push_str(&format!("\\cellx{}", colw * c as u32)); }
                for i in 0..ncol {
                    let bold = if ri == 0 { "\\b " } else { "" };
                    let unbold = if ri == 0 { "\\b0" } else { "" };
                    s.push_str(&format!("\\intbl {}{}{}\\cell ", bold, cell(r, i), unbold));
                }
                s.push_str("\\row\n");
            }
            s.push_str("\\pard\n");
        }
    }
    s
}

// ═════════════════════════════════════════════════════════════════════════════
// TeX/LaTeX export - native \begin{equation} blocks
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_tex(markdown: &str, output_path: &Path, meta: &PdfMetadata) -> Result<(), String> {
    let mut doc = String::with_capacity(markdown.len() * 2);

    // Preamble
    doc.push_str("\\documentclass[12pt,a4paper]{article}\n");
    doc.push_str("\\usepackage[utf8]{inputenc}\n\\usepackage[T1]{fontenc}\n");
    doc.push_str("\\usepackage{amsmath,amssymb,amsfonts}\n");
    doc.push_str("\\usepackage{graphicx,xcolor}\n");
    doc.push_str("\\usepackage[colorlinks,linkcolor=blue,urlcolor=blue]{hyperref}\n");
    doc.push_str("\\usepackage{listings,geometry}\n");
    doc.push_str("\\geometry{a4paper,margin=2.5cm}\n");
    doc.push_str("\\setlength{\\parskip}{0.5em}\\setlength{\\parindent}{0pt}\n");
    doc.push_str("\\lstset{basicstyle=\\ttfamily\\small,breaklines=true,frame=single,backgroundcolor=\\color{gray!10}}\n\n");

    if !meta.title.is_empty()  { doc.push_str(&format!("\\title{{{}}}\n", esc_tex(&meta.title))); }
    if !meta.author.is_empty() { doc.push_str(&format!("\\author{{{}}}\n", esc_tex(&meta.author))); }
    let date_line = if meta.timestamp.is_empty() {
        "\\date{}\n".to_string()
    } else {
        format!("\\date{{{}}}\n", esc_tex(&meta.timestamp))
    };
    doc.push_str(&date_line);

    doc.push_str("\n\\begin{document}\n");
    if !meta.title.is_empty() || !meta.author.is_empty() { doc.push_str("\\maketitle\n"); }
    doc.push('\n');

    // Display equations ($$...$$) become their own block; inline $...$ stays inline
    // inside md_fragment (split_blocks would wrongly hoist it to a display block,
    // fragmenting the sentence and shredding any table cell that contains math).
    for block in split_display_blocks(markdown) {
        match block {
            Block::Text(t) => doc.push_str(&md_fragment_to_tex(t)),
            Block::Equation(latex) => {
                doc.push_str("\\begin{equation}\n");
                doc.push_str(latex);
                doc.push_str("\n\\end{equation}\n\n");
            }
        }
    }

    doc.push_str("\\end{document}\n");
    std::fs::write(output_path, &doc).map_err(|e| format!("Write: {}", e))
}

fn md_fragment_to_tex(md: &str) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH;
    let mut out = String::new();
    let mut buf = String::new();
    let mut h_level: u32 = 0;
    let mut in_code = false;
    let mut is_ordered = false;
    let mut in_image = false; let mut img_url = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush { () => {
        let t = std::mem::take(&mut buf);
        if !t.trim().is_empty() { out.push_str(t.trim()); out.push('\n'); }
    }}

    for event in Parser::new_ext(md, opts) {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => { in_image = true; img_url = dest_url.to_string(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                // Reference the figure (graphicx is in the preamble); the user
                // keeps the image file alongside the .tex, as Pandoc does.
                buf.push_str(&format!("\\includegraphics[width=0.8\\linewidth]{{{}}}", img_url));
                img_url.clear();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                h_level = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, HeadingLevel::H4=>4, _=>5 };
                let cmd = ["","section","subsection","subsubsection","paragraph","subparagraph"][h_level as usize];
                out.push_str(&format!("\n\\{}{{\n", cmd));
            }
            Event::End(TagEnd::Heading(_)) => { flush!(); out.push_str("}\n\n"); h_level = 0; }
            Event::End(TagEnd::Paragraph) => { let t = std::mem::take(&mut buf); if !t.trim().is_empty() { out.push_str(t.trim()); out.push_str("\n\n"); } }
            Event::Start(Tag::Strong)     => buf.push_str("\\textbf{"),
            Event::End(TagEnd::Strong)    => buf.push('}'),
            Event::Start(Tag::Emphasis)   => buf.push_str("\\textit{"),
            Event::End(TagEnd::Emphasis)  => buf.push('}'),
            Event::Start(Tag::Strikethrough)  => buf.push_str("\\sout{"),
            Event::End(TagEnd::Strikethrough) => buf.push('}'),
            Event::Start(Tag::List(s)) => { flush!(); is_ordered = s.is_some(); out.push_str(if is_ordered { "\\begin{enumerate}\n" } else { "\\begin{itemize}\n" }); }
            Event::End(TagEnd::List(_)) => out.push_str(if is_ordered { "\\end{enumerate}\n\n" } else { "\\end{itemize}\n\n" }),
            Event::Start(Tag::Item) => out.push_str("  \\item "),
            Event::End(TagEnd::Item) => { let t = std::mem::take(&mut buf); out.push_str(t.trim()); out.push('\n'); }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush!(); in_code = true;
                let lang = match &kind { CodeBlockKind::Fenced(l) if !l.is_empty() => format!("[language={}]",l), _ => String::new() };
                out.push_str(&format!("\\begin{{lstlisting}}{}\n", lang));
            }
            Event::End(TagEnd::CodeBlock) => { out.push_str(&buf); buf.clear(); out.push_str("\\end{lstlisting}\n\n"); in_code = false; }
            Event::Start(Tag::BlockQuote(_)) => { flush!(); out.push_str("\\begin{quote}\n\\itshape "); }
            Event::End(TagEnd::BlockQuote(_)) => { let t = std::mem::take(&mut buf); out.push_str(t.trim()); out.push_str("\n\\end{quote}\n\n"); }
            Event::Start(Tag::Link { dest_url, .. }) => buf.push_str(&format!("\\href{{{}}}{{\n", esc_tex(&dest_url))),
            Event::End(TagEnd::Link) => buf.push('}'),
            Event::Rule => out.push_str("\n\\noindent\\rule{\\textwidth}{0.4pt}\n\n"),
            Event::Start(Tag::Table(_)) => { flush!(); trows.clear(); trow.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut buf)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => out.push_str(&render_table(&trows, TableStyle::Latex)),
            Event::InlineMath(m) => buf.push_str(&format!("${}$", m)),
            Event::DisplayMath(m) => { flush!(); out.push_str(&format!("\\[{}\\]\n\n", m)); }
            Event::Text(t) => { if in_image { /* alt caption - image speaks for itself */ } else if in_code { buf.push_str(&t); } else { buf.push_str(&esc_tex(&t)); } }
            Event::Code(c) => buf.push_str(&format!("\\texttt{{{}}}", esc_tex(&c))),
            Event::SoftBreak => buf.push(' '),
            Event::HardBreak => buf.push_str("\\\\\n"),
            _ => {}
        }
    }
    flush!();
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// Typst source export (.typ) - native $ math $
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_typst_src(markdown: &str, output_path: &Path, meta: &PdfMetadata) -> Result<(), String> {
    let mut doc = String::new();
    doc.push_str("#set page(paper: \"a4\", margin: (x: 2.5cm, y: 2.5cm))\n");
    doc.push_str("#set text(font: \"New Computer Modern\", size: 11pt, lang: \"");
    doc.push_str(if meta.lang.is_empty() { "en" } else { &meta.lang });
    doc.push_str("\")\n#set heading(numbering: \"1.\")\n#set par(justify: true)\n\n");

    if !meta.title.is_empty() {
        doc.push_str(&format!("#align(center)[#text(size: 18pt, weight: \"bold\")[{}]]\n", &meta.title));
        if !meta.author.is_empty() {
            doc.push_str(&format!("#align(center)[#text(size: 12pt, style: \"italic\")[{}]]\n", &meta.author));
        }
        doc.push_str("#v(1em)\n\n");
    }

    // Display math becomes its own block; inline $...$ stays inline in md_fragment.
    for block in split_display_blocks(markdown) {
        match block {
            Block::Text(t)       => doc.push_str(&md_fragment_to_typst(t)),
            Block::Equation(lat) => {
                let typst_math = crate::export_typst::latex_to_typst_math(lat);
                doc.push_str(&format!("$ {} $\n\n", typst_math));
            }
        }
    }

    std::fs::write(output_path, &doc).map_err(|e| format!("Write: {}", e))
}

fn md_fragment_to_typst(md: &str) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_MATH;
    let mut out = String::new();
    let mut buf = String::new();
    let mut h_level: u32 = 0;
    let mut in_code = false;
    let mut is_ordered = false;
    let mut list_depth: u32 = 0;
    let mut in_image = false; let mut img_url = String::new(); let mut img_alt = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush { () => {
        let t = std::mem::take(&mut buf);
        if !t.is_empty() { out.push_str(&t); }
    }}

    for event in Parser::new_ext(md, opts) {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => { in_image = true; img_url = dest_url.to_string(); img_alt.clear(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                if img_alt.is_empty() {
                    buf.push_str(&format!("#image(\"{}\", width: 80%)", img_url));
                } else {
                    buf.push_str(&format!("#figure(image(\"{}\", width: 80%), caption: [{}])", img_url, esc_typst(&img_alt)));
                }
                img_url.clear(); img_alt.clear();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                h_level = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, _=>4 };
                out.push_str(&"=".repeat(h_level as usize)); out.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => { flush!(); out.push_str("\n\n"); h_level = 0; }
            Event::End(TagEnd::Paragraph)  => { flush!(); out.push_str("\n\n"); }
            Event::Start(Tag::Strong)   => buf.push_str("*"),
            Event::End(TagEnd::Strong)  => buf.push('*'),
            Event::Start(Tag::Emphasis) => buf.push('_'),
            Event::End(TagEnd::Emphasis)=> buf.push('_'),
            Event::Start(Tag::Strikethrough)  => buf.push_str("#strike["),
            Event::End(TagEnd::Strikethrough) => buf.push(']'),
            Event::Start(Tag::List(s)) => { flush!(); list_depth += 1; is_ordered = s.is_some(); }
            Event::End(TagEnd::List(_))=> { list_depth = list_depth.saturating_sub(1); out.push('\n'); }
            Event::Start(Tag::Item) => {
                let ind = "  ".repeat(list_depth.saturating_sub(1) as usize);
                out.push_str(&format!("{}{} ", ind, if is_ordered { "+" } else { "-" }));
            }
            Event::End(TagEnd::Item) => { flush!(); out.push('\n'); }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush!(); in_code = true;
                let lang = match &kind { CodeBlockKind::Fenced(l) if !l.is_empty() => l.as_ref(), _ => "" };
                out.push_str(&format!("```{}\n", lang));
            }
            Event::End(TagEnd::CodeBlock) => { flush!(); out.push_str("```\n\n"); in_code = false; }
            Event::Start(Tag::BlockQuote(_)) => { flush!(); out.push_str("#quote[\n"); }
            Event::End(TagEnd::BlockQuote(_)) => { flush!(); out.push_str("]\n\n"); }
            Event::Start(Tag::Link { dest_url, .. }) => buf.push_str(&format!("#link(\"{}\")[", dest_url)),
            Event::End(TagEnd::Link) => buf.push(']'),
            Event::Rule => { flush!(); out.push_str("#line(length: 100%)\n\n"); }
            Event::Start(Tag::Table(_)) => { flush!(); trows.clear(); trow.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut buf)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => out.push_str(&render_table(&trows, TableStyle::Typst)),
            Event::InlineMath(m)  => buf.push_str(&format!("${}$", crate::export_typst::latex_to_typst_math(&m))),
            Event::DisplayMath(m) => { flush!(); out.push_str(&format!("$ {} $\n\n", crate::export_typst::latex_to_typst_math(&m))); }
            Event::Text(t) => { if in_image { img_alt.push_str(&t); } else if in_code { buf.push_str(&t); } else { buf.push_str(&esc_typst(&t)); } }
            Event::Code(c) => buf.push_str(&format!("`{}`", c)),
            Event::SoftBreak => buf.push(' '),
            Event::HardBreak => { flush!(); out.push_str("\\\n"); }
            _ => {}
        }
    }
    flush!();
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// RTF export - manual RTF generation, equations as unicode
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_rtf(markdown: &str, output_path: &Path, meta: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    let figures = crate::figure_embed::collect_figures(markdown, source_dir);
    let mut rtf = String::new();
    rtf.push_str("{\\rtf1\\ansi\\ansicpg1252\\cocoartf2639\n");
    rtf.push_str("{\\fonttbl\\f0\\froman\\fcharset0 Times New Roman;\\f1\\fmodern\\fcharset0 Courier New;}\n");
    rtf.push_str("{\\colortbl;\\red0\\green0\\blue0;\\red70\\green130\\blue200;\\red85\\green85\\blue85;}\n");
    rtf.push_str("\\widowctrl\\hyphauto\\widctlpar\\f0\\fs24\\cf1\n");

    if !meta.title.is_empty() {
        rtf.push_str(&format!("\\pard\\qc\\sb240\\b\\fs36 {}\\b0\\fs24\\par\n", esc_rtf(&meta.title)));
        if !meta.author.is_empty() {
            rtf.push_str(&format!("\\pard\\qc\\fs22\\cf3 {}\\cf1\\fs24\\par\n", esc_rtf(&meta.author)));
        }
        rtf.push_str("\\pard\\sb200\\par\n");
    }

    // Display math becomes a centered paragraph; inline $...$ stays inline.
    for block in split_display_blocks(markdown) {
        match block {
            Block::Text(t)       => rtf.push_str(&md_fragment_to_rtf(t, &figures)),
            Block::Equation(lat) => {
                let uni = crate::render::latex_to_unicode(lat);
                rtf.push_str(&format!("\\pard\\qc\\sb100\\sa100\\i {}\\i0\\par\n", esc_rtf(&uni)));
            }
        }
    }

    rtf.push('}');
    std::fs::write(output_path, rtf.as_bytes()).map_err(|e| format!("Write: {}", e))
}

/// One author figure as an embedded RTF picture (`\pict\pngblip`, hex-encoded).
/// Self-contained so the RTF travels with its figures, like the DOCX media path.
fn rtf_pict(png: &[u8], w: u32, h: u32) -> String {
    // px → twips at 96 DPI (1px = 15 twips), capped at ~6.25in (9000 twips) wide.
    let (dw, dh) = crate::figure_embed::fit_width(w, h, 600);
    let mut s = format!(
        "{{\\pict\\pngblip\\picwgoal{}\\pichgoal{}\n",
        dw as u64 * 15,
        dh as u64 * 15
    );
    s.reserve(png.len() * 2 + 8);
    for b in png {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0xF) as u32, 16).unwrap());
    }
    s.push_str("}\n");
    s
}

fn md_fragment_to_rtf(md: &str, figures: &[DocxFigure]) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH;
    let mut out = String::new();
    let mut buf = String::new();
    let mut h_level: u32 = 0;
    let mut in_code = false;
    let mut is_ordered = false;
    let mut ord_n = 0u64;
    let mut bold = false;
    let mut italic = false;
    let mut in_image = false; let mut img_src = String::new(); let mut img_alt = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush { () => {
        let t = std::mem::take(&mut buf);
        if !t.is_empty() { out.push_str(&t); }
    }}

    for event in Parser::new_ext(md, opts) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                h_level = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, _=>3 };
                let (sz, sb) = match h_level { 1=>(40u32,280u32), 2=>(32,220), _=>(28,180) };
                out.push_str(&format!("\\pard\\sb{}\\b\\fs{} ", sb, sz));
            }
            Event::End(TagEnd::Heading(_)) => { flush!(); out.push_str("\\b0\\fs24\\par\n"); h_level=0; }
            Event::End(TagEnd::Paragraph)  => { flush!(); out.push_str("\\par\n"); }
            Event::Start(Tag::Strong)   => { flush!(); bold=true;   out.push_str("\\b "); }
            Event::End(TagEnd::Strong)  => { flush!(); bold=false;  out.push_str("\\b0 "); }
            Event::Start(Tag::Emphasis) => { flush!(); italic=true;  out.push_str("\\i "); }
            Event::End(TagEnd::Emphasis)=> { flush!(); italic=false; out.push_str("\\i0 "); }
            Event::Start(Tag::Strikethrough)  => { flush!(); out.push_str("\\strike "); }
            Event::End(TagEnd::Strikethrough) => { flush!(); out.push_str("\\strike0 "); }
            Event::Start(Tag::List(s)) => { flush!(); is_ordered=s.is_some(); ord_n=s.unwrap_or(1).saturating_sub(1); }
            Event::End(TagEnd::List(_))=> { out.push_str("\\par\n"); }
            Event::Start(Tag::Item) => {
                if is_ordered { ord_n+=1; out.push_str(&format!("\\pard\\li720\\fi-360 {}.\\ ",ord_n)); }
                else { out.push_str("\\pard\\li720\\fi-360 \\bullet\\ "); }
            }
            Event::End(TagEnd::Item) => { flush!(); out.push_str("\\par\n"); }
            Event::Start(Tag::CodeBlock(_)) => { flush!(); in_code=true; out.push_str("\\pard\\f1\\fs20\\cf3 "); }
            Event::End(TagEnd::CodeBlock)   => { flush!(); out.push_str("\\f0\\fs24\\cf1\\par\n"); in_code=false; }
            Event::Start(Tag::BlockQuote(_)) => { flush!(); out.push_str("\\pard\\li720\\i\\cf3 "); }
            Event::End(TagEnd::BlockQuote(_)) => { flush!(); out.push_str("\\i0\\cf1\\par\n"); }
            Event::Rule => { flush!(); out.push_str("\\pard\\brdrb\\brdrs\\brdrw10\\brsp20 \\par\n"); }
            Event::Start(Tag::Image { dest_url, .. }) => { flush!(); in_image = true; img_src = dest_url.to_string(); img_alt.clear(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                if let Some(i) = figures.iter().position(|f| f.src == img_src) {
                    let fig = &figures[i];
                    out.push_str("\\pard\\qc ");
                    out.push_str(&rtf_pict(&fig.png, fig.w, fig.h));
                    out.push_str("\\par\n");
                } else if !img_alt.is_empty() {
                    out.push_str(&esc_rtf(&img_alt));
                }
                img_src.clear(); img_alt.clear();
            }
            Event::Start(Tag::Table(_)) => { flush!(); trows.clear(); trow.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut buf)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => out.push_str(&render_table(&trows, TableStyle::Rtf)),
            Event::InlineMath(m) => buf.push_str(&esc_rtf(&crate::render::latex_to_unicode(&m))),
            Event::DisplayMath(m) => { flush!(); out.push_str(&format!("\\pard\\qc\\i {}\\i0\\par\n", esc_rtf(&crate::render::latex_to_unicode(&m)))); }
            Event::Text(t) => { if in_image { img_alt.push_str(&t); } else { buf.push_str(&esc_rtf(&t)); } }
            Event::Code(c) => { flush!(); out.push_str(&format!("\\f1\\fs20 {}\\f0\\fs24 ", esc_rtf(&c))); }
            Event::SoftBreak => buf.push(' '),
            Event::HardBreak => { flush!(); out.push_str("\\line "); }
            _ => {}
        }
    }
    flush!();
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// DOCX export - Office Open XML inside a ZIP, equations embedded as PNG
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_docx(markdown: &str, output_path: &Path, meta: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    use std::io::Write as _;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    // Render PNG (rasterized 2×) + SVG (vector) for each unique equation
    let eq_images = render_eq_images(markdown);
    // Embed author figures (![](path)) so they travel inside the DOCX instead
    // of dangling as external references that show as broken images in Word.
    let figures = crate::figure_embed::collect_figures(markdown, source_dir);
    let (body_xml, comments) = docx_body(markdown, meta, &eq_images, &figures);
    let has_comments = !comments.is_empty();
    let rels_xml    = docx_doc_rels(&eq_images, &figures, has_comments);
    let ct_xml      = docx_content_types(&eq_images, &figures, has_comments);
    let comments_xml = if has_comments { docx_comments_xml(&comments) } else { String::new() };

    let file = std::fs::File::create(output_path).map_err(|e| format!("Create: {}", e))?;
    let mut z  = ZipWriter::new(file);
    let def = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let raw = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    macro_rules! zf {
        ($name:expr, $opt:expr, $data:expr) => {{
            z.start_file($name, $opt).map_err(|e| e.to_string())?;
            z.write_all($data).map_err(|e| e.to_string())?;
        }};
    }

    zf!("[Content_Types].xml", def, ct_xml.as_bytes());
    z.add_directory("_rels/", def).map_err(|e| e.to_string())?;
    zf!("_rels/.rels", def, DOCX_TOP_RELS.as_bytes());
    z.add_directory("word/", def).map_err(|e| e.to_string())?;
    zf!("word/document.xml", def, body_xml.as_bytes());
    if has_comments {
        zf!("word/comments.xml", def, comments_xml.as_bytes());
    }
    z.add_directory("word/_rels/", def).map_err(|e| e.to_string())?;
    zf!("word/_rels/document.xml.rels", def, rels_xml.as_bytes());
    zf!("word/styles.xml",   def, DOCX_STYLES.as_bytes());
    zf!("word/settings.xml", def, DOCX_SETTINGS.as_bytes());

    let has_media = !figures.is_empty()
        || eq_images.iter().any(|(_, png, svg, ..)| !png.is_empty() || !svg.is_empty());
    if has_media {
        z.add_directory("word/media/", def).map_err(|e| e.to_string())?;
        for (i, (_, png, svg, ..)) in eq_images.iter().enumerate() {
            if !png.is_empty() {
                zf!(&format!("word/media/eq_{}.png", i), raw, png.as_slice());
            }
            if !svg.is_empty() {
                zf!(&format!("word/media/eq_{}.svg", i), raw, svg.as_slice());
            }
        }
        for (i, fig) in figures.iter().enumerate() {
            zf!(&format!("word/media/fig_{}.png", i), raw, fig.png.as_slice());
        }
    }

    // ── Embed full markdown source for lossless re-import ────────────────
    // Word ignores unknown ZIP entries; our importer looks for this file first.
    let source_xml = crate::source_embed::build_source_xml(markdown);
    zf!(crate::source_embed::DOCX_SOURCE_ENTRY, def, source_xml.as_bytes());

    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

// (latex, png_bytes, svg_bytes, width_px, height_px) - PNG at 2× scale; SVG vector for DOCX
// Both PNG and SVG carry the LaTeX source in their metadata (tEXt chunk / <metadata> element)
// so the export is fully reversible even if the DOCX is stripped of comments.
fn render_eq_images(markdown: &str) -> Vec<(String, Vec<u8>, Vec<u8>, u32, u32)> {
    let mut images: Vec<(String, Vec<u8>, Vec<u8>, u32, u32)> = Vec::new();
    for block in split_blocks(markdown) {
        if let Block::Equation(lat) = block {
            if images.iter().any(|(l, ..)| l == lat) { continue; }
            let (png_opt, _) = crate::equation_renderer::render_equation_png(lat, 2.0);

            // Embed LaTeX source in SVG <metadata> for lossless round-trip.
            let svg_bytes = crate::equation_renderer::render_equation_svg(lat)
                .map(|svg| {
                    crate::source_embed::embed_latex_in_svg(&svg, lat).into_bytes()
                })
                .unwrap_or_default();

            if let Some(png) = png_opt {
                if let Ok(img) = image::load_from_memory(&png) {
                    // Embed LaTeX source in PNG tEXt chunk.
                    let png_with_source = crate::source_embed::embed_latex_in_png(&png, lat);
                    images.push((lat.to_string(), png_with_source, svg_bytes, img.width(), img.height()));
                    continue;
                }
            }
            images.push((lat.to_string(), Vec::new(), svg_bytes, 0, 0));
        }
    }
    images
}

/// Author figures (`![](path)`) are collected by the shared
/// `figure_embed::collect_figures` and embedded by each container exporter.
use crate::figure_embed::Figure as DocxFigure;

/// EMU extent for a figure at 1× (96 DPI: 1px = 9525 EMU), capped at the same
/// 15 cm width used for equations so wide figures stay within the page.
fn figure_extent(w: u32, h: u32) -> (u64, u64) {
    let dw = w.max(1) as u64;
    let dh = h.max(1) as u64;
    let max_emu: u64 = 5_400_000; // 15 cm
    let cx_full = dw * 9525;
    if cx_full > max_emu {
        (max_emu, max_emu * dh / dw)
    } else {
        (cx_full, dh * 9525)
    }
}

/// One author figure as an inline drawing run (PNG raster, no comment bubble).
fn docx_figure_run(rid: &str, fig_id: u32, cx: u64, cy: u64, name: &str) -> String {
    let did = fig_id + 5000; // keep clear of equation docPr ids (eq_id + 100)
    let safe = esc_xml(name);
    format!(
        "<w:r><w:drawing><wp:inline>\
        <wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
        <wp:docPr id=\"{did}\" name=\"{name}\"/>\
        <a:graphic><a:graphicData \
        uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
        <pic:pic>\
        <pic:nvPicPr><pic:cNvPr id=\"{did}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr>\
        <pic:blipFill><a:blip r:embed=\"{rid}\"/>\
        <a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
        <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
        <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
        </pic:pic></a:graphicData></a:graphic>\
        </wp:inline></w:drawing></w:r>",
        cx = cx, cy = cy, did = did, name = safe, rid = rid
    )
}

fn docx_body(
    markdown: &str,
    meta: &PdfMetadata,
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
    figures: &[DocxFigure],
) -> (String, Vec<(u32, String)>) {
    let mut body = String::new();
    let mut comments: Vec<(u32, String)> = Vec::new();
    let mut comment_id = 0u32;

    if !meta.title.is_empty() {
        body.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>",
            esc_xml(&meta.title)
        ));
    }
    if !meta.author.is_empty() {
        body.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"Subtitle\"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>",
            esc_xml(&meta.author)
        ));
    }

    for block in split_display_blocks(markdown) {
        match block {
            Block::Text(t) => body.push_str(&md_fragment_to_docx(t, eq_images, figures)),
            Block::Equation(lat) => {
                if let Some(i) = eq_images.iter().position(|(l, ..)| l == lat) {
                    let (_, png, svg, w, h) = &eq_images[i];
                    let cid = comment_id;
                    comment_id += 1;
                    comments.push((cid, lat.to_string()));

                    if !png.is_empty() && *w > 0 {
                        // 2× render → display at 1× → EMU at 96 DPI (1px = 9525 EMU)
                        let dw = (*w / 2) as u64;
                        let dh = (*h / 2) as u64;
                        let max_emu: u64 = 5_400_000; // 15 cm
                        let (cx, cy) = if dw * 9525 > max_emu {
                            let cx = max_emu;
                            let cy = max_emu * dh / dw.max(1);
                            (cx, cy)
                        } else {
                            (dw * 9525, dh * 9525)
                        };
                        let png_rid = format!("rIdPng{}", i);
                        let svg_rid = if !svg.is_empty() { Some(format!("rIdSvg{}", i)) } else { None };
                        body.push_str(&docx_img_para(
                            &png_rid,
                            svg_rid.as_deref(),
                            i as u32,
                            cx,
                            cy,
                            cid,
                        ));
                    } else {
                        // Fallback: unicode approximation with comment marker
                        let unicode = esc_xml(&crate::render::latex_to_unicode(lat));
                        body.push_str(&format!(
                            "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>\
                            <w:commentRangeStart w:id=\"{c}\"/>\
                            <w:r><w:rPr><w:i/></w:rPr><w:t>{u}</w:t></w:r>\
                            <w:commentRangeEnd w:id=\"{c}\"/>\
                            <w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr>\
                            <w:commentReference w:id=\"{c}\"/></w:r></w:p>",
                            c = cid, u = unicode
                        ));
                    }
                }
            }
        }
    }

    body.push_str("<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
        <w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr>");

    let body_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <w:document \
        xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
        xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
        xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
        xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
        xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
        <w:body>{}</w:body></w:document>",
        body
    );
    (body_xml, comments)
}

fn md_fragment_to_docx(
    md: &str,
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
    figures: &[DocxFigure],
) -> String {
    // Shield inline math from Markdown parsing and render it inline (not as a
    // separate paragraph, which used to fragment sentences in the DOCX output).
    let (md, inline_eqs) = placeholder_inline_math(md);
    let md: &str = &md;
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    let mut run = String::new();
    let mut bold = false; let mut italic = false; let mut code = false;
    let mut is_ordered = false; let mut ord_n = 0u64;
    let mut h_level: u32 = 0;
    let mut in_para = false;
    // While inside an image, text events are the alt caption, not body text.
    let mut in_image = false;
    let mut img_src = String::new();
    let mut img_alt = String::new();
    // Table accumulation: cell text (with inline-math placeholders) → rows.
    let mut in_table = false;
    let mut tcell = String::new();
    let mut trow: Vec<String> = Vec::new();
    let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush_run { () => {
        if !run.is_empty() {
            let text = std::mem::take(&mut run);
            let rpr = if bold || italic || code {
                format!("<w:rPr>{}{}{}</w:rPr>",
                    if bold  { "<w:b/>" } else { "" },
                    if italic{ "<w:i/>" } else { "" },
                    if code  { "<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\"/><w:sz w:val=\"18\"/>" } else { "" })
            } else { String::new() };
            push_run_with_inline_math(&mut out, &text, &rpr, &inline_eqs, eq_images);
        }
    }}

    for event in Parser::new_ext(md, opts) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_run!();
                h_level = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, _=>4 };
                out.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Heading{}\"/></w:pPr>", h_level));
            }
            Event::End(TagEnd::Heading(_)) => { flush_run!(); out.push_str("</w:p>"); h_level=0; }
            Event::Start(Tag::Paragraph) => { in_para=true; out.push_str("<w:p>"); }
            Event::End(TagEnd::Paragraph) => { flush_run!(); out.push_str("</w:p>"); in_para=false; }
            Event::Start(Tag::Strong)    => { flush_run!(); bold=true; }
            Event::End(TagEnd::Strong)   => { flush_run!(); bold=false; }
            Event::Start(Tag::Emphasis)  => { flush_run!(); italic=true; }
            Event::End(TagEnd::Emphasis) => { flush_run!(); italic=false; }
            Event::Start(Tag::List(s)) => { is_ordered=s.is_some(); ord_n=s.unwrap_or(1).saturating_sub(1); }
            Event::End(TagEnd::List(_)) => {}
            Event::Start(Tag::Item) => {
                ord_n += 1;
                let style = if is_ordered { "ListNumber" } else { "ListBullet" };
                out.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"{}\"/></w:pPr>", style));
            }
            Event::End(TagEnd::Item) => { flush_run!(); out.push_str("</w:p>"); }
            Event::Start(Tag::CodeBlock(_)) => { flush_run!(); code=true; out.push_str("<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/></w:pPr>"); }
            Event::End(TagEnd::CodeBlock)   => { flush_run!(); out.push_str("</w:p>"); code=false; }
            Event::Start(Tag::BlockQuote(_)) => { flush_run!(); out.push_str("<w:p><w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr>"); }
            Event::End(TagEnd::BlockQuote(_)) => { flush_run!(); out.push_str("</w:p>"); }
            Event::Rule => out.push_str("<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"888888\"/></w:pBdr></w:pPr></w:p>"),
            Event::Start(Tag::Image { dest_url, .. }) => {
                flush_run!();
                in_image = true;
                img_src = dest_url.to_string();
                img_alt.clear();
            }
            Event::End(TagEnd::Image) => {
                in_image = false;
                if let Some(i) = figures.iter().position(|f| f.src == img_src) {
                    let fig = &figures[i];
                    let (cx, cy) = figure_extent(fig.w, fig.h);
                    let alt = if img_alt.is_empty() { "Figure".to_string() } else { img_alt.clone() };
                    out.push_str(&docx_figure_run(&format!("rIdFig{}", i), i as u32, cx, cy, &alt));
                } else if !img_alt.is_empty() {
                    // Figure could not be embedded - keep its alt text so the
                    // reference is not silently dropped from the document.
                    let alt = std::mem::take(&mut img_alt);
                    push_run_with_inline_math(&mut out, &alt, "", &inline_eqs, eq_images);
                }
                img_src.clear();
                img_alt.clear();
            }
            Event::Start(Tag::Table(_)) => { flush_run!(); in_table=true; trows.clear(); trow.clear(); tcell.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut tcell)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => {
                in_table = false;
                let ncol = trows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                out.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders>");
                for edge in ["top","left","bottom","right","insideH","insideV"] {
                    out.push_str(&format!("<w:{} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"888888\"/>", edge));
                }
                out.push_str("</w:tblBorders></w:tblPr>");
                for (ri, r) in trows.iter().enumerate() {
                    out.push_str("<w:tr>");
                    for i in 0..ncol {
                        let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
                        out.push_str("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/></w:tcPr><w:p>");
                        let rpr = if ri == 0 { "<w:rPr><w:b/></w:rPr>" } else { "" };
                        push_run_with_inline_math(&mut out, cell, rpr, &inline_eqs, eq_images);
                        out.push_str("</w:p></w:tc>");
                    }
                    out.push_str("</w:tr>");
                }
                // A paragraph must follow a table in WordprocessingML.
                out.push_str("</w:tbl><w:p/>");
            }
            Event::Text(t) => { if in_image { img_alt.push_str(&t); } else if in_table { tcell.push_str(&t); } else { run.push_str(&t); } }
            Event::Code(c) => { flush_run!(); code=true; run.push_str(&c); flush_run!(); code=false; }
            Event::SoftBreak => { if in_table { tcell.push(' '); } else if !in_image { run.push(' '); } }
            Event::HardBreak => { flush_run!(); out.push_str("<w:r><w:br/></w:r>"); }
            _ => {}
        }
    }
    out
}

// One equation image as an in-line drawing run (no paragraph, no comment).
// Shared by the centered display paragraph and by inline-in-text equations.
//   - SVG vector primary (Word 2016+) + PNG raster fallback (Word 2013/older)
fn docx_img_run(png_rid: &str, svg_rid: Option<&str>, eq_id: u32, cx: u64, cy: u64) -> String {
    let did = eq_id + 100;

    // BlipFill: PNG always present; SVG extension if available
    let blip_fill = if let Some(srid) = svg_rid {
        format!(
            "<pic:blipFill>\
            <a:blip r:embed=\"{prid}\">\
            <a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\">\
            <asvg:svgBlip \
            xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" \
            r:embed=\"{srid}\"/>\
            </a:ext></a:extLst>\
            </a:blip>\
            <a:stretch><a:fillRect/></a:stretch>\
            </pic:blipFill>",
            prid = png_rid, srid = srid
        )
    } else {
        format!(
            "<pic:blipFill><a:blip r:embed=\"{prid}\"/>\
            <a:stretch><a:fillRect/></a:stretch></pic:blipFill>",
            prid = png_rid
        )
    };

    format!(
        "<w:r><w:drawing><wp:inline>\
        <wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
        <wp:docPr id=\"{did}\" name=\"Eq{eid}\"/>\
        <a:graphic><a:graphicData \
        uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
        <pic:pic>\
        <pic:nvPicPr>\
        <pic:cNvPr id=\"{did}\" name=\"Eq{eid}\"/>\
        <pic:cNvPicPr/>\
        </pic:nvPicPr>\
        {bf}\
        <pic:spPr><a:xfrm>\
        <a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/>\
        </a:xfrm>\
        <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>\
        </pic:spPr>\
        </pic:pic></a:graphicData></a:graphic>\
        </wp:inline></w:drawing></w:r>",
        cx = cx, cy = cy, did = did, eid = eq_id, bf = blip_fill
    )
}

// Centered equation paragraph with a Word comment bubble carrying the LaTeX source.
fn docx_img_para(
    png_rid: &str,
    svg_rid: Option<&str>,
    eq_id: u32,
    cx: u64,
    cy: u64,
    comment_id: u32,
) -> String {
    format!(
        "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>\
        <w:commentRangeStart w:id=\"{cid}\"/>\
        {run}\
        <w:commentRangeEnd w:id=\"{cid}\"/>\
        <w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr>\
        <w:commentReference w:id=\"{cid}\"/></w:r>\
        </w:p>",
        cid = comment_id, run = docx_img_run(png_rid, svg_rid, eq_id, cx, cy)
    )
}

/// Render one inline equation (looked up by its LaTeX) as an in-line DOCX run.
/// Reversibility is preserved by the PNG tEXt / SVG metadata / source.xml layers,
/// so inline equations carry no Word comment - keeps the prose clean.
fn docx_inline_eq_run(
    latex: &str,
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
) -> String {
    if let Some(i) = eq_images.iter().position(|(l, ..)| l == latex) {
        let (_, png, svg, w, h) = &eq_images[i];
        if !png.is_empty() && *w > 0 {
            let dw = (*w / 2) as u64;
            let dh = (*h / 2) as u64;
            let max_emu: u64 = 5_400_000; // 15 cm
            let (cx, cy) = if dw * 9525 > max_emu {
                (max_emu, max_emu * dh / dw.max(1))
            } else {
                (dw * 9525, dh * 9525)
            };
            let png_rid = format!("rIdPng{}", i);
            let svg_rid = if !svg.is_empty() { Some(format!("rIdSvg{}", i)) } else { None };
            return docx_img_run(&png_rid, svg_rid.as_deref(), i as u32, cx, cy);
        }
    }
    // Fallback: italic unicode approximation.
    let unicode = esc_xml(&crate::render::latex_to_unicode(latex));
    format!("<w:r><w:rPr><w:i/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>", unicode)
}

// Private-use delimiters wrapping an inline-equation index inside text runs.
// They survive pulldown-cmark untouched (no Markdown meaning) so inline math is
// shielded from emphasis/escape parsing, then expanded back to drawings on flush.
const EQ_PH_OPEN: char = '\u{E000}';
const EQ_PH_CLOSE: char = '\u{E001}';

/// Replace inline `$...$` and `\(...\)` math with placeholder tokens, returning
/// the rewritten text and the ordered list of extracted LaTeX strings. Display
/// `$$...$$` is copied verbatim (handled at block level, not here).
fn placeholder_inline_math(md: &str) -> (String, Vec<String>) {
    let b = md.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n);
    let mut eqs: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut seg_start = 0usize;
    while i < n {
        // Display $$...$$ - leave verbatim in the text run.
        if b[i] == b'$' && i + 1 < n && b[i + 1] == b'$' {
            let mut j = i + 2;
            while j + 1 < n && !(b[j] == b'$' && b[j + 1] == b'$') { j += 1; }
            i = if j + 1 < n { j + 2 } else { n };
            continue;
        }
        // Inline $...$
        if b[i] == b'$' {
            let mut j = i + 1;
            while j < n && b[j] != b'$' { j += 1; }
            if j < n {
                out.push_str(&md[seg_start..i]);
                eqs.push(md[i + 1..j].trim().to_string());
                out.push(EQ_PH_OPEN);
                out.push_str(&(eqs.len() - 1).to_string());
                out.push(EQ_PH_CLOSE);
                i = j + 1;
                seg_start = i;
                continue;
            }
            i += 1;
            continue;
        }
        // Inline \(...\)
        if b[i] == b'\\' && i + 1 < n && b[i + 1] == b'(' {
            if let Some(rel) = md[i + 2..].find("\\)") {
                out.push_str(&md[seg_start..i]);
                eqs.push(md[i + 2..i + 2 + rel].trim().to_string());
                out.push(EQ_PH_OPEN);
                out.push_str(&(eqs.len() - 1).to_string());
                out.push(EQ_PH_CLOSE);
                i = i + 2 + rel + 2;
                seg_start = i;
                continue;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out.push_str(&md[seg_start..]);
    (out, eqs)
}

/// Emit a text run, expanding any inline-equation placeholders into drawing runs.
fn push_run_with_inline_math(
    out: &mut String,
    text: &str,
    rpr: &str,
    inline_eqs: &[String],
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
) {
    if !text.contains(EQ_PH_OPEN) {
        out.push_str(&format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", rpr, esc_xml(text)));
        return;
    }
    let mut rest = text;
    while let Some(start) = rest.find(EQ_PH_OPEN) {
        let before = &rest[..start];
        if !before.is_empty() {
            out.push_str(&format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", rpr, esc_xml(before)));
        }
        let after_open = &rest[start + EQ_PH_OPEN.len_utf8()..];
        if let Some(end) = after_open.find(EQ_PH_CLOSE) {
            if let Ok(idx) = after_open[..end].parse::<usize>() {
                if let Some(latex) = inline_eqs.get(idx) {
                    out.push_str(&docx_inline_eq_run(latex, eq_images));
                }
            }
            rest = &after_open[end + EQ_PH_CLOSE.len_utf8()..];
        } else {
            out.push_str(&format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", rpr, esc_xml(after_open)));
            return;
        }
    }
    if !rest.is_empty() {
        out.push_str(&format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", rpr, esc_xml(rest)));
    }
}

/// Generate word/comments.xml - each entry holds the LaTeX source for one equation occurrence.
fn docx_comments_xml(comments: &[(u32, String)]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <w:comments xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">"
    );
    for (id, latex) in comments {
        s.push_str(&format!(
            "<w:comment w:id=\"{id}\" w:author=\"MD-TO-ALL\" \
            w:date=\"2024-01-01T00:00:00Z\" w:initials=\"M\">\
            <w:p><w:r>\
            <w:t xml:space=\"preserve\">LaTeX: {latex}</w:t>\
            </w:r></w:p></w:comment>",
            id = id,
            latex = esc_xml(latex)
        ));
    }
    s.push_str("</w:comments>");
    s
}

fn docx_doc_rels(
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
    figures: &[DocxFigure],
    has_comments: bool,
) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" \
        Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" \
        Target=\"styles.xml\"/>"
    );
    if has_comments {
        s.push_str(
            "<Relationship Id=\"rIdComments\" \
            Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments\" \
            Target=\"comments.xml\"/>"
        );
    }
    for (i, (_, png, svg, ..)) in eq_images.iter().enumerate() {
        if !png.is_empty() {
            s.push_str(&format!(
                "<Relationship Id=\"rIdPng{i}\" \
                Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" \
                Target=\"media/eq_{i}.png\"/>",
                i = i
            ));
        }
        if !svg.is_empty() {
            s.push_str(&format!(
                "<Relationship Id=\"rIdSvg{i}\" \
                Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" \
                Target=\"media/eq_{i}.svg\"/>",
                i = i
            ));
        }
    }
    for (i, _) in figures.iter().enumerate() {
        s.push_str(&format!(
            "<Relationship Id=\"rIdFig{i}\" \
            Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" \
            Target=\"media/fig_{i}.png\"/>",
            i = i
        ));
    }
    s.push_str("</Relationships>");
    s
}

fn docx_content_types(
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
    figures: &[DocxFigure],
    has_comments: bool,
) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
        <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
        <Default Extension=\"xml\" ContentType=\"application/xml\"/>"
    );
    if !figures.is_empty() || eq_images.iter().any(|(_, p, ..)| !p.is_empty()) {
        s.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/>");
    }
    if eq_images.iter().any(|(_, _, sv, ..)| !sv.is_empty()) {
        s.push_str("<Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>");
    }
    s.push_str("<Override PartName=\"/word/document.xml\" \
        ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>");
    s.push_str("<Override PartName=\"/word/styles.xml\" \
        ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>");
    if has_comments {
        s.push_str("<Override PartName=\"/word/comments.xml\" \
            ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/>");
    }
    s.push_str("</Types>");
    s
}

const DOCX_TOP_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
    <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
    <Relationship Id=\"rId1\" \
    Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" \
    Target=\"word/document.xml\"/></Relationships>";

const DOCX_SETTINGS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
    <w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
    <w:defaultTabStop w:val=\"720\"/></w:settings>";

const DOCX_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:default="1" w:styleId="Normal">
  <w:name w:val="Normal"/>
  <w:rPr><w:sz w:val="24"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Title">
  <w:name w:val="Title"/>
  <w:pPr><w:jc w:val="center"/><w:spacing w:after="240"/></w:pPr>
  <w:rPr><w:b/><w:sz w:val="52"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Subtitle">
  <w:name w:val="Subtitle"/>
  <w:pPr><w:jc w:val="center"/><w:spacing w:after="120"/></w:pPr>
  <w:rPr><w:i/><w:sz w:val="28"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading1">
  <w:name w:val="heading 1"/>
  <w:pPr><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr>
  <w:rPr><w:b/><w:sz w:val="40"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading2">
  <w:name w:val="heading 2"/>
  <w:pPr><w:spacing w:before="200" w:after="100"/><w:outlineLvl w:val="1"/></w:pPr>
  <w:rPr><w:b/><w:sz w:val="32"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading3">
  <w:name w:val="heading 3"/>
  <w:pPr><w:spacing w:before="160" w:after="80"/><w:outlineLvl w:val="2"/></w:pPr>
  <w:rPr><w:b/><w:i/><w:sz w:val="28"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading4">
  <w:name w:val="heading 4"/>
  <w:pPr><w:outlineLvl w:val="3"/></w:pPr>
  <w:rPr><w:b/><w:sz w:val="24"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="ListBullet">
  <w:name w:val="List Bullet"/>
  <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr>
</w:style>
<w:style w:type="paragraph" w:styleId="ListNumber">
  <w:name w:val="List Number"/>
  <w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr>
</w:style>
<w:style w:type="paragraph" w:styleId="CodeBlock">
  <w:name w:val="Code Block"/>
  <w:pPr><w:spacing w:before="60" w:after="60"/><w:shd w:val="clear" w:fill="F4F4F4"/></w:pPr>
  <w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/><w:sz w:val="18"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Quote">
  <w:name w:val="Quote"/>
  <w:pPr><w:ind w:left="720"/></w:pPr>
  <w:rPr><w:i/><w:color w:val="555555"/></w:rPr>
</w:style>
</w:styles>"#;

// ═════════════════════════════════════════════════════════════════════════════
// ODT export - ODF 1.3 inside ZIP, equations embedded as PNG
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_odt(markdown: &str, output_path: &Path, meta: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    use std::io::Write as _;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    let eq_images = render_eq_images(markdown);
    let figures   = crate::figure_embed::collect_figures(markdown, source_dir);
    let content   = odt_content(markdown, meta, &eq_images, &figures);
    let manifest  = odt_manifest(&eq_images, &figures);

    let file = std::fs::File::create(output_path).map_err(|e| format!("Create: {}", e))?;
    let mut z  = ZipWriter::new(file);
    let def = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let raw = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    macro_rules! zf {
        ($name:expr, $opt:expr, $data:expr) => {{
            z.start_file($name, $opt).map_err(|e| e.to_string())?;
            z.write_all($data).map_err(|e| e.to_string())?;
        }};
    }

    // mimetype MUST be first + MUST be stored (spec requirement)
    zf!("mimetype", raw, b"application/vnd.oasis.opendocument.text");
    z.add_directory("META-INF/", def).map_err(|e| e.to_string())?;
    zf!("META-INF/manifest.xml", def, manifest.as_bytes());
    zf!("content.xml", def, content.as_bytes());
    zf!("styles.xml",  def, ODT_STYLES.as_bytes());

    if !eq_images.is_empty() || !figures.is_empty() {
        z.add_directory("Pictures/", def).map_err(|e| e.to_string())?;
        for (i, (_, png, ..)) in eq_images.iter().enumerate() {
            if !png.is_empty() {
                zf!(&format!("Pictures/eq_{}.png", i), raw, png.as_slice());
            }
        }
        for (i, fig) in figures.iter().enumerate() {
            zf!(&format!("Pictures/fig_{}.png", i), raw, fig.png.as_slice());
        }
    }

    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn odt_content(markdown: &str, meta: &PdfMetadata, eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)], figures: &[DocxFigure]) -> String {
    let mut body = String::new();

    if !meta.title.is_empty() {
        body.push_str(&format!(
            "<text:h text:style-name=\"Heading_1\" text:outline-level=\"1\">{}</text:h>",
            esc_xml(&meta.title)
        ));
        if !meta.author.is_empty() {
            body.push_str(&format!(
                "<text:p text:style-name=\"Subtitle\">{}</text:p>",
                esc_xml(&meta.author)
            ));
        }
    }

    for block in split_display_blocks(markdown) {
        match block {
            Block::Text(t)       => body.push_str(&md_fragment_to_odt(t, figures, eq_images)),
            Block::Equation(lat) => {
                if let Some(i) = eq_images.iter().position(|(l, ..)| l == lat) {
                    let (_, png, _svg, w, h) = &eq_images[i];
                    if !png.is_empty() && *w > 0 {
                        // 2× render → display at 1× → cm at 96 DPI (1px = 0.02646 cm)
                        let mut wc = (*w as f32) / 2.0 * 0.02646_f32;
                        let mut hc = (*h as f32) / 2.0 * 0.02646_f32;
                        if wc > 15.0 { hc *= 15.0 / wc; wc = 15.0; }
                        body.push_str(&format!(
                            "<text:p text:style-name=\"Equation\">\
                            <draw:frame draw:style-name=\"Graphics\" draw:name=\"Eq{i}\" \
                            text:anchor-type=\"as-char\" \
                            svg:width=\"{w:.3}cm\" svg:height=\"{h:.3}cm\">\
                            <draw:image xlink:href=\"Pictures/eq_{i}.png\" \
                            xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>\
                            </draw:frame></text:p>",
                            i=i, w=wc, h=hc
                        ));
                    } else {
                        body.push_str(&format!(
                            "<text:p text:style-name=\"Equation\">{}</text:p>",
                            esc_xml(&crate::render::latex_to_unicode(lat))
                        ));
                    }
                }
            }
        }
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <office:document-content \
        xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
        xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
        xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
        xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
        xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
        xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
        office:version=\"1.3\">\
        <office:body><office:text>{}</office:text></office:body>\
        </office:document-content>",
        body
    )
}

/// One inline equation as an as-char ODT image frame (or unicode fallback).
fn odt_inline_eq_frame(latex: &str, eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)]) -> String {
    if let Some(i) = eq_images.iter().position(|(l, ..)| l == latex) {
        let (_, png, _svg, w, h) = &eq_images[i];
        if !png.is_empty() && *w > 0 {
            let wc = (*w as f32) / 2.0 * 0.02646_f32;
            let hc = (*h as f32) / 2.0 * 0.02646_f32;
            return format!(
                "<draw:frame draw:style-name=\"Graphics\" draw:name=\"IEq{i}\" \
                text:anchor-type=\"as-char\" svg:width=\"{w:.3}cm\" svg:height=\"{h:.3}cm\">\
                <draw:image xlink:href=\"Pictures/eq_{i}.png\" xlink:type=\"simple\" \
                xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
                i = i, w = wc, h = hc
            );
        }
    }
    esc_xml(&crate::render::latex_to_unicode(latex))
}

/// Emit text wrapped in an optional span style, expanding inline-math placeholders
/// into ODT image frames (frames cannot live inside a styled span run, so each
/// text segment gets its own span and frames are emitted bare).
fn odt_emit_inline(
    out: &mut String,
    text: &str,
    sopen: &str,
    sclose: &str,
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
    inline_eqs: &[String],
) {
    if !text.contains(EQ_PH_OPEN) {
        out.push_str(sopen); out.push_str(&esc_xml(text)); out.push_str(sclose);
        return;
    }
    let mut rest = text;
    while let Some(start) = rest.find(EQ_PH_OPEN) {
        let before = &rest[..start];
        if !before.is_empty() { out.push_str(sopen); out.push_str(&esc_xml(before)); out.push_str(sclose); }
        let after_open = &rest[start + EQ_PH_OPEN.len_utf8()..];
        if let Some(end) = after_open.find(EQ_PH_CLOSE) {
            if let Ok(idx) = after_open[..end].parse::<usize>() {
                if let Some(latex) = inline_eqs.get(idx) {
                    out.push_str(&odt_inline_eq_frame(latex, eq_images));
                }
            }
            rest = &after_open[end + EQ_PH_CLOSE.len_utf8()..];
        } else { break; }
    }
    if !rest.is_empty() { out.push_str(sopen); out.push_str(&esc_xml(rest)); out.push_str(sclose); }
}

fn md_fragment_to_odt(
    md: &str,
    figures: &[DocxFigure],
    eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)],
) -> String {
    let (md, inline_eqs) = placeholder_inline_math(md);
    let md: &str = &md;
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    let mut buf = String::new();
    let mut bold = false; let mut italic = false; let mut in_code = false;
    let mut is_ordered = false;
    let mut h_level: u32 = 0;
    let mut in_image = false; let mut img_src = String::new(); let mut img_alt = String::new();
    let mut in_table = false; let mut tcell = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush_span { () => {
        if !buf.is_empty() {
            let t = std::mem::take(&mut buf);
            let (sopen, sclose) = if bold || italic || in_code {
                let sty = if in_code { "CodeChar" } else if bold && italic { "BoldItalic" } else if bold { "Bold" } else { "Italic" };
                (format!("<text:span text:style-name=\"{}\">", sty), "</text:span>".to_string())
            } else { (String::new(), String::new()) };
            odt_emit_inline(&mut out, &t, &sopen, &sclose, eq_images, &inline_eqs);
        }
    }}

    for event in Parser::new_ext(md, opts) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_span!();
                h_level = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, _=>4 };
                out.push_str(&format!("<text:h text:style-name=\"Heading_{}\" text:outline-level=\"{}\">", h_level, h_level));
            }
            Event::End(TagEnd::Heading(_)) => { flush_span!(); out.push_str("</text:h>"); h_level=0; }
            Event::Start(Tag::Paragraph)   => out.push_str("<text:p text:style-name=\"Text_Body\">"),
            Event::End(TagEnd::Paragraph)  => { flush_span!(); out.push_str("</text:p>"); }
            Event::Start(Tag::Strong)   => { flush_span!(); bold=true; }
            Event::End(TagEnd::Strong)  => { flush_span!(); bold=false; }
            Event::Start(Tag::Emphasis) => { flush_span!(); italic=true; }
            Event::End(TagEnd::Emphasis)=> { flush_span!(); italic=false; }
            Event::Start(Tag::Strikethrough)  => { flush_span!(); out.push_str("<text:span text:style-name=\"Strike\">"); }
            Event::End(TagEnd::Strikethrough) => { flush_span!(); out.push_str("</text:span>"); }
            Event::Start(Tag::List(s)) => { is_ordered=s.is_some(); out.push_str(if is_ordered {"<text:list text:style-name=\"List_Number\">"} else {"<text:list text:style-name=\"List_Bullet\">"}); }
            Event::End(TagEnd::List(_))=> out.push_str("</text:list>"),
            Event::Start(Tag::Item)    => out.push_str("<text:list-item><text:p>"),
            Event::End(TagEnd::Item)   => { flush_span!(); out.push_str("</text:p></text:list-item>"); }
            Event::Start(Tag::CodeBlock(_)) => { flush_span!(); in_code=true; out.push_str("<text:p text:style-name=\"Code\">"); }
            Event::End(TagEnd::CodeBlock)   => { flush_span!(); out.push_str("</text:p>"); in_code=false; }
            Event::Start(Tag::BlockQuote(_)) => out.push_str("<text:p text:style-name=\"Quotations\">"),
            Event::End(TagEnd::BlockQuote(_)) => { flush_span!(); out.push_str("</text:p>"); }
            Event::Rule => out.push_str("<text:p text:style-name=\"Horizontal_Line\"/>"),
            Event::Start(Tag::Image { dest_url, .. }) => {
                flush_span!(); in_image = true; img_src = dest_url.to_string(); img_alt.clear();
            }
            Event::End(TagEnd::Image) => {
                in_image = false;
                if let Some(i) = figures.iter().position(|f| f.src == img_src) {
                    let fig = &figures[i];
                    // px → cm at 96 DPI (1px = 0.02646 cm), capped at 15 cm width.
                    let (cw, ch) = crate::figure_embed::fit_width(fig.w, fig.h, 567); // 567px ≈ 15cm
                    let wc = cw as f32 * 0.02646_f32;
                    let hc = ch as f32 * 0.02646_f32;
                    // Anchored as-char → emit the frame inline within the current
                    // paragraph. A wrapping <text:p> here would nest paragraphs,
                    // which ODF forbids (LibreOffice rejects the whole document).
                    out.push_str(&format!(
                        "<draw:frame draw:style-name=\"Graphics\" draw:name=\"Fig{i}\" \
                        text:anchor-type=\"as-char\" svg:width=\"{w:.3}cm\" svg:height=\"{h:.3}cm\">\
                        <draw:image xlink:href=\"Pictures/fig_{i}.png\" \
                        xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>\
                        </draw:frame>",
                        i = i, w = wc, h = hc
                    ));
                } else if !img_alt.is_empty() {
                    out.push_str(&esc_xml(&img_alt));
                }
                img_src.clear(); img_alt.clear();
            }
            Event::Start(Tag::Table(_)) => { flush_span!(); in_table=true; trows.clear(); trow.clear(); tcell.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut tcell)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => {
                in_table = false;
                let ncol = trows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                out.push_str(&format!(
                    "<table:table table:name=\"Tbl\"><table:table-column table:number-columns-repeated=\"{}\"/>",
                    ncol
                ));
                for r in &trows {
                    out.push_str("<table:table-row>");
                    for i in 0..ncol {
                        let cell = r.get(i).map(|s| s.as_str()).unwrap_or("");
                        out.push_str("<table:table-cell office:value-type=\"string\"><text:p text:style-name=\"Text_Body\">");
                        odt_emit_inline(&mut out, cell, "", "", eq_images, &inline_eqs);
                        out.push_str("</text:p></table:table-cell>");
                    }
                    out.push_str("</table:table-row>");
                }
                out.push_str("</table:table>");
            }
            Event::Text(t) => { if in_image { img_alt.push_str(&t); } else if in_table { tcell.push_str(&t); } else { buf.push_str(&t); } }
            Event::Code(c) => { flush_span!(); out.push_str(&format!("<text:span text:style-name=\"CodeChar\">{}</text:span>", esc_xml(&c))); }
            Event::SoftBreak => { if in_table { tcell.push(' '); } else if !in_image { buf.push(' '); } }
            Event::HardBreak => { flush_span!(); out.push_str("<text:line-break/>"); }
            _ => {}
        }
    }
    out
}

fn odt_manifest(eq_images: &[(String, Vec<u8>, Vec<u8>, u32, u32)], figures: &[DocxFigure]) -> String {
    let mut m = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\
        <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\
        <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
        <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>"
    );
    for (i, (_, png, ..)) in eq_images.iter().enumerate() {
        if !png.is_empty() {
            m.push_str(&format!(
                "<manifest:file-entry manifest:full-path=\"Pictures/eq_{}.png\" manifest:media-type=\"image/png\"/>", i
            ));
        }
    }
    for (i, _) in figures.iter().enumerate() {
        m.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"Pictures/fig_{}.png\" manifest:media-type=\"image/png\"/>", i
        ));
    }
    m.push_str("</manifest:manifest>");
    m
}

const ODT_STYLES: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
  office:version="1.3">
<office:styles>
  <style:style style:name="Text_Body" style:family="paragraph">
    <style:paragraph-properties fo:margin-bottom="0.21cm"/>
    <style:text-properties fo:font-size="12pt"/>
  </style:style>
  <style:style style:name="Heading_1" style:family="paragraph">
    <style:paragraph-properties fo:margin-top="0.42cm" fo:margin-bottom="0.21cm"/>
    <style:text-properties fo:font-size="20pt" fo:font-weight="bold"/>
  </style:style>
  <style:style style:name="Heading_2" style:family="paragraph">
    <style:paragraph-properties fo:margin-top="0.35cm" fo:margin-bottom="0.18cm"/>
    <style:text-properties fo:font-size="16pt" fo:font-weight="bold"/>
  </style:style>
  <style:style style:name="Heading_3" style:family="paragraph">
    <style:paragraph-properties fo:margin-top="0.28cm" fo:margin-bottom="0.14cm"/>
    <style:text-properties fo:font-size="13pt" fo:font-weight="bold" fo:font-style="italic"/>
  </style:style>
  <style:style style:name="Heading_4" style:family="paragraph">
    <style:text-properties fo:font-size="12pt" fo:font-weight="bold"/>
  </style:style>
  <style:style style:name="Subtitle" style:family="paragraph">
    <style:paragraph-properties fo:text-align="center"/>
    <style:text-properties fo:font-size="14pt" fo:font-style="italic"/>
  </style:style>
  <style:style style:name="Code" style:family="paragraph">
    <style:paragraph-properties fo:background-color="#f4f4f4" fo:padding="0.1cm" fo:margin-bottom="0.2cm"/>
    <style:text-properties style:font-name="Courier New" fo:font-size="10pt"/>
  </style:style>
  <style:style style:name="Equation" style:family="paragraph">
    <style:paragraph-properties fo:text-align="center" fo:margin-top="0.3cm" fo:margin-bottom="0.3cm"/>
  </style:style>
  <style:style style:name="Quotations" style:family="paragraph">
    <style:paragraph-properties fo:margin-left="1cm" fo:border-left="0.15cm solid #4a9eff" fo:padding-left="0.3cm"/>
    <style:text-properties fo:font-style="italic" fo:color="#555555"/>
  </style:style>
  <style:style style:name="Horizontal_Line" style:family="paragraph">
    <style:paragraph-properties fo:border-bottom="0.05cm solid #888888" fo:padding-bottom="0.1cm"/>
  </style:style>
  <style:style style:name="Bold"      style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
  <style:style style:name="Italic"    style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
  <style:style style:name="BoldItalic" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic"/></style:style>
  <style:style style:name="Strike"    style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style>
  <style:style style:name="CodeChar"  style:family="text"><style:text-properties style:font-name="Courier New" fo:background-color="#f4f4f4"/></style:style>
  <style:style style:name="Graphics"  style:family="graphic"/>
</office:styles>
</office:document-styles>"##;

// ═════════════════════════════════════════════════════════════════════════════
// EPUB export - epub-builder crate, equations as embedded PNG
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_epub(markdown: &str, output_path: &Path, meta: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};
    use std::io::Cursor;

    let eq_images = render_eq_images(markdown);
    let figures   = crate::figure_embed::collect_figures(markdown, source_dir);

    // Build XHTML body
    let mut body = String::new();
    for block in split_blocks(markdown) {
        match block {
            Block::Text(t) => {
                let html = crate::render::markdown_to_html(t);
                body.push_str(&html);
            }
            Block::Equation(lat) => {
                if let Some(i) = eq_images.iter().position(|(l, ..)| l == lat) {
                    if !eq_images[i].1.is_empty() {
                        body.push_str(&format!(
                            "<p style=\"text-align:center;margin:1em 0\">\
                            <img src=\"../images/eq_{}.png\" alt=\"{}\" style=\"max-width:100%;vertical-align:middle\"/>\
                            </p>",
                            i, esc_xml(lat)
                        ));
                    } else {
                        body.push_str(&format!(
                            "<p style=\"text-align:center;font-style:italic\">{}</p>",
                            esc_xml(&crate::render::latex_to_unicode(lat))
                        ));
                    }
                }
            }
        }
    }

    // Point author-figure <img> at the embedded EPUB resources (added below)
    // so figures travel inside the book instead of dangling as external files.
    for (i, fig) in figures.iter().enumerate() {
        body = body.replace(
            &format!("src=\"{}\"", fig.src),
            &format!("src=\"../images/fig_{}.png\"", i),
        );
    }

    let title = if meta.title.is_empty() { "Document" } else { &meta.title };
    let xhtml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE html>\
        <html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{}\">\
        <head><meta charset=\"UTF-8\"/><title>{}</title>\
        <style>\
        body{{font-family:Georgia,serif;line-height:1.7;max-width:40em;margin:0 auto;padding:1em;color:#1a1a1a}}\
        h1{{font-size:1.8em;border-bottom:2px solid #ccc;padding-bottom:.3em}}\
        h2{{font-size:1.4em;border-bottom:1px solid #eee}}\
        h3{{font-size:1.2em}}\
        pre,code{{font-family:monospace;background:#f4f4f4;border-radius:3px}}\
        pre{{padding:.8em;overflow-x:auto}}code{{padding:0 .3em}}\
        blockquote{{border-left:4px solid #4a9eff;margin:0;padding-left:1em;color:#555}}\
        table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #ddd;padding:.4em .8em}}\
        th{{background:#f4f4f4}}\
        </style></head><body>{}</body></html>",
        if meta.lang.is_empty() { "en" } else { &meta.lang },
        esc_xml(title),
        body
    );

    let mut epub = EpubBuilder::new(
        ZipLibrary::new().map_err(|e| format!("EPUB zip: {}", e))?
    ).map_err(|e| format!("EPUB init: {}", e))?;

    epub.epub_version(EpubVersion::V30);
    epub.metadata("title", title).map_err(|e| format!("EPUB meta: {}", e))?;
    if !meta.author.is_empty() {
        epub.metadata("author", &meta.author).map_err(|e| format!("EPUB meta: {}", e))?;
    }
    if !meta.lang.is_empty() {
        epub.metadata("lang", &meta.lang).map_err(|e| format!("EPUB meta: {}", e))?;
    }

    epub.add_content(
        EpubContent::new("content.xhtml", Cursor::new(xhtml.as_bytes()))
            .title(title)
            .reftype(ReferenceType::Text),
    ).map_err(|e| format!("EPUB content: {}", e))?;

    for (i, (_, png, ..)) in eq_images.iter().enumerate() {
        if !png.is_empty() {
            epub.add_resource(
                &format!("images/eq_{}.png", i),
                Cursor::new(png.as_slice()),
                "image/png",
            ).map_err(|e| format!("EPUB img: {}", e))?;
        }
    }
    for (i, fig) in figures.iter().enumerate() {
        epub.add_resource(
            &format!("images/fig_{}.png", i),
            Cursor::new(fig.png.as_slice()),
            "image/png",
        ).map_err(|e| format!("EPUB fig: {}", e))?;
    }

    let mut buf: Vec<u8> = Vec::new();
    epub.generate(&mut buf).map_err(|e| format!("EPUB generate: {}", e))?;
    std::fs::write(output_path, &buf).map_err(|e| format!("Write: {}", e))
}

// ═════════════════════════════════════════════════════════════════════════════
// Emacs Org-mode export (.org)
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_org(markdown: &str, output_path: &Path, meta: &PdfMetadata) -> Result<(), String> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES
             | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH;
    let mut doc = String::new();

    if !meta.title.is_empty()  { doc.push_str(&format!("#+TITLE: {}\n", meta.title)); }
    if !meta.author.is_empty() { doc.push_str(&format!("#+AUTHOR: {}\n", meta.author)); }
    doc.push_str("#+OPTIONS: toc:t num:t\n\n");

    let mut buf        = String::new();
    let mut in_code    = false;
    let mut is_ordered = false;
    let mut in_bquote  = false;
    let mut in_image   = false; let mut img_url = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush { () => { let t = std::mem::take(&mut buf); if !t.is_empty() { doc.push_str(t.trim()); } }}

    for event in Parser::new_ext(markdown, opts) {
        match event {
            Event::Start(Tag::Table(_)) => { flush!(); trows.clear(); trow.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut buf)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => { doc.push('\n'); doc.push_str(&render_table(&trows, TableStyle::Org)); }
            Event::Start(Tag::Image { dest_url, .. }) => { in_image = true; img_url = dest_url.to_string(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                // Org renders a bare file link inline as the image itself.
                buf.push_str(&format!("[[file:{}]]", img_url));
                img_url.clear();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                let n = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, HeadingLevel::H4=>4, _=>5 };
                doc.push_str(&"*".repeat(n));
                doc.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => { flush!(); doc.push_str("\n\n"); }
            Event::End(TagEnd::Paragraph)  => { flush!(); doc.push_str("\n\n"); }
            Event::Start(Tag::Strong)      => buf.push('*'),
            Event::End(TagEnd::Strong)     => buf.push('*'),
            Event::Start(Tag::Emphasis)    => buf.push('/'),
            Event::End(TagEnd::Emphasis)   => buf.push('/'),
            Event::Start(Tag::Strikethrough)  => buf.push('+'),
            Event::End(TagEnd::Strikethrough) => buf.push('+'),
            Event::Start(Tag::List(s))  => { is_ordered = s.is_some(); }
            Event::End(TagEnd::List(_)) => { doc.push('\n'); }
            Event::Start(Tag::Item) => { if is_ordered { doc.push_str("1. "); } else { doc.push_str("- "); } }
            Event::End(TagEnd::Item) => { flush!(); doc.push('\n'); }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush!(); in_code = true;
                let lang = match &kind { CodeBlockKind::Fenced(l) if !l.is_empty() => l.to_string(), _ => String::new() };
                doc.push_str(&format!("#+begin_src {}\n", lang));
            }
            Event::End(TagEnd::CodeBlock) => {
                doc.push_str(buf.trim_end()); buf.clear();
                doc.push_str("\n#+end_src\n\n"); in_code = false;
            }
            Event::Start(Tag::BlockQuote(_)) => { flush!(); in_bquote = true; doc.push_str("#+begin_quote\n"); }
            Event::End(TagEnd::BlockQuote(_)) => { flush!(); doc.push_str("\n#+end_quote\n\n"); in_bquote = false; }
            Event::Start(Tag::Link { dest_url, .. }) => buf.push_str(&format!("[[{}][", dest_url)),
            Event::End(TagEnd::Link) => buf.push_str("]]"),
            Event::Rule => { flush!(); doc.push_str("-----\n\n"); }
            Event::DisplayMath(m) => { flush!(); doc.push_str(&format!("\n\\begin{{equation}}\n{}\n\\end{{equation}}\n\n", m)); }
            Event::InlineMath(m)  => buf.push_str(&format!("${}$", m)),
            Event::Text(t) => { if in_image { /* alt */ } else { buf.push_str(&t); } }
            Event::Code(c) => buf.push_str(&format!("={}=", c)),
            Event::SoftBreak => buf.push(' '),
            Event::HardBreak => { flush!(); doc.push('\n'); }
            _ => {}
        }
    }

    std::fs::write(output_path, &doc).map_err(|e| format!("Write: {}", e))
}

// ═════════════════════════════════════════════════════════════════════════════
// reStructuredText export (.rst)
// ═════════════════════════════════════════════════════════════════════════════

/// Escape the RST inline-markup triggers in prose text. Emphasis/code/math come
/// through their own events (not Text), so escaping here is safe and prevents
/// stray `|` (substitution refs) and unmatched `*`/backtick from breaking parse.
fn esc_rst_text(s: &str) -> String {
    s.replace('*', "\\*").replace('`', "\\`").replace('|', "\\|")
}

pub fn export_rst(markdown: &str, output_path: &Path, meta: &PdfMetadata) -> Result<(), String> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_MATH;
    let underlines = ['=', '-', '~', '^', '"'];
    let mut doc = String::new();

    if !meta.title.is_empty() {
        let u = "=".repeat(meta.title.len());
        doc.push_str(&format!("{}\n{}\n{}\n\n", u, meta.title, u));
    }
    if !meta.author.is_empty() { doc.push_str(&format!(":Author: {}\n\n", meta.author)); }

    let mut buf        = String::new();
    let mut in_code    = false;
    let mut code_lang  = String::new();
    let mut is_ordered = false;
    let mut in_image   = false; let mut img_url = String::new(); let mut img_alt = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush { () => { let t = std::mem::take(&mut buf); if !t.trim().is_empty() { doc.push_str(t.trim()); } }}

    for event in Parser::new_ext(markdown, opts) {
        match event {
            Event::Start(Tag::Table(_)) => { flush!(); trows.clear(); trow.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut buf)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => doc.push_str(&render_table(&trows, TableStyle::Rst)),
            Event::Start(Tag::Image { dest_url, .. }) => { flush!(); in_image = true; img_url = dest_url.to_string(); img_alt.clear(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                doc.push_str(&format!("\n\n.. image:: {}\n", img_url));
                if !img_alt.is_empty() { doc.push_str(&format!("   :alt: {}\n", img_alt)); }
                doc.push('\n');
                img_url.clear(); img_alt.clear();
            }
            Event::Start(Tag::Heading { level: _, .. }) => {
                flush!();
                doc.push('\n');
            }
            Event::End(TagEnd::Heading(level)) => {
                let lvl = match level { HeadingLevel::H1=>0, HeadingLevel::H2=>1, HeadingLevel::H3=>2, HeadingLevel::H4=>3, _=>4 };
                let uc  = underlines.get(lvl).copied().unwrap_or('"');
                let t   = buf.trim().to_string(); buf.clear();
                let u   = uc.to_string().repeat(t.len().max(4));
                doc.push_str(&format!("{}\n{}\n\n", t, u));
            }
            Event::End(TagEnd::Paragraph)  => { flush!(); doc.push_str("\n\n"); }
            Event::Start(Tag::Strong)      => buf.push_str("**"),
            Event::End(TagEnd::Strong)     => buf.push_str("**"),
            Event::Start(Tag::Emphasis)    => buf.push('*'),
            Event::End(TagEnd::Emphasis)   => buf.push('*'),
            Event::Start(Tag::List(s))     => { is_ordered = s.is_some(); }
            Event::End(TagEnd::List(_))    => doc.push('\n'),
            Event::Start(Tag::Item) => { if is_ordered { doc.push_str("#. "); } else { doc.push_str("- "); } }
            Event::End(TagEnd::Item) => { flush!(); doc.push('\n'); }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush!(); in_code = true;
                code_lang = match &kind { CodeBlockKind::Fenced(l) if !l.is_empty() => l.to_string(), _ => String::new() };
                doc.push_str(&format!(".. code-block:: {}\n\n", code_lang));
            }
            Event::End(TagEnd::CodeBlock) => {
                for line in buf.lines() { doc.push_str(&format!("   {}\n", line)); }
                buf.clear(); doc.push('\n'); in_code = false;
            }
            Event::Start(Tag::BlockQuote(_)) => {}
            Event::End(TagEnd::BlockQuote(_)) => {
                let t = std::mem::take(&mut buf);
                for line in t.lines() { doc.push_str(&format!("   {}\n", line.trim())); }
                doc.push('\n');
            }
            Event::DisplayMath(m) => { flush!(); doc.push_str(&format!(".. math::\n\n   {}\n\n", m.replace('\n', "\n   "))); }
            // Wrap the role in RST null separators (backslash-space) so it may
            // abut text/bold/dashes without "inline markup without end-string".
            Event::InlineMath(m)  => buf.push_str(&format!("\\ :math:`{}`\\ ", m)),
            Event::Rule => { flush!(); doc.push_str("\n----\n\n"); }
            Event::Text(t) => { if in_image { img_alt.push_str(&t); } else { buf.push_str(&esc_rst_text(&t)); } }
            Event::Code(c) => buf.push_str(&format!("\\ ``{}``\\ ", c)),
            Event::SoftBreak => buf.push(' '),
            Event::HardBreak => { flush!(); doc.push_str("\n\n"); }
            _ => {}
        }
    }

    std::fs::write(output_path, &doc).map_err(|e| format!("Write: {}", e))
}

// ═════════════════════════════════════════════════════════════════════════════
// AsciiDoc export (.adoc)
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_adoc(markdown: &str, output_path: &Path, meta: &PdfMetadata) -> Result<(), String> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_MATH;
    let mut doc = String::new();

    if !meta.title.is_empty()  { doc.push_str(&format!("= {}\n", meta.title)); }
    if !meta.author.is_empty() { doc.push_str(&format!("{}\n", meta.author)); }
    doc.push_str(":toc:\n:stem: latexmath\n\n");

    let mut buf        = String::new();
    let mut in_code    = false;
    let mut code_lang  = String::new();
    let mut is_ordered = false;
    let mut in_image   = false; let mut img_url = String::new(); let mut img_alt = String::new();
    let mut trow: Vec<String> = Vec::new(); let mut trows: Vec<Vec<String>> = Vec::new();

    macro_rules! flush { () => { let t = std::mem::take(&mut buf); if !t.trim().is_empty() { doc.push_str(t.trim()); } }}

    for event in Parser::new_ext(markdown, opts) {
        match event {
            Event::Start(Tag::Table(_)) => { flush!(); trows.clear(); trow.clear(); }
            Event::End(TagEnd::TableCell) => trow.push(std::mem::take(&mut buf)),
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => trows.push(std::mem::take(&mut trow)),
            Event::End(TagEnd::Table) => doc.push_str(&render_table(&trows, TableStyle::Adoc)),
            Event::Start(Tag::Image { dest_url, .. }) => { flush!(); in_image = true; img_url = dest_url.to_string(); img_alt.clear(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                doc.push_str(&format!("\n\nimage::{}[{}]\n\n", img_url, img_alt));
                img_url.clear(); img_alt.clear();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                let lvl = match level { HeadingLevel::H1=>2, HeadingLevel::H2=>3, HeadingLevel::H3=>4, HeadingLevel::H4=>5, _=>6 };
                doc.push_str(&"=".repeat(lvl));
                doc.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => { flush!(); doc.push_str("\n\n"); }
            Event::End(TagEnd::Paragraph)  => { flush!(); doc.push_str("\n\n"); }
            Event::Start(Tag::Strong)      => buf.push('*'),
            Event::End(TagEnd::Strong)     => buf.push('*'),
            Event::Start(Tag::Emphasis)    => buf.push('_'),
            Event::End(TagEnd::Emphasis)   => buf.push('_'),
            Event::Start(Tag::Strikethrough)  => buf.push_str("[.line-through]#"),
            Event::End(TagEnd::Strikethrough) => buf.push('#'),
            Event::Start(Tag::List(s))  => { is_ordered = s.is_some(); }
            Event::End(TagEnd::List(_)) => doc.push('\n'),
            Event::Start(Tag::Item) => { if is_ordered { doc.push_str(". "); } else { doc.push_str("* "); } }
            Event::End(TagEnd::Item) => { flush!(); doc.push('\n'); }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush!(); in_code = true;
                code_lang = match &kind { CodeBlockKind::Fenced(l) if !l.is_empty() => l.to_string(), _ => String::new() };
                if !code_lang.is_empty() { doc.push_str(&format!("[source,{}]\n", code_lang)); }
                doc.push_str("----\n");
            }
            Event::End(TagEnd::CodeBlock) => {
                doc.push_str(buf.trim_end()); buf.clear();
                doc.push_str("\n----\n\n"); in_code = false;
            }
            Event::Start(Tag::BlockQuote(_)) => { flush!(); doc.push_str("____\n"); }
            Event::End(TagEnd::BlockQuote(_)) => { flush!(); doc.push_str("\n____\n\n"); }
            Event::DisplayMath(m) => { flush!(); doc.push_str(&format!("[stem]\n++++\n{}\n++++\n\n", m)); }
            Event::InlineMath(m)  => buf.push_str(&format!("stem:[{}]", m)),
            Event::Rule => { flush!(); doc.push_str("'''\n\n"); }
            Event::Text(t) => { if in_image { img_alt.push_str(&t); } else { buf.push_str(&t); } }
            Event::Code(c) => buf.push_str(&format!("`{}`", c)),
            Event::SoftBreak => buf.push(' '),
            Event::HardBreak => { flush!(); doc.push_str(" +\n"); }
            _ => {}
        }
    }

    std::fs::write(output_path, &doc).map_err(|e| format!("Write: {}", e))
}

// ═════════════════════════════════════════════════════════════════════════════
// Jupyter Notebook export (.ipynb)
// ═════════════════════════════════════════════════════════════════════════════

pub fn export_ipynb(markdown: &str, output_path: &Path, _meta: &PdfMetadata) -> Result<(), String> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_MATH;
    let mut cells: Vec<serde_json::Value> = Vec::new();
    let mut md_buf   = String::new();
    let mut code_buf = String::new();
    let mut in_code  = false;
    let mut code_lang = String::new();
    let mut in_image = false; let mut img_url = String::new(); let mut img_alt = String::new();

    // Helper: flush accumulated markdown text as a markdown cell
    let flush_md = |md_buf: &mut String, cells: &mut Vec<serde_json::Value>| {
        let t = std::mem::take(md_buf);
        if !t.trim().is_empty() {
            let source: Vec<serde_json::Value> = t.trim_end().split('\n')
                .map(|l| serde_json::Value::String(format!("{}\n", l)))
                .collect();
            cells.push(serde_json::json!({
                "cell_type": "markdown", "metadata": {}, "source": source
            }));
        }
    };
    let flush_code = |code_buf: &mut String, cells: &mut Vec<serde_json::Value>| {
        let c = std::mem::take(code_buf);
        if !c.trim().is_empty() {
            let source: Vec<serde_json::Value> = c.trim_end().split('\n')
                .map(|l| serde_json::Value::String(format!("{}\n", l)))
                .collect();
            cells.push(serde_json::json!({
                "cell_type": "code", "metadata": {},
                "source": source, "outputs": [], "execution_count": null
            }));
        }
    };

    for event in Parser::new_ext(markdown, opts) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_md(&mut md_buf, &mut cells);
                in_code   = true;
                code_lang = match &kind { CodeBlockKind::Fenced(l) if !l.is_empty() => l.to_string(), _ => "python".into() };
            }
            Event::End(TagEnd::CodeBlock) => {
                flush_code(&mut code_buf, &mut cells);
                in_code = false;
            }
            Event::Start(Tag::Image { dest_url, .. }) => { in_image = true; img_url = dest_url.to_string(); img_alt.clear(); }
            Event::End(TagEnd::Image) => {
                in_image = false;
                md_buf.push_str(&format!("![{}]({})", img_alt, img_url));
                img_url.clear(); img_alt.clear();
            }
            Event::Text(t) => {
                if in_image { img_alt.push_str(&t); }
                else if in_code { code_buf.push_str(&t); }
                else { md_buf.push_str(&t); }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let n = match level { HeadingLevel::H1=>1, HeadingLevel::H2=>2, HeadingLevel::H3=>3, _=>4 };
                md_buf.push_str(&"#".repeat(n));
                md_buf.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => md_buf.push_str("\n\n"),
            Event::End(TagEnd::Paragraph)  => md_buf.push_str("\n\n"),
            Event::Start(Tag::Strong)      => md_buf.push_str("**"),
            Event::End(TagEnd::Strong)     => md_buf.push_str("**"),
            Event::Start(Tag::Emphasis)    => md_buf.push('*'),
            Event::End(TagEnd::Emphasis)   => md_buf.push('*'),
            Event::Start(Tag::Item)        => md_buf.push_str("- "),
            Event::End(TagEnd::Item)       => md_buf.push('\n'),
            Event::DisplayMath(m) => md_buf.push_str(&format!("\n$$\n{}\n$$\n\n", m)),
            Event::InlineMath(m)  => md_buf.push_str(&format!("${}$", m)),
            Event::Code(c)  => md_buf.push_str(&format!("`{}`", c)),
            Event::Rule     => md_buf.push_str("\n---\n\n"),
            Event::SoftBreak => md_buf.push(' '),
            Event::HardBreak => md_buf.push('\n'),
            _ => {}
        }
    }
    flush_md(&mut md_buf, &mut cells);

    let notebook = serde_json::json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {
            "kernelspec": { "name": "python3", "display_name": "Python 3", "language": "python" },
            "language_info": { "name": "python" }
        },
        "cells": cells
    });

    let json = serde_json::to_string_pretty(&notebook)
        .map_err(|e| format!("Jupyter JSON: {}", e))?;
    std::fs::write(output_path, json).map_err(|e| format!("Write: {}", e))
}

#[cfg(test)]
mod docx_inline_tests {
    use super::*;

    #[test]
    fn placeholder_extracts_inline_and_keeps_display() {
        let (txt, eqs) = placeholder_inline_math("a $x^2$ b $$D$$ c \\(y\\) d");
        assert_eq!(eqs, vec!["x^2".to_string(), "y".to_string()]);
        assert!(txt.contains("$$D$$"), "display math must stay verbatim: {txt:?}");
        assert!(txt.contains(EQ_PH_OPEN) && txt.contains(EQ_PH_CLOSE), "no placeholder: {txt:?}");
        assert!(!txt.contains("$x^2$"), "inline math left literal: {txt:?}");
    }

    #[test]
    fn inline_equation_does_not_fragment_paragraph() {
        // Regression: inline $...$ used to split the sentence into 3 paragraphs.
        // The whole sentence must stay in a single <w:p> (one open, one close).
        let xml = md_fragment_to_docx("where $x = 1$ holds for all cases.", &[], &[]);
        assert_eq!(xml.matches("<w:p>").count(), 1, "paragraph fragmented: {xml}");
        assert_eq!(xml.matches("</w:p>").count(), 1, "paragraph fragmented: {xml}");
        // The prose on both sides of the equation is preserved.
        assert!(xml.contains("where "), "lead text lost: {xml}");
        assert!(xml.contains(" holds for all cases."), "trailing text lost: {xml}");
    }

    #[test]
    fn inline_equation_renders_as_image_when_available() {
        // With an image available for the latex, the inline run is a drawing.
        let eq_images = vec![("x".to_string(), vec![1u8, 2, 3], Vec::new(), 40u32, 20u32)];
        let xml = md_fragment_to_docx("a $x$ b", &eq_images, &[]);
        assert!(xml.contains("<w:drawing>"), "inline image not emitted: {xml}");
        assert_eq!(xml.matches("<w:p>").count(), 1, "fragmented: {xml}");
    }

    #[test]
    fn docx_roundtrip_recovers_latex_including_inline() {
        // THE core differentiator (project spec §3): an exported DOCX must let
        // MD -> ALL recover the ORIGINAL editable LaTeX on re-import. The inline
        // fix removed the per-inline Word comment, so prove the lossless
        // md-to-all-source.xml layer still round-trips both inline and display math.
        let md = "Intro with inline $E = mc^2$ and display below:\n\n$$\\int_0^1 x\\,dx = \\frac{1}{2}$$\n\nEnd $\\alpha_n$ here.\n";
        let path = std::env::temp_dir().join("mdall_roundtrip_test.docx");
        export_docx(md, &path, &PdfMetadata::default(), None).expect("export");
        let recovered = crate::source_embed::import_docx_source(&path).expect("recover");
        let _ = std::fs::remove_file(&path);
        assert!(recovered.contains("$E = mc^2$"), "inline latex lost: {recovered:?}");
        assert!(recovered.contains("\\int_0^1 x"), "display latex lost: {recovered:?}");
        assert!(recovered.contains("$\\alpha_n$"), "second inline latex lost: {recovered:?}");
        assert_eq!(recovered, md, "round-trip is not byte-identical: {recovered:?}");
    }

    #[test]
    fn docx_embeds_author_figure_into_media() {
        use std::io::Read as _;
        let dir = std::env::temp_dir().join("mdall_docx_figure_test");
        let _ = std::fs::create_dir_all(&dir);
        let img_path = dir.join("pic.png");
        // A tiny real PNG resolved relative to the document folder.
        image::RgbaImage::from_pixel(3, 2, image::Rgba([200, 30, 30, 255]))
            .save(&img_path)
            .expect("write test png");

        let md = "Intro paragraph.\n\n![A caption](pic.png)\n";
        let out = dir.join("figure.docx");
        export_docx(md, &out, &PdfMetadata::default(), Some(&dir)).expect("export_docx failed");

        let f = std::fs::File::open(&out).expect("open docx");
        let mut zip = zip::ZipArchive::new(f).expect("read zip");
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "word/media/fig_0.png"),
            "figure binary not packed into media: {names:?}"
        );
        let mut doc = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut doc)
            .unwrap();
        assert!(doc.contains("rIdFig0"), "figure relationship not referenced in body");
        assert!(doc.contains("<w:drawing>"), "figure drawing run missing");
        // The reversibility layer must still be present alongside figures.
        assert!(
            names.iter().any(|n| n == crate::source_embed::DOCX_SOURCE_ENTRY),
            "source.xml dropped when figures present: {names:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
