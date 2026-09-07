// Pure-Rust PDF backend via Typst - zero external dependencies.
// Converts Markdown + LaTeX math to a Typst document, renders to PDF in memory.

use crate::export::PdfMetadata;
use comemo::Prehashed;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use typst::diag::{FileError, FileResult};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::{Library, World};

// ── World implementation ──────────────────────────────────────────────────────

struct MdWorld {
    library: Prehashed<Library>,
    book: Prehashed<FontBook>,
    fonts: Vec<Font>,
    source: Source,
    images: HashMap<PathBuf, Bytes>,
}

impl MdWorld {
    fn new(
        typst_source: &str,
        source_dir: Option<&Path>,
    ) -> Result<Self, String> {
        let mut book = FontBook::new();
        let mut fonts: Vec<Font> = Vec::new();

        // Embedded Typst fonts FIRST - includes New Computer Modern Math, the
        // OpenType MATH-table font required for math mode. Windows system fonts
        // alone have NO math font, so without these EVERY equation in the
        // full-document PDF fails with "current font does not support math".
        // (equation_renderer already does this for per-equation images.)
        for data in typst_assets::fonts() {
            let bytes = Bytes::from_static(data);
            for face_idx in 0u32.. {
                match Font::new(bytes.clone(), face_idx) {
                    Some(f) => { book.push(f.info().clone()); fonts.push(f); }
                    None => break,
                }
            }
        }

        // Load Windows system fonts
        let font_paths = [
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\segoeuib.ttf",
            r"C:\Windows\Fonts\segoeuii.ttf",
            r"C:\Windows\Fonts\segoeuiz.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\arialbd.ttf",
            r"C:\Windows\Fonts\ariali.ttf",
            r"C:\Windows\Fonts\times.ttf",
            r"C:\Windows\Fonts\calibri.ttf",
            r"C:\Windows\Fonts\consola.ttf",
            r"C:\Windows\Fonts\NotoSansMath-Regular.ttf",
            r"C:\Windows\Fonts\seguisym.ttf",
        ];

        let mut loaded = false;
        for path in &font_paths {
            if let Ok(data) = std::fs::read(path) {
                let bytes = Bytes::from(data);
                for i in 0.. {
                    match Font::new(bytes.clone(), i) {
                        Some(f) => {
                            book.push(f.info().clone());
                            fonts.push(f);
                            loaded = true;
                        }
                        None => break,
                    }
                }
            }
        }

        let _ = loaded; // Windows fonts are optional now (embedded fonts suffice).
        if fonts.is_empty() {
            return Err("No fonts available - cannot render PDF".to_string());
        }

        // Collect images from source directory
        let mut images: HashMap<PathBuf, Bytes> = HashMap::new();
        if let Some(dir) = source_dir {
            for ext in &["png", "jpg", "jpeg", "gif", "svg", "webp"] {
                let _pattern = dir.join(format!("*.{}", ext));
                if let Ok(entries) = glob_images(dir, ext) {
                    for path in entries {
                        if let Ok(data) = std::fs::read(&path) {
                            let rel = path.strip_prefix(dir).unwrap_or(&path).to_path_buf();
                            images.insert(rel, Bytes::from(data));
                        }
                    }
                }
            }
        }

        let source = Source::detached(typst_source.to_string());

        Ok(Self {
            library: Prehashed::new(Library::builder().build()),
            book: Prehashed::new(book),
            fonts,
            source,
            images,
        })
    }
}

fn glob_images(dir: &Path, ext: &str) -> Result<Vec<PathBuf>, ()> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
            {
                results.push(p);
            }
        }
    }
    Ok(results)
}

