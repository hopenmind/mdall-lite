use pulldown_cmark::{html, Options, Parser};
use std::path::Path;

pub fn markdown_to_html(markdown: &str) -> String {
    let (protected, placeholders) = extract_and_replace_latex(markdown);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(&protected, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    for (i, (latex, is_display)) in placeholders.iter().enumerate() {
        let rendered = render_katex(latex, *is_display);
        let placeholder = format!("KATEXPH{:04}ENDPH", i);
        html_output = html_output.replace(&placeholder, &rendered);
    }

    html_output
}

// ── LaTeX normalization: \\cmd → \cmd ──

// pub: shared with the UI-side equation layout builder (src/equation_layout.rs).
pub fn normalize_latex_escapes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if next == '\\' {
                    // \\cmd → \cmd (double backslash before LaTeX command)
                    chars.next();
                    if let Some(&after) = chars.peek() {
                        if after.is_alphabetic() || after == '{' || after == '}'
                            || after == '_' || after == '%' || after == '$'
                            || after == ',' || after == ';' || after == '!'
                            || after == '(' || after == ')' || after == '['
                            || after == ']' || after == '|' || after == '\\'
                        {
                            result.push('\\');
                        } else {
                            result.push(' ');
                        }
                    } else {
                        result.push(' ');
                    }
                } else if next == '{' || next == '}' || next == '[' || next == ']'
                    || next == '_' || next == '<' || next == '>' || next == '#'
                    || next == '*' || next == '~' || next == '`'
                {
                    // \{ → {, \< → <, \> → >, etc. (markdown escapes)
                    result.push(next);
                    chars.next();
                } else {
                    result.push('\\');
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

// ── Placeholder-based LaTeX extraction for HTML export ──

pub(crate) fn extract_and_replace_latex(markdown: &str) -> (String, Vec<(String, bool)>) {
    let mut result = String::new();
    let mut placeholders: Vec<(String, bool)> = Vec::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;
    let mut in_equation = false;
    let mut in_code_block = false;
    let mut equation_buf = String::new();

    while i < lines.len() {
        let line = lines[i];

        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            result.push_str(line);
            result.push('\n');
            i += 1;
            continue;
        }
        if in_code_block {
            result.push_str(line);
            result.push('\n');
            i += 1;
            continue;
        }

        // ── Display math: $$ or \[ ───────────────────────────────────────────
        let trimmed = line.trim_start();
        let is_dd_open  = trimmed.starts_with("$$");
        let is_lb_open  = trimmed.starts_with("\\[");

        if (is_dd_open || is_lb_open) && !in_equation {
            // Determine which closing marker to expect
            let close_marker = if is_dd_open { "$$" } else { "\\]" };
            let after = trimmed
                .trim_start_matches(if is_dd_open { "$$" } else { "\\[" })
                .trim();
            // Single-line form: $$ content $$ or \[ content \]
            if !after.is_empty() && after.ends_with(close_marker) {
                let content = after.trim_end_matches(close_marker).trim();
                let idx = placeholders.len();
                placeholders.push((normalize_latex_escapes(content), true));
                result.push_str(&format!("\n\nKATEXPH{:04}ENDPH\n\n", idx));
                i += 1;
                continue;
            }
            in_equation = true;
            equation_buf.clear();
            equation_buf.push_str("@@CLOSE@@");
            equation_buf.replace_range(..9, close_marker); // store close marker
            // The close marker fits in 2 chars; we just store it as the first 4 bytes
            // Simpler: use a second field. Reconstruct with a separator trick.
            // Actually: store close_marker then a NUL separator then content.
            equation_buf.clear();
            equation_buf.push_str(close_marker); // "$$" or "\\]"
            equation_buf.push('\x00');            // separator
            if !after.is_empty() {
                equation_buf.push_str(after);
            }
            i += 1;
            continue;
        }

        if in_equation {
            // Extract stored close marker and content
            let sep = equation_buf.find('\x00').unwrap_or(0);
            let close_marker = &equation_buf[..sep].to_string();
            let content_so_far = &equation_buf[sep + 1..].to_string();

            if line.trim() == close_marker || line.trim().ends_with(close_marker.as_str()) {
                let before = line.trim().trim_end_matches(close_marker.as_str()).trim();
                let mut full = content_so_far.clone();
                if !before.is_empty() {
                    if !full.is_empty() { full.push('\n'); }
                    full.push_str(before);
                }
                in_equation = false;
                let idx = placeholders.len();
                placeholders.push((normalize_latex_escapes(&full), true));
                result.push_str(&format!("\n\nKATEXPH{:04}ENDPH\n\n", idx));
                equation_buf.clear();
            } else {
                let mut full = content_so_far.clone();
                if !full.is_empty() { full.push('\n'); }
                full.push_str(line);
                equation_buf.clear();
                equation_buf.push_str(close_marker);
                equation_buf.push('\x00');
                equation_buf.push_str(&full);
            }
            i += 1;
            continue;
        }

        let processed = replace_inline_latex(line, &mut placeholders);
        result.push_str(&processed);
        result.push('\n');
        i += 1;
    }

    if in_equation && !equation_buf.is_empty() {
        let sep = equation_buf.find('\x00').unwrap_or(0);
        let content = &equation_buf[sep + 1..].to_string();
        let idx = placeholders.len();
        placeholders.push((normalize_latex_escapes(content), true));
        result.push_str(&format!("\n\nKATEXPH{:04}ENDPH\n\n", idx));
    }

    (result, placeholders)
}

fn replace_inline_latex(line: &str, placeholders: &mut Vec<(String, bool)>) -> String {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut result = String::new();
    let mut i = 0;

    while i < len {
        // Skip backtick code spans - no math inside code
        if bytes[i] == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' { i += 1; }
            if i < len { i += 1; }
            result.push_str(&line[start..i]);
            continue;
        }

        // \(...\) inline math
        if bytes[i] == b'\\' && i + 1 < len && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut j = start;
            let mut found = false;
            while j + 1 < len {
                if bytes[j] == b'\\' && bytes[j + 1] == b')' { found = true; break; }
                j += 1;
            }
            if found {
                let math = &line[start..j];
                let idx = placeholders.len();
                placeholders.push((normalize_latex_escapes(math), false));
                result.push_str(&format!("KATEXPH{:04}ENDPH", idx));
                i = j + 2;
            } else {
                result.push('\\'); result.push('(');
                i += 2;
            }
            continue;
        }

        // $...$ inline math
        if bytes[i] == b'$' {
            if i + 1 < len && bytes[i + 1] == b'$' {
                result.push('$'); result.push('$');
                i += 2;
                continue;
            }
            if i + 1 >= len { result.push('$'); i += 1; continue; }
            let next = bytes[i + 1];
            if next == b' ' || next == b'\n' || next == b'\t' {
                result.push('$'); i += 1; continue;
            }
            let start = i + 1;
            let mut j = start;
            let mut found = false;
            while j < len {
                if bytes[j] == b'$' && j > start { found = true; break; }
                j += 1;
            }
            if found {
                let math = &line[start..j];
                let idx = placeholders.len();
                placeholders.push((normalize_latex_escapes(math), false));
                result.push_str(&format!("KATEXPH{:04}ENDPH", idx));
                i = j + 1;
            } else {
                result.push('$'); i += 1;
            }
            continue;
        }

        let ch = line[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

// ── Preview: segmented rendering ──

#[derive(Clone)]
pub enum PreviewSegment {
    /// A chunk of Markdown text (processed for inline math display).
    /// `source_range` points to the corresponding raw bytes in the original source.
    Text { content: String, source_range: std::ops::Range<usize> },
    /// A display-math equation block.
    /// `source_range` covers the whole `$$...$$` (or `\[...\]`) span in the source.
    Equation { latex: String, index: usize, source_range: std::ops::Range<usize> },
}

pub fn split_into_segments(markdown: &str, base_dir: Option<&Path>) -> Vec<PreviewSegment> {
    let mut segments = Vec::new();

    // Collect lines with their byte positions.
    // Each entry: (line_content_str, byte_start, byte_end_including_newline)
    let mut lines_info: Vec<(String, usize, usize)> = Vec::new();
    {
        let mut pos = 0;
        let bytes = markdown.as_bytes();
        while pos < markdown.len() {
            let start = pos;
            while pos < markdown.len() && bytes[pos] != b'\n' { pos += 1; }
            let content = markdown[start..pos].trim_end_matches('\r').to_string();
            let next = if pos < markdown.len() { pos + 1 } else { pos };
            lines_info.push((content, start, next));
            pos = next;
        }
    }

    let mut i = 0;
    let mut eq_index = 0;
    let mut in_equation = false;
    let mut in_code_block = false;
    let mut equation_buf = String::new();
    let mut eq_byte_start: usize = 0;

    let mut text_buf = String::new();
    let mut text_buf_start: usize = 0;
    let mut text_buf_end: usize = 0;
    let mut text_buf_started = false;

    while i < lines_info.len() {
        let line = lines_info[i].0.as_str();
        let lbs   = lines_info[i].1; // line byte start
        let lbe   = lines_info[i].2; // line byte end (position after \n)

        // ── Code block toggle ──────────────────────────────────────────────
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            if !text_buf_started { text_buf_start = lbs; text_buf_started = true; }
            text_buf_end = lbe;
            text_buf.push_str(line);
            text_buf.push('\n');
            i += 1;
            continue;
        }
        if in_code_block {
            if !text_buf_started { text_buf_start = lbs; text_buf_started = true; }
            text_buf_end = lbe;
            text_buf.push_str(line);
            text_buf.push('\n');
            i += 1;
            continue;
        }

        // ── Display math: $$ or \[ ─────────────────────────────────────────
        let trimmed = line.trim_start();
        let is_dd_open = trimmed.starts_with("$$");
        let is_lb_open = trimmed.starts_with("\\[");

        if (is_dd_open || is_lb_open) && !in_equation {
            // Flush pending text segment
            if !text_buf.trim().is_empty() {
                segments.push(PreviewSegment::Text {
                    content: text_buf.clone(),
                    source_range: text_buf_start..text_buf_end,
                });
            }
            text_buf.clear();
            text_buf_started = false;

            let close_marker = if is_dd_open { "$$" } else { "\\]" };
            let after = trimmed
                .trim_start_matches(if is_dd_open { "$$" } else { "\\[" })
                .trim();

            // Single-line form: $$ content $$
            if !after.is_empty() && after.ends_with(close_marker) {
                let content = after.trim_end_matches(close_marker).trim();
                segments.push(PreviewSegment::Equation {
                    latex: content.to_string(),
                    index: eq_index,
                    source_range: lbs..lbe,
                });
                eq_index += 1;
                i += 1;
                continue;
            }

            in_equation = true;
            eq_byte_start = lbs;
            equation_buf.clear();
            equation_buf.push_str(close_marker);
            equation_buf.push('\x00');
            if !after.is_empty() { equation_buf.push_str(after); }
            i += 1;
            continue;
        }

        if in_equation {
            let sep = equation_buf.find('\x00').unwrap_or(0);
            let close_marker = equation_buf[..sep].to_string();
            let content_so_far = equation_buf[sep + 1..].to_string();

            if line.trim() == close_marker || line.trim().ends_with(close_marker.as_str()) {
                let before = line.trim().trim_end_matches(close_marker.as_str()).trim();
                let mut full = content_so_far;
                if !before.is_empty() {
                    if !full.is_empty() { full.push('\n'); }
                    full.push_str(before);
                }
                in_equation = false;
                segments.push(PreviewSegment::Equation {
                    latex: full,
                    index: eq_index,
                    source_range: eq_byte_start..lbe,
                });
                eq_index += 1;
                equation_buf.clear();
            } else {
                let mut full = content_so_far;
                if !full.is_empty() { full.push('\n'); }
                full.push_str(line.trim());
                equation_buf.clear();
                equation_buf.push_str(&close_marker);
                equation_buf.push('\x00');
                equation_buf.push_str(&full);
            }
            i += 1;
            continue;
        }

        // ── Normal text / inline math ──────────────────────────────────────
        let processed = preview_inline_math(line);
        let processed = resolve_image_paths_in_line(&processed, base_dir);
        if !text_buf_started { text_buf_start = lbs; text_buf_started = true; }
        text_buf_end = lbe;
        text_buf.push_str(&processed);
        text_buf.push('\n');
        i += 1;
    }

    // Flush unclosed equation (unterminated $$)
    if in_equation && !equation_buf.is_empty() {
        let sep = equation_buf.find('\x00').unwrap_or(0);
        let content = equation_buf[sep + 1..].to_string();
        segments.push(PreviewSegment::Equation {
            latex: content,
            index: eq_index,
            source_range: eq_byte_start..markdown.len(),
        });
    }
    // Flush remaining text
    if !text_buf.trim().is_empty() {
        segments.push(PreviewSegment::Text {
            content: text_buf,
            source_range: text_buf_start..text_buf_end,
        });
    }

    segments
}

/// Pre-process a markdown block for CommonMarkViewer:
/// - Convert inline `$...$` and `\(...\)` to Unicode approximations
/// - Resolve relative image paths to absolute `file://` URIs
pub fn preprocess_block_for_preview(block: &str, base_dir: Option<&Path>) -> String {
    let mut in_code = false;
    let mut result = String::new();
    for line in block.lines() {
        let t = line.trim();
        if t.starts_with("```") { in_code = !in_code; }
        let processed = if in_code {
            line.to_string()
        } else {
            let l = preview_inline_math(line);
            resolve_image_paths_in_line(&l, base_dir)
        };
        result.push_str(&processed);
        result.push('\n');
    }
    result
}

fn preview_inline_math(line: &str) -> String {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut result = String::new();
    let mut i = 0;

    while i < len {
        // \(...\) inline math → unicode approximation
        if bytes[i] == b'\\' && i + 1 < len && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut j = start;
            let mut found = false;
            while j + 1 < len {
                if bytes[j] == b'\\' && bytes[j + 1] == b')' { found = true; break; }
                j += 1;
            }
            if found {
                result.push_str(&latex_to_unicode(&line[start..j]));
                i = j + 2;
            } else {
                result.push('\\'); result.push('(');
                i += 2;
            }
            continue;
        }

        // $...$ inline math → unicode approximation
        if bytes[i] == b'$' {
            if i + 1 < len && bytes[i + 1] == b'$' {
                result.push('$'); result.push('$');
                i += 2;
                continue;
            }
            if i + 1 >= len { result.push('$'); i += 1; continue; }
            let next = bytes[i + 1];
            if next == b' ' || next == b'\n' || next == b'\t' {
                result.push('$'); i += 1; continue;
            }
            let start = i + 1;
            let mut j = start;
            let mut found = false;
            while j < len {
                if bytes[j] == b'$' && j > start { found = true; break; }
                j += 1;
            }
            if found {
                result.push_str(&latex_to_unicode(&line[start..j]));
                i = j + 1;
            } else {
                result.push('$'); i += 1;
            }
            continue;
        }

        let ch = line[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

// ── Image path resolution for preview ──

fn resolve_image_paths_in_line(line: &str, base_dir: Option<&Path>) -> String {
    let base = match base_dir {
        Some(d) => d,
        None => return line.to_string(),
    };

    let mut result = String::new();
    let mut remaining = line;

    while let Some(img_start) = remaining.find("![") {
        result.push_str(&remaining[..img_start]);
        remaining = &remaining[img_start..];

        if let Some(bracket_close) = remaining.find("](") {
            let alt = &remaining[2..bracket_close];
            let path_start = bracket_close + 2;
            if let Some(paren_close) = remaining[path_start..].find(')') {
                let raw_path = &remaining[path_start..path_start + paren_close];
                let resolved = resolve_path(raw_path, base);
                result.push_str(&format!("![{}]({})", alt, resolved));
                remaining = &remaining[path_start + paren_close + 1..];
            } else {
                result.push_str("![");
                remaining = &remaining[2..];
            }
        } else {
            result.push_str("![");
            remaining = &remaining[2..];
        }
    }
    result.push_str(remaining);
    result
}

fn resolve_path(path: &str, base_dir: &Path) -> String {
    if path.starts_with("http://") || path.starts_with("https://")
        || path.starts_with("file://") || path.starts_with("bytes://")
    {
        return path.to_string();
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return format!("file:///{}", path.replace('\\', "/"));
    }
    let resolved = base_dir.join(path);
    if resolved.exists() {
        format!("file:///{}", resolved.display().to_string().replace('\\', "/"))
    } else {
        path.to_string()
    }
}

// ── Unicode approximation ──

/// GREEKS + SYMBOLS as one list sorted by descending command length, so
/// `String::replace` always substitutes the longest matching command first and
/// never mangles one command inside a longer one. Built once and cached.
fn sorted_latex_replacements() -> &'static [(&'static str, &'static str)] {
    use std::sync::OnceLock;
    static CELL: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut v: Vec<(&'static str, &'static str)> =
            GREEKS.iter().chain(SYMBOLS.iter()).copied().collect();
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        v
    })
}