impl World for MdWorld {
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.book
    }

    fn main(&self) -> Source {
        self.source.clone()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        Err(FileError::NotFound(
            id.vpath().as_rootless_path().to_path_buf(),
        ))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let path = id.vpath().as_rootless_path();
        if let Some(bytes) = self.images.get(path) {
            return Ok(bytes.clone());
        }
        // Absolute path fallback for images referenced with full paths
        if let Ok(data) = std::fs::read(path) {
            return Ok(Bytes::from(data));
        }
        Err(FileError::NotFound(path.to_path_buf()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        None
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn export_pdf_typst(
    markdown: &str,
    output_path: &Path,
    metadata: &PdfMetadata,
    source_dir: Option<&Path>,
) -> Result<(), String> {
    let typst_src = markdown_to_typst(markdown, metadata, source_dir);

    let world = MdWorld::new(&typst_src, source_dir)
        .map_err(|e| format!("Typst world init: {}", e))?;

    let mut tracer = Tracer::new();
    let document = typst::compile(&world, &mut tracer)
        .map_err(|errs| {
            let msgs: Vec<String> = errs
                .iter()
                .map(|e| e.message.to_string())
                .collect();
            format!("Typst compile error: {}", msgs.join("; "))
        })?;

    use typst::foundations::Smart;
    let pdf_bytes = typst_pdf::pdf(&document, Smart::Auto, None);

    std::fs::write(output_path, pdf_bytes)
        .map_err(|e| format!("PDF write error: {}", e))?;

    Ok(())
}

// ── Markdown → Typst source converter ────────────────────────────────────────

fn markdown_to_typst(
    markdown: &str,
    metadata: &PdfMetadata,
    source_dir: Option<&Path>,
) -> String {
    let mut out = String::new();

    // Preamble
    out.push_str("#set page(paper: \"us-letter\", margin: (x: 2.5cm, y: 2.5cm))\n");
    out.push_str("#set text(size: 11pt, lang: \"en\")\n");
    out.push_str("#set par(justify: true, leading: 0.65em)\n");
    out.push_str("#show raw: it => block(fill: luma(230), inset: 8pt, radius: 4pt, it)\n");
    out.push_str("#show heading: it => { set text(weight: \"bold\"); it; v(0.3em) }\n");

    if !metadata.title.is_empty() {
        out.push_str(&format!(
            "#align(center)[#text(18pt, weight: \"bold\")[{}]]\n",
            typst_escape(&metadata.title)
        ));
        if !metadata.author.is_empty() {
            out.push_str(&format!(
                "#align(center)[#text(12pt)[{}]]\n",
                typst_escape(&metadata.author)
            ));
        }
        out.push_str("#v(1em)\n\n");
    }

    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;
    let mut in_code_block = false;
    let mut in_math_block = false;
    let mut math_buf = String::new();
    while i < lines.len() {
        let line = lines[i];

        // ── Code blocks ───────────────────────────────────────────────────────
        if !in_math_block && line.trim_start().starts_with("```") {
            if !in_code_block {
                let code_lang = line.trim_start().trim_start_matches('`').trim();
                if code_lang.is_empty() {
                    out.push_str("```\n");
                } else {
                    out.push_str(&format!("```{}\n", code_lang));
                }
                in_code_block = true;
            } else {
                out.push_str("```\n\n");
                in_code_block = false;
            }
            i += 1;
            continue;
        }
        if in_code_block {
            out.push_str(line);
            out.push('\n');
            i += 1;
            continue;
        }

        // ── Display math blocks ───────────────────────────────────────────────
        let trimmed = line.trim();
        if !in_math_block && trimmed.starts_with("$$") {
            let after = trimmed.trim_start_matches("$$").trim();
            if !after.is_empty() && after.ends_with("$$") {
                // Single-line: $$ expr $$
                let content = after.trim_end_matches("$$").trim();
                out.push_str(&format!(
                    "#align(center)[${}$]\n\n",
                    latex_to_typst_math(content)
                ));
                i += 1;
                continue;
            }
            in_math_block = true;
            math_buf.clear();
            if !after.is_empty() {
                math_buf.push_str(after);
            }
            i += 1;
            continue;
        }
        if in_math_block {
            if trimmed == "$$" || (trimmed.ends_with("$$") && trimmed.len() > 2) {
                let before = trimmed.trim_end_matches("$$").trim();
                if !before.is_empty() {
                    if !math_buf.is_empty() { math_buf.push('\n'); }
                    math_buf.push_str(before);
                }
                out.push_str(&format!(
                    "#align(center)[$\n{}\n$]\n\n",
                    latex_to_typst_math(&math_buf)
                ));
                in_math_block = false;
                math_buf.clear();
            } else {
                if !math_buf.is_empty() { math_buf.push('\n'); }
                math_buf.push_str(trimmed);
            }
            i += 1;
            continue;
        }

        // ── Headings ──────────────────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("#### ") {
            out.push_str(&format!("==== {}\n\n", convert_inline(rest.trim())));
            i += 1; continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            out.push_str(&format!("=== {}\n\n", convert_inline(rest.trim())));
            i += 1; continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            out.push_str(&format!("== {}\n\n", convert_inline(rest.trim())));
            i += 1; continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            out.push_str(&format!("= {}\n\n", convert_inline(rest.trim())));
            i += 1; continue;
        }

        // ── Horizontal rule ───────────────────────────────────────────────────
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push_str("#line(length: 100%)\n\n");
            i += 1; continue;
        }

        // ── Blockquote ────────────────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("> ") {
            out.push_str(&format!("#quote[{}]\n\n", convert_inline(rest.trim())));
            i += 1; continue;
        }

        // ── Unordered list ────────────────────────────────────────────────────
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            out.push_str(&format!("- {}\n", convert_inline(rest.trim())));
            i += 1; continue;
        }

        // ── Ordered list (simple: starts with digit + ". ") ───────────────────
        if line.len() > 3
            && line.chars().next().unwrap().is_ascii_digit()
            && line.get(1..3) == Some(". ")
        {
            out.push_str(&format!("+ {}\n", convert_inline(line[3..].trim())));
            i += 1; continue;
        }

        // ── Image (standalone line) ───────────────────────────────────────────
        if trimmed.starts_with("![") {
            if let Some(img) = parse_image(trimmed, source_dir) {
                out.push_str(&img);
                i += 1; continue;
            }
        }

        // ── Empty line ────────────────────────────────────────────────────────
        if trimmed.is_empty() {
            out.push('\n');
            i += 1; continue;
        }

        // ── Regular paragraph ─────────────────────────────────────────────────
        out.push_str(&convert_inline(line));
        out.push('\n');
        i += 1;
    }

    out
}