pub fn latex_to_unicode(latex: &str) -> String {
    // Expand the document's custom macros (\newcommand/\def) when a table is
    // installed for this thread (set by the UI each frame). No-op otherwise.
    let expanded = crate::latex_macros::expand_active(latex);
    let mut s = normalize_latex_escapes(&expanded);

    // Shared sanitization: drop \label/\tag/\nonumber, normalize spacing macros.
    s = crate::latex_macros::sanitize_latex(&s);

    // Normalize line breaks
    s = s.replace("\r\n", " ");
    s = s.replace('\n', " ");

    // Unescape LaTeX specials
    s = s.replace("\\{", "{");
    s = s.replace("\\}", "}");
    s = s.replace("\\_", "_");

    // Remove environment wrappers
    for env in &["align", "aligned", "equation", "gather", "gathered", "split",
                 "cases", "matrix", "pmatrix", "bmatrix", "vmatrix", "array"] {
        s = s.replace(&format!("\\begin{{{}}}", env), "");
        s = s.replace(&format!("\\end{{{}}}", env), "");
    }

    s = s.replace("&=", " = ");
    s = s.replace("&", " ");

    // Passthrough commands: \text{...} → inner text, etc.
    let passthrough_cmds = [
        "\\text{", "\\textbf{", "\\textit{", "\\textrm{", "\\texttt{",
        "\\mathrm{", "\\mathit{", "\\mathbf{", "\\mathbb{", "\\mathcal{",
        "\\mathsf{", "\\mathfrak{", "\\operatorname{", "\\boldsymbol{",
        "\\hat{", "\\tilde{", "\\bar{", "\\vec{", "\\dot{", "\\ddot{",
        "\\overline{", "\\underline{", "\\widehat{", "\\widetilde{",
        "\\bm{", "\\rm{", "\\color{",
    ];
    for cmd in &passthrough_cmds {
        loop {
            if let Some(pos) = s.find(cmd) {
                let cs = pos + cmd.len();
                if let Some(end) = find_brace_end(&s, cs) {
                    let inner = s[cs..end].to_string();
                    s = format!("{}{}{}", &s[..pos], inner, &s[end + 1..]);
                    continue;
                }
            }
            break;
        }
    }

    // \frac{a}{b} → (a)/(b)
    loop {
        if let Some(pos) = s.find("\\frac{") {
            let ns = pos + 6;
            if let Some(ne) = find_brace_end(&s, ns) {
                let num = s[ns..ne].to_string();
                let rest = &s[ne + 1..];
                if rest.starts_with('{') {
                    if let Some(de) = find_brace_end(rest, 1) {
                        let den = rest[1..de].to_string();
                        let num_s = if num.len() > 1 { format!("({})", num) } else { num };
                        let den_s = if den.len() > 1 { format!("({})", den) } else { den };
                        s = format!("{}{}/{}{}", &s[..pos], num_s, den_s, &rest[de + 1..]);
                        continue;
                    }
                }
            }
        }
        break;
    }

    // \sqrt[n]{x} → ⁿ√(x), \sqrt{x} → √(x)
    loop {
        if let Some(pos) = s.find("\\sqrt") {
            let after = &s[pos + 5..];
            if after.starts_with('[') {
                if let Some(bracket_end) = after.find(']') {
                    let n = &after[1..bracket_end];
                    let rest = &after[bracket_end + 1..];
                    if rest.starts_with('{') {
                        if let Some(end) = find_brace_end(rest, 1) {
                            let inner = rest[1..end].to_string();
                            let sup_n = to_superscript(n);
                            let prefix_len = pos + 5 + bracket_end + 1;
                            s = format!("{}{}√({}){}", &s[..pos], sup_n, inner, &s[prefix_len + end + 1..]);
                            continue;
                        }
                    }
                }
            } else if after.starts_with('{') {
                if let Some(end) = find_brace_end(after, 1) {
                    let inner = after[1..end].to_string();
                    s = format!("{}√({}){}", &s[..pos], inner, &after[end + 1..]);
                    continue;
                }
            }
        }
        break;
    }

    // Greek letters + math symbols, applied LONGEST COMMAND FIRST so a short
    // command is never matched inside a longer one (e.g. `\le` must not eat
    // `\left` -> "≤ft", `\in` must not eat `\int`). A naive in-table-order pass
    // mangled `\left(`/`\right)` from imported Word/OMML equations.
    for (cmd, uni) in sorted_latex_replacements() {
        s = s.replace(cmd, uni);
    }

    // Subscripts: _{...} - partial conversion: unrecognized chars kept as-is
    loop {
        if let Some(pos) = s.find("_{") {
            let cs = pos + 2;
            if let Some(end) = find_brace_end(&s, cs) {
                let inner = s[cs..end].to_string();
                let display = to_subscript(&inner);
                s = format!("{}{}{}", &s[..pos], display, &s[end + 1..]);
                continue;
            }
        }
        break;
    }

    // Superscripts: ^{...} - partial conversion: unrecognized chars kept as-is
    loop {
        if let Some(pos) = s.find("^{") {
            let cs = pos + 2;
            if let Some(end) = find_brace_end(&s, cs) {
                let inner = s[cs..end].to_string();
                let display = to_superscript(&inner);
                s = format!("{}{}{}", &s[..pos], display, &s[end + 1..]);
                continue;
            }
        }
        break;
    }

    // Single-char subscript/superscript - only when NOT followed by alphanumeric.
    // Prevents "_right" → "ᵣight" via the "_r" pattern.
    s = apply_isolated_replacements(s, SUB_CHARS);
    s = apply_isolated_replacements(s, SUP_CHARS);

    // Remove remaining \commands → keep name only
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let mut cmd = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphabetic() {
                    cmd.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if !cmd.is_empty() {
                out.push_str(&cmd);
            }
        } else {
            out.push(ch);
        }
    }

    // Clean up
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out = out.replace("{ ", "").replace(" }", "");
    out = out.replace("{}", "");
    out.trim().to_string()
}