// ── Inline Markdown → Typst ──────────────────────────────────────────────────

fn convert_inline(text: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Bold: **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_closing(&chars, i + 2, "**") {
                result.push('*');
                result.push_str(&convert_inline_simple(&chars[i+2..end]));
                result.push('*');
                i = end + 2;
                continue;
            }
        }
        // Italic: *text* or _text_
        if chars[i] == '*' || chars[i] == '_' {
            let delim = chars[i];
            if let Some(end) = find_closing_char(&chars, i + 1, delim) {
                if end > i + 1 {
                    result.push('_');
                    result.push_str(&convert_inline_simple(&chars[i+1..end]));
                    result.push('_');
                    i = end + 1;
                    continue;
                }
            }
        }
        // Inline code: `code`
        if chars[i] == '`' {
            if let Some(end) = find_closing_char(&chars, i + 1, '`') {
                let code: String = chars[i+1..end].iter().collect();
                result.push('`');
                result.push_str(&code);
                result.push('`');
                i = end + 1;
                continue;
            }
        }
        // Inline math: $math$
        if chars[i] == '$' && (i + 1 >= len || chars[i + 1] != '$') {
            if let Some(end) = find_closing_char(&chars, i + 1, '$') {
                if end > i + 1 {
                    let math: String = chars[i+1..end].iter().collect();
                    result.push('$');
                    result.push_str(&latex_to_typst_math(&math));
                    result.push('$');
                    i = end + 1;
                    continue;
                }
            }
        }
        // Links: [text](url)
        if chars[i] == '[' {
            if let Some(bracket_end) = find_closing_char(&chars, i + 1, ']') {
                if bracket_end + 1 < len && chars[bracket_end + 1] == '(' {
                    if let Some(paren_end) = find_closing_char(&chars, bracket_end + 2, ')') {
                        let _link_text: String = chars[i+1..bracket_end].iter().collect();
                        let url: String = chars[bracket_end+2..paren_end].iter().collect();
                        result.push_str(&format!(
                            "#link(\"{}\")[{}]",
                            url, convert_inline_simple(&chars[i+1..bracket_end])
                        ));
                        i = paren_end + 1;
                        continue;
                    }
                }
            }
        }
        // Strikethrough: ~~text~~
        if i + 1 < len && chars[i] == '~' && chars[i + 1] == '~' {
            if let Some(end) = find_closing(&chars, i + 2, "~~") {
                result.push_str(&format!(
                    "#strike[{}]",
                    convert_inline_simple(&chars[i+2..end])
                ));
                i = end + 2;
                continue;
            }
        }
        // Default: escape Typst special chars
        result.push_str(&typst_escape_char(chars[i]));
        i += 1;
    }

    result
}

fn convert_inline_simple(chars: &[char]) -> String {
    let s: String = chars.iter().collect();
    typst_escape(&s)
}

fn typst_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        out.push_str(&typst_escape_char(ch));
    }
    out
}

fn typst_escape_char(ch: char) -> String {
    match ch {
        '@' | '#' | '<' | '>' | '\\' | '~' => format!("\\{}", ch),
        _ => ch.to_string(),
    }
}