// NOTE: `latex_to_layout_job` (LaTeX -> egui LayoutJob) moved to the binary
// (src/equation_layout.rs) so this core module stays 100% egui-free.
// It reuses the pub helpers below: normalize_latex_escapes, find_brace_end,
// GREEKS, SYMBOLS.

pub fn extract_equations(markdown: &str) -> Vec<String> {
    let mut equations = Vec::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;
    let mut in_equation = false;
    let mut in_code_block = false;
    let mut equation_buf = String::new();

    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            i += 1;
            continue;
        }
        if in_code_block {
            i += 1;
            continue;
        }
        // Recognise $$ and \[ as display math openers
        let trimmed = line.trim_start();
        let is_dd = trimmed.starts_with("$$");
        let is_lb = trimmed.starts_with("\\[");

        if (is_dd || is_lb) && !in_equation {
            let close_marker = if is_dd { "$$" } else { "\\]" };
            let after = trimmed
                .trim_start_matches(if is_dd { "$$" } else { "\\[" })
                .trim();
            if !after.is_empty() && after.ends_with(close_marker) {
                equations.push(after.trim_end_matches(close_marker).trim().to_string());
                i += 1;
                continue;
            }
            in_equation = true;
            equation_buf.clear();
            equation_buf.push_str(close_marker);
            equation_buf.push('\x00');
            if !after.is_empty() { equation_buf.push_str(after); }
            i += 1;
            continue;
        }
        if in_equation {
            let sep = equation_buf.find('\x00').unwrap_or(0);
            let close_marker = equation_buf[..sep].to_string();
            let content_so_far = equation_buf[sep + 1..].to_string();
            if line.trim() == close_marker || line.trim().ends_with(close_marker.as_str()) {
                let before = line.trim().trim_end_matches(close_marker.as_str()).trim();
                let mut full = content_so_far;
                if !before.is_empty() {
                    if !full.is_empty() { full.push('\n'); }
                    full.push_str(before);
                }
                in_equation = false;
                equations.push(full);
                equation_buf.clear();
            } else {
                let mut full = content_so_far;
                if !full.is_empty() { full.push('\n'); }
                full.push_str(line.trim());
                equation_buf.clear();
                equation_buf.push_str(&close_marker);
                equation_buf.push('\x00');
                equation_buf.push_str(&full);
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    if in_equation && !equation_buf.is_empty() {
        let sep = equation_buf.find('\x00').unwrap_or(0);
        equations.push(equation_buf[sep + 1..].to_string());
    }
    equations
}

// ── Internal helpers ──

pub fn find_brace_end(s: &str, start: usize) -> Option<usize> {
    let mut depth = 1i32;
    for (i, b) in s[start..].bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Apply single-char subscript/superscript replacements only when the matched
/// char is NOT followed by another alphanumeric character or underscore.
/// Example: "_r " → "ᵣ " but "_right" stays "_right" (would give "ᵣight").
fn apply_isolated_replacements(mut s: String, patterns: &[(&str, &str)]) -> String {
    for &(pat, uni) in patterns {
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(pos) = rest.find(pat) {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + pat.len()..];
            // Replace only when not followed by alphanumeric or '_'
            let isolated = after.chars().next()
                .map_or(true, |c| !c.is_alphanumeric() && c != '_');
            if isolated {
                out.push_str(uni);
            } else {
                out.push_str(pat);
            }
            rest = after;
        }
        out.push_str(rest);
        s = out;
    }
    s
}

#[allow(dead_code)] // unicode sub/superscript helpers, kept for the TXT exporter
fn can_subscript(c: char) -> bool {
    matches!(c,
        '0'..='9'
        | 'a' | 'e' | 'h' | 'i' | 'j' | 'k' | 'l' | 'm' | 'n'
        | 'o' | 'p' | 'r' | 's' | 't' | 'u' | 'v' | 'x'
        | '+' | '-' | '=' | '(' | ')'
    )
}

#[allow(dead_code)]
fn can_superscript(c: char) -> bool {
    matches!(c,
        '0'..='9'
        | 'a' | 'b' | 'c' | 'd' | 'e' | 'f' | 'g' | 'h' | 'i' | 'j'
        | 'k' | 'l' | 'm' | 'n' | 'o' | 'p' | 'r' | 's' | 't' | 'u'
        | 'v' | 'w' | 'x' | 'y' | 'z'
        | 'A' | 'B' | 'D' | 'E' | 'G' | 'H' | 'I' | 'J' | 'K' | 'L'
        | 'M' | 'N' | 'O' | 'P' | 'R' | 'T' | 'U' | 'V' | 'W'
        | '+' | '-' | '=' | '(' | ')' | '*'
    )
}

fn to_subscript(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄',
            '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
            'a' => 'ₐ', 'e' => 'ₑ', 'h' => 'ₕ', 'i' => 'ᵢ', 'j' => 'ⱼ',
            'k' => 'ₖ', 'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'o' => 'ₒ',
            'p' => 'ₚ', 'r' => 'ᵣ', 's' => 'ₛ', 't' => 'ₜ', 'u' => 'ᵤ',
            'v' => 'ᵥ', 'x' => 'ₓ',
            '+' => '₊', '-' => '₋', '=' => '₌',
            '(' => '₍', ')' => '₎',
            _ => c, // keep non-subscriptable chars as-is (partial conversion)
        })
        .collect()
}

fn to_superscript(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴',
            '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
            'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ', 'd' => 'ᵈ', 'e' => 'ᵉ',
            'f' => 'ᶠ', 'g' => 'ᵍ', 'h' => 'ʰ', 'i' => 'ⁱ', 'j' => 'ʲ',
            'k' => 'ᵏ', 'l' => 'ˡ', 'm' => 'ᵐ', 'n' => 'ⁿ', 'o' => 'ᵒ',
            'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ', 't' => 'ᵗ', 'u' => 'ᵘ',
            'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ',
            // Uppercase
            'A' => 'ᴬ', 'B' => 'ᴮ', 'D' => 'ᴰ', 'E' => 'ᴱ', 'G' => 'ᴳ',
            'H' => 'ᴴ', 'I' => 'ᴵ', 'J' => 'ᴶ', 'K' => 'ᴷ', 'L' => 'ᴸ',
            'M' => 'ᴹ', 'N' => 'ᴺ', 'O' => 'ᴼ', 'P' => 'ᴾ', 'R' => 'ᴿ',
            'T' => 'ᵀ', 'U' => 'ᵁ', 'V' => 'ⱽ', 'W' => 'ᵂ',
            '+' => '⁺', '-' => '⁻', '=' => '⁼',
            '(' => '⁽', ')' => '⁾', '*' => '⃰',
            _ => c, // keep non-superscriptable chars as-is (partial conversion)
        })
        .collect()
}