fn find_closing(chars: &[char], start: usize, pattern: &str) -> Option<usize> {
    let pat: Vec<char> = pattern.chars().collect();
    let plen = pat.len();
    let n = chars.len();
    let mut i = start;
    while i + plen <= n {
        if chars[i..i+plen] == pat[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_closing_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for (j, &ch) in chars[start..].iter().enumerate() {
        if ch == target {
            return Some(start + j);
        }
    }
    None
}

fn parse_image(line: &str, source_dir: Option<&Path>) -> Option<String> {
    // ![alt](path)
    let rest = line.strip_prefix("![")?;
    let bracket_end = rest.find("](")?;
    let path_start = bracket_end + 2;
    let path_end = rest[path_start..].find(')')?;
    let raw_path = &rest[path_start..path_start + path_end];

    let abs_path = if let Some(dir) = source_dir {
        let p = dir.join(raw_path);
        if p.exists() {
            p.display().to_string().replace('\\', "/")
        } else {
            raw_path.replace('\\', "/")
        }
    } else {
        raw_path.replace('\\', "/")
    };

    Some(format!(
        "#figure(image(\"{}\"), caption: [])\n\n",
        abs_path
    ))
}

// ── LaTeX → Typst math converter ─────────────────────────────────────────────

/// In LaTeX math a bare run of letters is an implicit product of single-letter
/// variables (`ERV` renders as E·R·V), but in Typst the same run is ONE
/// identifier (`ERV`) that fails as "unknown variable". Insert spaces between the
/// letters of bare runs so Typst renders the identical italic product. Letters
/// that belong to a `\command` name, and the brace group of text/upright-style
/// commands (\text, \mathrm, \operatorname, \begin{env}, ...), are left untouched.
/// Must run on RAW LaTeX, before any conversion emits multi-letter Typst tokens
/// (upright, lr, alpha, ...) which must NOT be split.
fn space_bare_identifiers(s: &str) -> String {
    // Commands whose first {...} group is upright/text or a structural name.
    const PROTECT: &[&str] = &[
        "text", "mathrm", "operatorname", "mathbf", "mathit", "mathsf",
        "mathfrak", "mathtt", "mathbb", "mathcal", "mathscr", "textbf",
        "textit", "texttt", "textrm", "boldsymbol", "begin", "end",
    ];
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(n + 16);
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '\\' {
            out.push(c);
            i += 1;
            let name_start = i;
            while i < n && chars[i].is_ascii_alphabetic() { out.push(chars[i]); i += 1; }
            let name: String = chars[name_start..i].iter().collect();
            if name.is_empty() {
                // Symbol command (\\, \,, \{, ...): emit the single escaped char.
                if i < n { out.push(chars[i]); i += 1; }
                continue;
            }
            if i < n && chars[i] == '*' { out.push('*'); i += 1; }
            if PROTECT.contains(&name.as_str()) {
                while i < n && chars[i] == ' ' { out.push(' '); i += 1; }
                if i < n && chars[i] == '{' {
                    let mut depth = 0i32;
                    while i < n {
                        let ch = chars[i];
                        out.push(ch);
                        i += 1;
                        if ch == '{' { depth += 1; }
                        else if ch == '}' { depth -= 1; if depth == 0 { break; } }
                    }
                }
            }
            // A converted command word (\alpha, \Delta, \sin) jams into a
            // following letter or command after conversion (alphasin, kDelta).
            // Insert a boundary so each token stays a distinct Typst identifier.
            if i < n && (chars[i].is_ascii_alphabetic() || chars[i] == '\\') {
                out.push(' ');
            }
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < n && chars[i].is_ascii_alphabetic() { i += 1; }
            if i - start >= 2 {
                for (k, idx) in (start..i).enumerate() {
                    if k > 0 { out.push(' '); }
                    out.push(chars[idx]);
                }
            } else {
                out.push(chars[start]);
            }
            // A letter/run immediately followed by a command word also jams.
            if i < n && chars[i] == '\\' { out.push(' '); }
            continue;
        }
        if c.is_ascii_digit() {
            // A subscript/superscript digit followed by a command (omega_0\tau)
            // jams too; keep the boundary.
            out.push(c);
            i += 1;
            if i < n && chars[i] == '\\' { out.push(' '); }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

pub fn latex_to_typst_math(latex: &str) -> String {
    // Expand the document's custom macros (\KK, \tauc, \sket, ...) using the
    // thread-active table installed at the export entry point. Without this,
    // custom-macro-heavy papers emit unknown Typst identifiers that fail to
    // compile (and the same equations rendered as PDF/DOCX images degraded).
    // No-op when no macros are installed.
    let expanded = crate::latex_macros::expand_active(latex);
    // Shared sanitization: drop \label/\tag/\nonumber and normalize spacing
    // macros so Typst does not fail on non-visual LaTeX constructs.
    let sanitized = crate::latex_macros::sanitize_latex(&expanded);
    // Space out bare multi-letter variable runs (E·R·V) before any conversion.
    let mut s = space_bare_identifiers(sanitized.trim());

    // Remove LaTeX environments (typst has equivalents but we handle them specially)
    for env in &[
        "align", "aligned", "equation", "gather", "gathered", "split",
        "array", "eqnarray",
    ] {
        s = s.replace(&format!("\\begin{{{}}}", env), "");
        s = s.replace(&format!("\\end{{{}}}", env), "");
    }

    // matrix environments → mat(...)
    for env in &["matrix", "pmatrix", "bmatrix", "vmatrix", "Bmatrix"] {
        let open = format!("\\begin{{{}}}", env);
        let close = format!("\\end{{{}}}", env);
        if let Some(start) = s.find(&open) {
            if let Some(end) = s.find(&close) {
                let inner = s[start + open.len()..end].trim().to_string();
                let mat = inner.replace("\\\\", ",").replace("&", " ");
                s = format!("{}mat({}){}",
                    &s[..start], mat, &s[end + close.len()..]);
            }
        }
    }

    // cases → cases(...)
    let cases_open = "\\begin{cases}";
    let cases_close = "\\end{cases}";
    if let Some(start) = s.find(cases_open) {
        if let Some(end) = s.find(cases_close) {
            let inner = s[start + cases_open.len()..end].trim().to_string();
            let items: Vec<&str> = inner.split("\\\\").collect();
            let converted: Vec<String> = items.iter()
                .map(|item| item.replace("&", " "))
                .collect();
            s = format!("{}cases({}){}",
                &s[..start], converted.join(", "), &s[end + cases_close.len()..]);
        }
    }

    // Alignment markers
    s = s.replace("&=", " &= ");
    s = s.replace("&", " ");
    s = s.replace("\\\\", "\\ ");

    // \frac{a}{b} → (a)/(b)
    s = convert_frac(&s);

    // \text{...} → "..."
    s = convert_cmd_to(&s, "\\text{", "\"", "\"");
    // operatorname variants → quoted text (e.g. \operatorname{Center} → "Center")
    s = convert_cmd_to(&s, "\\operatorname*{", "\"", "\"");
    s = convert_cmd_to(&s, "\\operatorname{", "\"", "\"");
    s = convert_cmd_to(&s, "\\DeclareMathOperator{", "\"", "\"");
    // Font commands: quote multi-char word content to avoid "unknown variable" errors in Typst.
    // Single-char content keeps the function form (bold(x), upright(v), etc.) for correct math styling.
    s = convert_cmd_font(&s, "\\mathrm{", "upright(", ")");
    s = convert_cmd_font(&s, "\\mathbf{", "bold(", ")");
    s = convert_cmd_font(&s, "\\mathit{", "italic(", ")");
    s = convert_cmd_font(&s, "\\mathbb{", "bb(", ")");
    s = convert_cmd_font(&s, "\\mathcal{", "cal(", ")");
    s = convert_cmd_font(&s, "\\mathsf{", "sans(", ")");
    s = convert_cmd_font(&s, "\\mathfrak{", "frak(", ")");
    s = convert_cmd_font(&s, "\\boldsymbol{", "bold(", ")");
    s = convert_cmd_to(&s, "\\hat{", "hat(", ")");
    s = convert_cmd_to(&s, "\\tilde{", "tilde(", ")");
    s = convert_cmd_to(&s, "\\bar{", "overline(", ")");
    s = convert_cmd_to(&s, "\\vec{", "arrow(", ")");
    s = convert_cmd_to(&s, "\\dot{", "dot(", ")");
    s = convert_cmd_to(&s, "\\ddot{", "dot.double(", ")");
    s = convert_cmd_to(&s, "\\overline{", "overline(", ")");
    s = convert_cmd_to(&s, "\\underline{", "underline(", ")");
    s = convert_cmd_to(&s, "\\widehat{", "hat(", ")");
    s = convert_cmd_to(&s, "\\widetilde{", "tilde(", ")");
    s = convert_cmd_to(&s, "\\sqrt{", "sqrt(", ")");
    // \boxed{X} -> content (Typst math has no simple box; keep the equation, drop
    // the frame rather than failing). \xrightarrow{X} -> arrow with X above.
    s = convert_cmd_to(&s, "\\xrightarrow{", " ->^(", ") ");
    s = convert_cmd_to(&s, "\\xleftarrow{", " <-^(", ") ");
    s = convert_cmd_to(&s, "\\boxed{", "", "");

    // Variant Greek letters: Typst 0.11 has no `varepsilon`, `varphi`, ... so the
    // LaTeX \var* commands map to Typst's base or `.alt` symbol names. This MUST run
    // before the generic greek pass below, which would otherwise just strip the
    // backslash and leave Typst an unknown variable (e.g. `varepsilon`).
    let var_greeks: &[(&str, &str)] = &[
        ("\\varepsilon", "epsilon"), // Typst `epsilon` already renders the lunate form
        ("\\varphi", "phi"),
        ("\\vartheta", "theta.alt"),
        ("\\varrho", "rho.alt"),
        ("\\varsigma", "sigma.alt"),
        ("\\varpi", "pi.alt"),
        ("\\varkappa", "kappa.alt"),
    ];
    for (from, to) in var_greeks {
        s = s.replace(from, to);
    }

    // Greek letters (remove backslash - typst uses bare names)
    let greeks = [
        "alpha", "beta", "gamma", "delta", "epsilon",
        "zeta", "eta", "theta", "iota", "kappa", "lambda",
        "mu", "nu", "xi", "pi", "rho", "sigma",
        "tau", "upsilon", "phi", "chi", "psi", "omega",
        "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma",
        "Upsilon", "Phi", "Psi", "Omega",
    ];
    for g in &greeks {
        s = s.replace(&format!("\\{}", g), g);
    }

    // Operators and symbols
    let replacements: &[(&str, &str)] = &[
        ("\\sum", "sum"),
        ("\\prod", "product"),
        ("\\coprod", "product.co"),
        ("\\int", "integral"),
        ("\\oint", "integral.cont"),
        ("\\iint", "integral.double"),
        ("\\iiint", "integral.triple"),
        ("\\infty", "oo"),
        ("\\partial", "diff"),
        ("\\nabla", "nabla"),
        ("\\cdot", "dot.c"),
        ("\\times", "times"),
        ("\\div", "div"),
        ("\\pm", "plus.minus"),
        ("\\mp", "minus.plus"),
        ("\\rightarrow", "->"),
        ("\\to", "->"),
        ("\\leftarrow", "<-"),
        ("\\leftrightarrow", "<->"),
        ("\\Rightarrow", "=>"),
        ("\\Leftarrow", "<="),
        ("\\Leftrightarrow", "<=>"),
        ("\\mapsto", "|->"),
        ("\\leq", "<="),
        ("\\le", "<="),
        ("\\geq", ">="),
        ("\\ge", ">="),
        ("\\neq", "!="),
        ("\\ne", "!="),
        ("\\gg", " ≫ "),
        ("\\ll", " ≪ "),
        ("\\hbar", " planck.reduce "),
        ("\\iff", " <=> "),
        ("\\approx", "approx"),
        ("\\equiv", "equiv"),
        ("\\sim", "tilde.op"),
        ("\\simeq", "tilde.eq"),
        ("\\cong", "tilde.eq.rev"),
        ("\\propto", "prop"),
        ("\\in", "in"),
        ("\\notin", "in.not"),
        ("\\subset", "subset"),
        ("\\supset", "supset"),
        ("\\subseteq", "subset.eq"),
        ("\\supseteq", "supset.eq"),
        ("\\cup", "union"),
        ("\\cap", "sect"),
        ("\\setminus", "without"),
        ("\\emptyset", "nothing"),
        ("\\varnothing", "nothing"),
        ("\\forall", "forall"),
        ("\\exists", "exists"),
        ("\\nexists", "exists.not"),
        ("\\neg", "not"),
        ("\\wedge", "and"),
        ("\\vee", "or"),
        ("\\oplus", "xor"),
        ("\\otimes", "times.circle"),
        ("\\odot", "dot.circle"),
        ("\\circ", "compose"),
        ("\\bullet", "bullet"),
        ("\\perp", "bot"),
        ("\\parallel", "parallel"),
        ("\\angle", "angle"),
        ("\\langle", " lr(angle.l "),
        ("\\rangle", " angle.r)"),
        ("\\lfloor", " lr(floor.l "),
        ("\\rfloor", " floor.r)"),
        ("\\lceil", " lr(ceil.l "),
        ("\\rceil", " ceil.r)"),
        // Each \left opens exactly one Typst lr( call and each \right closes it,
        // so the delimiter char is emitted INSIDE lr(...). The closing forms must
        // therefore carry BOTH the right delimiter and the lr( terminator, else
        // nested \left(\left(...\right)\right) leaves lr( calls unclosed
        // ("expected closing paren"). Invisible \left./\right. and the bare forms
        // still open/close an lr( so mixed pairs (e.g. \left. ... \right|) balance.
        ("\\left(", " lr(("),
        ("\\right)", "))"),
        ("\\left[", " lr(["),
        ("\\right]", "])"),
        ("\\left\\{", " lr({"),
        ("\\right\\}", "})"),
        ("\\left|", " lr(|"),
        ("\\right|", "|)"),
        ("\\left.", " lr("),
        ("\\right.", ")"),
        ("\\left", " lr("),
        ("\\right", ")"),
        ("\\Big", ""),
        ("\\big", ""),
        ("\\bigg", ""),
        ("\\Bigg", ""),
        ("\\ldots", "..."),
        ("\\cdots", "dots.c"),
        ("\\vdots", "dots.v"),
        ("\\ddots", "dots.down"),
        ("\\quad", "quad"),
        ("\\qquad", "wide"),
        ("\\,", " "),
        ("\\;", " "),
        ("\\:", " "),
        ("\\!", ""),
        ("\\{", "{"),
        ("\\}", "}"),
        ("\\_", "_"),
        ("\\%", "%"),
        ("\\$", "$"),
        ("\\sqrt", "sqrt"),
    ];

    // Apply longest command first so a short command is never matched inside a
    // longer one (e.g. `\le` -> "<=" must not eat `\left(` -> "<=ft(" which made
    // Typst fail with "unknown variable: ft" on imported Word/OMML equations).
    let mut reps: Vec<(&str, &str)> = replacements.to_vec();
    reps.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (from, to) in reps {
        s = s.replace(from, to);
    }

    // Remove remaining unknown \commands (keep their name)
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let mut cmd = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphabetic() || c == '*' {
                    cmd.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            out.push_str(&cmd);
        } else {
            out.push(ch);
        }
    }

    // Convert LaTeX braces {...} to Typst parentheses (...) for grouping in math
    // Only naked braces (not already handled by operators)
    out = out.replace('{', "(").replace('}', ")");

    // Clean up excess whitespace
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }

    out.trim().to_string()
}

fn convert_frac(s: &str) -> String {
    let mut result = String::new();
    let mut remaining = s;

    while let Some(pos) = remaining.find("\\frac{") {
        result.push_str(&remaining[..pos]);
        remaining = &remaining[pos + 6..]; // skip \frac{

        if let Some((num, rest)) = extract_brace_content(remaining) {
            remaining = rest;
            if remaining.starts_with('{') {
                if let Some((den, rest2)) = extract_brace_content(&remaining[1..]) {
                    remaining = rest2;
                    let num_s = if num.len() == 1 { num.to_string() } else { format!("({})", num) };
                    let den_s = if den.len() == 1 { den.to_string() } else { format!("({})", den) };
                    result.push_str(&format!("{}/{}", num_s, den_s));
                    continue;
                }
            }
            result.push_str(&format!("frac({})", num));
        } else {
            result.push_str("frac(");
        }
    }
    result.push_str(remaining);
    result
}

/// Like `convert_cmd_to` but quotes the inner content when it looks like a
/// text word (multi-char, all letters) to avoid Typst "unknown variable" errors.
/// E.g. `\mathrm{Center}` → `upright("Center")` but `\mathrm{x}` → `upright(x)`.
fn convert_cmd_font(s: &str, cmd: &str, open: &str, close: &str) -> String {
    let mut result = String::new();
    let mut remaining = s;

    while let Some(pos) = remaining.find(cmd) {
        result.push_str(&remaining[..pos]);
        let after = &remaining[pos + cmd.len()..];
        if let Some((inner, rest)) = extract_brace_content(after) {
            result.push_str(open);
            if is_text_word(inner) {
                result.push('"');
                result.push_str(inner);
                result.push('"');
            } else {
                result.push_str(inner);
            }
            result.push_str(close);
            remaining = rest;
        } else {
            result.push_str(cmd);
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

/// Returns true if the string looks like a text word (multi-char, only letters/digits/spaces/hyphens)
/// rather than a math expression. Used to decide whether to quote it in Typst.
fn is_text_word(s: &str) -> bool {
    let s = s.trim();
    // Single char → treat as math variable
    if s.chars().count() <= 1 { return false; }
    // Contains math operators → treat as math expression
    if s.contains('_') || s.contains('^') || s.contains('+') || s.contains('/')
        || s.contains('=') || s.contains('(') || s.contains('\\') { return false; }
    // All chars are letters, digits, spaces or hyphens → text word
    s.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c.is_ascii_digit())
}

fn convert_cmd_to(s: &str, cmd: &str, open: &str, close: &str) -> String {
    let mut result = String::new();
    let mut remaining = s;

    while let Some(pos) = remaining.find(cmd) {
        result.push_str(&remaining[..pos]);
        let after = &remaining[pos + cmd.len()..];
        if let Some((inner, rest)) = extract_brace_content(after) {
            result.push_str(open);
            result.push_str(inner);
            result.push_str(close);
            remaining = rest;
        } else {
            result.push_str(cmd);
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

fn extract_brace_content(s: &str) -> Option<(&str, &str)> {
    let mut depth = 1i32;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod typst_identifier_tests {
    use super::{space_bare_identifiers, latex_to_typst_math};

    #[test]
    fn common_math_commands_are_converted() {
        // Regression: these standard LaTeX commands were missing from the Typst
        // table, so equations using them fell back to unicode in DOCX images.
        for cmd in [r"\boxed{x}", r"a \gg b", r"a \ll b", r"\hbar", r"p \iff q", r"\xrightarrow{f}"] {
            let t = latex_to_typst_math(cmd);
            for bad in [r"\boxed", r"\gg", r"\ll", r"\hbar", r"\iff", r"\xrightarrow"] {
                assert!(!t.contains(bad), "unconverted {bad} in {cmd:?} -> {t:?}");
            }
        }
    }

    #[test]
    fn var_greek_letters_compile_in_typst() {
        // Regression: \varepsilon (and the var* family) used to emit the bare token
        // `varepsilon`, which Typst 0.11 rejects ("unknown variable: varepsilon").
        // They must map to valid Typst names AND actually compile to an image.
        let eq = r"\varepsilon_0 + \varphi + \vartheta + \varrho + \varsigma + \varpi + \varkappa";
        let t = latex_to_typst_math(eq);
        for bad in ["varepsilon", "varphi", "vartheta", "varrho", "varsigma", "varpi", "varkappa"] {
            assert!(!t.contains(bad), "var* leaked into Typst output: {bad} in {t:?}");
        }
        let (png, err) = crate::equation_renderer::render_equation_png(eq, 2.0);
        assert!(err.is_none(), "Typst rejected the var greek letters: {err:?}");
        assert!(png.is_some(), "no PNG produced for the var greek letters");
    }

    #[test]
    fn expands_active_custom_macros_in_typst_math() {
        // A document's \newcommand macros must expand in the .typ output (and in
        // the equation images that share this function), not leak as unknown
        // Typst identifiers. Regression lock for the macro-heavy-paper fix.
        crate::latex_macros::install_from_source(r"\newcommand{\KK}{\mathcal{K}}");
        let t = latex_to_typst_math(r"\KK(t,s)");
        crate::latex_macros::install_from_source(""); // reset thread-local table
        assert!(!t.contains("KK"), "custom macro \\KK left unexpanded: {t:?}");
        assert!(t.contains("cal(K)"), "\\KK did not expand to cal(K): {t:?}");
    }

    #[test]
    fn spaces_bare_runs_but_protects_commands_and_text() {
        // Bare multi-letter run becomes a product of single-letter variables.
        assert_eq!(space_bare_identifiers("ERV"), "E R V");
        assert_eq!(space_bare_identifiers("R(VIF)"), "R(V I F)");
        // Command names stay intact, but a boundary space is inserted where a
        // letter abuts a command (R\left → R \left) so the converted tokens do
        // not jam into one unknown identifier (Rlr) at compile time.
        assert_eq!(space_bare_identifiers(r"R\left(VIF\right)"), r"R \left(V I F \right)");
        // \sin stays a function (it is a command, not a bare run).
        assert_eq!(space_bare_identifiers(r"\sin x"), r"\sin x");
        // Upright/text groups keep their content grouped.
        assert_eq!(space_bare_identifiers(r"\mathrm{ERV}"), r"\mathrm{ERV}");
        assert_eq!(space_bare_identifiers(r"\text{abc}"), r"\text{abc}");
        // Environment names survive (boundary space after the group is benign -
        // the env markers are stripped downstream).
        assert_eq!(
            space_bare_identifiers(r"\begin{aligned}AB\end{aligned}"),
            r"\begin{aligned} A B \end{aligned}"
        );
        // Single letters and symbol commands are left alone.
        assert_eq!(space_bare_identifiers("x + y"), "x + y");
        assert_eq!(space_bare_identifiers(r"a \\ b"), r"a \\ b");
    }
}