pub const GREEKS: &[(&str, &str)] = &[
    ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"),
    ("\\epsilon", "ε"), ("\\varepsilon", "ε"), ("\\zeta", "ζ"), ("\\eta", "η"),
    ("\\theta", "θ"), ("\\iota", "ι"), ("\\kappa", "κ"), ("\\lambda", "λ"),
    ("\\mu", "μ"), ("\\nu", "ν"), ("\\xi", "ξ"), ("\\pi", "π"),
    ("\\rho", "ρ"), ("\\sigma", "σ"), ("\\tau", "τ"), ("\\phi", "φ"),
    ("\\varphi", "φ"), ("\\chi", "χ"), ("\\psi", "ψ"), ("\\omega", "ω"),
    ("\\Gamma", "Γ"), ("\\Delta", "Δ"), ("\\Theta", "Θ"), ("\\Lambda", "Λ"),
    ("\\Sigma", "Σ"), ("\\Phi", "Φ"), ("\\Psi", "Ψ"), ("\\Omega", "Ω"),
    ("\\Pi", "Π"), ("\\Xi", "Ξ"),
];

pub const SYMBOLS: &[(&str, &str)] = &[
    ("\\rightarrow", "→"), ("\\leftarrow", "←"), ("\\leftrightarrow", "↔"),
    ("\\Rightarrow", "⇒"), ("\\Leftarrow", "⇐"), ("\\Leftrightarrow", "⇔"),
    ("\\cdot", "·"), ("\\times", "×"), ("\\odot", "⊙"), ("\\oplus", "⊕"),
    ("\\otimes", "⊗"), ("\\circ", "∘"), ("\\bullet", "•"),
    ("\\sum", "∑"), ("\\int", "∫"), ("\\prod", "∏"), ("\\coprod", "∐"),
    ("\\infty", "∞"), ("\\partial", "∂"), ("\\nabla", "∇"),
    ("\\forall", "∀"), ("\\exists", "∃"), ("\\nexists", "∄"),
    ("\\in", "∈"), ("\\notin", "∉"), ("\\subset", "⊂"), ("\\supset", "⊃"),
    ("\\subseteq", "⊆"), ("\\supseteq", "⊇"),
    ("\\cup", "∪"), ("\\cap", "∩"),
    ("\\approx", "≈"), ("\\neq", "≠"), ("\\equiv", "≡"),
    ("\\leq", "≤"), ("\\geq", "≥"), ("\\ll", "≪"), ("\\gg", "≫"),
    ("\\le", "≤"), ("\\ge", "≥"),
    ("\\parallel", "∥"), ("\\perp", "⊥"),
    ("\\pm", "±"), ("\\mp", "∓"),
    ("\\star", "⋆"), ("\\ast", "∗"),
    ("\\ldots", "..."), ("\\cdots", "⋯"), ("\\vdots", "⋮"), ("\\ddots", "⋱"),
    ("\\langle", "⟨"), ("\\rangle", "⟩"),
    ("\\hbar", "ℏ"), ("\\ell", "ℓ"), ("\\Re", "ℜ"), ("\\Im", "ℑ"),
    ("\\aleph", "ℵ"), ("\\wp", "℘"), ("\\emptyset", "∅"), ("\\varnothing", "∅"),
    ("\\mapsto", "↦"), ("\\to", "→"), ("\\gets", "←"),
    ("\\Longrightarrow", "⟹"), ("\\Longleftarrow", "⟸"),
    ("\\propto", "∝"), ("\\sim", "∼"), ("\\simeq", "≃"), ("\\cong", "≅"),
    ("\\angle", "∠"), ("\\triangle", "△"), ("\\square", "□"),
    ("\\dagger", "†"), ("\\ddagger", "‡"),
    ("\\lfloor", "⌊"), ("\\rfloor", "⌋"), ("\\lceil", "⌈"), ("\\rceil", "⌉"),
    ("\\%", "%"), ("\\$", "$"),
    ("\\quad", "  "), ("\\qquad", "    "),
    ("\\,", " "), ("\\;", " "), ("\\!", ""),
    ("\\left", ""), ("\\right", ""),
    ("\\Big", ""), ("\\big", ""), ("\\bigg", ""), ("\\Bigg", ""),
];

const SUB_CHARS: &[(&str, &str)] = &[
    ("_0", "₀"), ("_1", "₁"), ("_2", "₂"), ("_3", "₃"), ("_4", "₄"),
    ("_5", "₅"), ("_6", "₆"), ("_7", "₇"), ("_8", "₈"), ("_9", "₉"),
    ("_a", "ₐ"), ("_e", "ₑ"), ("_h", "ₕ"), ("_i", "ᵢ"), ("_j", "ⱼ"),
    ("_k", "ₖ"), ("_l", "ₗ"), ("_m", "ₘ"), ("_n", "ₙ"), ("_o", "ₒ"),
    ("_p", "ₚ"), ("_r", "ᵣ"), ("_s", "ₛ"), ("_t", "ₜ"), ("_u", "ᵤ"),
    ("_v", "ᵥ"), ("_x", "ₓ"),
];

const SUP_CHARS: &[(&str, &str)] = &[
    ("^0", "⁰"), ("^1", "¹"), ("^2", "²"), ("^3", "³"), ("^4", "⁴"),
    ("^5", "⁵"), ("^6", "⁶"), ("^7", "⁷"), ("^8", "⁸"), ("^9", "⁹"),
    ("^a", "ᵃ"), ("^b", "ᵇ"), ("^c", "ᶜ"), ("^d", "ᵈ"), ("^e", "ᵉ"),
    ("^f", "ᶠ"), ("^g", "ᵍ"), ("^h", "ʰ"), ("^i", "ⁱ"), ("^j", "ʲ"),
    ("^k", "ᵏ"), ("^l", "ˡ"), ("^m", "ᵐ"), ("^n", "ⁿ"), ("^o", "ᵒ"),
    ("^p", "ᵖ"), ("^r", "ʳ"), ("^s", "ˢ"), ("^t", "ᵗ"), ("^u", "ᵘ"),
    ("^v", "ᵛ"), ("^w", "ʷ"), ("^x", "ˣ"), ("^y", "ʸ"), ("^z", "ᶻ"),
    ("^A", "ᴬ"), ("^B", "ᴮ"), ("^D", "ᴰ"), ("^E", "ᴱ"), ("^G", "ᴳ"),
    ("^H", "ᴴ"), ("^I", "ᴵ"), ("^J", "ᴶ"), ("^K", "ᴷ"), ("^L", "ᴸ"),
    ("^M", "ᴹ"), ("^N", "ᴺ"), ("^O", "ᴼ"), ("^P", "ᴾ"), ("^R", "ᴿ"),
    ("^T", "ᵀ"), ("^U", "ᵁ"), ("^V", "ⱽ"), ("^W", "ᵂ"),
    ("^+", "⁺"), ("^-", "⁻"), ("^=", "⁼"), ("^(", "⁽"), ("^)", "⁾"),
    ("^*", "⃰"),
];

// ── KaTeX rendering for HTML/PDF export ──

fn render_katex(latex: &str, display_mode: bool) -> String {
    // Expand document-level custom macros (\newcommand/\def collected at import)
    // and strip non-math directives (\label/\tag/\ref/\cite) before handing the
    // LaTeX to KaTeX - otherwise KaTeX fails with "Undefined control sequence".
    let expanded = crate::latex_macros::expand_active(latex);
    let sanitized = crate::latex_macros::sanitize_latex(&expanded);
    let cleaned = sanitized.trim();
    if cleaned.is_empty() { return String::new(); }
    let opts = katex::Opts::builder()
        .display_mode(display_mode)
        .output_type(katex::OutputType::HtmlAndMathml)
        .build()
        .unwrap_or_default();
    match katex::render_with_opts(cleaned, opts) {
        Ok(rendered_html) => {
            if display_mode {
                format!("<div class=\"eq-block\">{}</div>", rendered_html)
            } else {
                format!("<span class=\"eq-inline\">{}</span>", rendered_html)
            }
        }
        Err(e) => {
            let class = if display_mode { "eq-error-block" } else { "eq-error-inline" };
            format!(
                "<span class=\"{}\"><code>{}</code><br><small>KaTeX error: {}</small></span>",
                class, html_escape(cleaned), html_escape(&e.to_string())
            )
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_right_not_mangled_by_le() {
        // Regression: `\le` used to eat `\left` -> "≤ft" on imported Word/OMML eqs.
        let out = latex_to_unicode("R\\left( x \\right)");
        assert!(!out.contains("ft"), "left/right mangled: {out:?}");
        assert!(!out.contains('≤'), "spurious ≤ from \\left: {out:?}");
        assert!(out.contains('(') && out.contains(')'), "delimiters lost: {out:?}");
        // The real symbol still works on its own.
        assert!(latex_to_unicode("a \\le b").contains('≤'));
        // And a longer command is not eaten by a shorter prefix.
        assert!(latex_to_unicode("\\int x").contains('∫'));
    }

    #[test]
    fn normalize_double_backslash_commands() {
        assert_eq!(normalize_latex_escapes("\\\\alpha"), "\\alpha");
        assert_eq!(normalize_latex_escapes("\\\\text{hi}"), "\\text{hi}");
        assert_eq!(normalize_latex_escapes("a \\\\ b"), "a   b");
        // \\\\ before _ : double-backslash escaping → keeps \_
        assert_eq!(
            normalize_latex_escapes("\\\\text{State}\\\\_i"),
            "\\text{State}\\_i"
        );
    }

    #[test]
    fn normalize_markdown_escapes() {
        assert_eq!(normalize_latex_escapes("\\{x\\}"), "{x}");
        assert_eq!(normalize_latex_escapes("\\[a\\]"), "[a]");
        assert_eq!(normalize_latex_escapes("\\_i"), "_i");
        assert_eq!(
            normalize_latex_escapes("\\\\text\\{Capture\\}\\_i = \\\\text\\{Tanh\\}(W\\_\\{\\\\text\\{cap\\}\\})"),
            "\\text{Capture}_i = \\text{Tanh}(W_{\\text{cap}})"
        );
    }

    #[test]
    fn katex_renders_simple() {
        let result = render_katex(r"\alpha + \beta", true);
        assert!(result.contains("eq-block"));
        assert!(result.contains("katex"));
        assert!(!result.contains("eq-error"));
    }

    #[test]
    fn katex_expands_active_custom_macros() {
        // Regression: imported .tex with `\newcommand{\tauc}{\tau_c}` used to hit
        // KaTeX "Undefined control sequence: \tauc" because render_katex skipped
        // macro expansion. It must now expand active macros before KaTeX.
        crate::latex_macros::install_from_source(
            "<!-- mdall:latex-macros -->\n\\newcommand{\\tauc}{\\tau_c}\n\\newcommand{\\sket}[1]{|#1\\rangle}\n",
        );
        let html = render_katex(r"\tauc \to 0 \quad \sket{\psi}", true);
        assert!(html.contains("katex"), "should render via KaTeX: {html}");
        assert!(!html.contains("eq-error"), "macro left unexpanded: {html}");
        assert!(!html.contains("Undefined control sequence"), "{html}");
    }

    #[test]
    fn pipeline_block_equation() {
        let md = "# Test\n\n$$\n\\alpha + \\beta\n$$\n\nDone.\n";
        let html = markdown_to_html(md);
        assert!(html.contains("eq-block"));
        assert!(html.contains("katex"));
    }

    #[test]
    fn pipeline_inline_equation() {
        let md = "Text with $\\alpha$ inline.\n";
        let html = markdown_to_html(md);
        assert!(html.contains("eq-inline"));
    }

    #[test]
    fn pipeline_double_backslash_equation() {
        let md = "$$\n\\\\text\\{State\\}\\\\_i = \\\\text\\{Tanh\\}(W\\\\_\\{\\\\text\\{fwd\\}\\})\n$$\n";
        let html = markdown_to_html(md);
        assert!(html.contains("eq-block"), "Must render block equation");
        assert!(!html.contains("eq-error"), "Must not error on double-escaped LaTeX: {}", html);
    }

    #[test]
    fn unicode_double_backslash() {
        let result = latex_to_unicode("\\\\text{State}\\\\_i = \\\\sigma(W)");
        assert!(result.contains("State"), "Must extract text content");
        assert!(result.contains("σ"), "Must convert sigma: got '{}'", result);
        assert!(!result.contains("text{"), "Must not show raw text{{ command: got '{}'", result);
    }

    #[test]
    fn normalize_triple_backslash_underscore() {
        // \\\ + _ in source: \\ → \, then \_ → _ in next pass → produces \_
        assert_eq!(
            normalize_latex_escapes("\\\\text{Center\\\\\\_State}"),
            "\\text{Center\\_State}"
        );
    }

    #[test]
    fn katex_center_state_equation() {
        let latex = normalize_latex_escapes(
            "\\\\text{Center\\\\\\_State} = (\\\\text{Feat}\\_\\{\\\\text{left}\\})"
        );
        assert_eq!(latex, "\\text{Center\\_State} = (\\text{Feat}_{\\text{left}})");
        let html = render_katex(&latex, true);
        assert!(html.contains("katex"), "Must render: {}", html);
        assert!(!html.contains("eq-error"), "Must not error: {}", html);
    }

    #[test]
    fn normalize_angle_brackets_and_percent() {
        assert_eq!(normalize_latex_escapes("\\<1\\\\%"), "<1\\%");
        assert_eq!(normalize_latex_escapes("\\>50\\\\%"), ">50\\%");
        let latex = normalize_latex_escapes("\\<1\\\\%");
        let html = render_katex(&latex, false);
        assert!(!html.contains("eq-error"), "Must render <1%: {}", html);
    }
}
