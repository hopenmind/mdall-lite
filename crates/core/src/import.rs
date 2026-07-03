// import.rs - Pure-Rust multi-format import pipeline.
//
// Each public function converts a source file / string to a Markdown String.
// Zero external tool dependencies - uses the `zip` crate + string processing.
//
// Quality level: "best-effort" - structure and text are preserved faithfully,
// but complex styling (custom fonts, floats, tracked changes) is ignored.

use std::path::Path;

// ═════════════════════════════════════════════════════════════════════════════
// HTML → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert an HTML string to Markdown.
///
/// Preserves: headings (h1-h6), bold, italic, code/pre, ordered/unordered
/// lists, blockquotes, horizontal rules, images.  Inline `$...$` / `\(...\)` /
/// `$$...$$` / `\[...\]` math is passed through unchanged.
pub fn html_to_md(html: &str) -> Result<String, String> {
    // Pre-process: extract math before general HTML stripping
    let html = preprocess_html_math(html);
    let mut md     = String::with_capacity(html.len());
    let html = html.as_str();
    let bytes      = html.as_bytes();
    let len        = bytes.len();
    let mut pos    = 0usize;

    // Visibility flags
    let mut in_head   = false;
    let mut in_script = false;
    let mut in_style  = false;
    let mut in_pre    = false;

    // Inline formatting
    let mut bold_depth   = 0i32;
    let mut italic_depth = 0i32;
    let mut code_depth   = 0i32;

    // Block state
    let mut list_stack: Vec<bool> = Vec::new(); // true = ordered
    let mut ord_counters: Vec<u64> = Vec::new();

    while pos < len {
        if bytes[pos] != b'<' {
            // Text node
            if !in_head && !in_script && !in_style {
                let start = pos;
                while pos < len && bytes[pos] != b'<' { pos += 1; }
                let raw  = &html[start..pos];
                let text = decode_entities(raw);
                if in_pre {
                    md.push_str(&text);
                } else {
                    // Collapse internal whitespace, trim edges
                    let collapsed = collapse_ws(&text);
                    if !collapsed.is_empty() {
                        md.push_str(&collapsed);
                    }
                }
            } else {
                while pos < len && bytes[pos] != b'<' { pos += 1; }
            }
            continue;
        }

        // Parse tag: <...>
        pos += 1; // skip <
        if pos >= len { break; }

        // Handle comments <!-- ... -->
        if html[pos..].starts_with("!--") {
            if let Some(end) = html[pos..].find("-->") {
                pos += end + 3;
            } else {
                pos = len;
            }
            continue;
        }

        let tag_content_start = pos;
        // Find closing >
        let mut in_quotes = false;
        let mut quote_char = b'"';
        while pos < len {
            if in_quotes {
                if bytes[pos] == quote_char { in_quotes = false; }
            } else if bytes[pos] == b'"' || bytes[pos] == b'\'' {
                in_quotes = true;
                quote_char = bytes[pos];
            } else if bytes[pos] == b'>' {
                break;
            }
            pos += 1;
        }
        let tag_inner = &html[tag_content_start..pos];
        if pos < len { pos += 1; } // skip >

        let closing  = tag_inner.starts_with('/');
        let self_closing = tag_inner.ends_with('/');
        let tag_body = if closing { tag_inner[1..].trim() } else { tag_inner.trim() };
        let tag_name = tag_body.split(|c: char| !c.is_alphanumeric())
                               .next().unwrap_or("").to_lowercase();

        match tag_name.as_str() {
            // ── Invisible sections ──────────────────────────────────────────
            "head"   => { in_head   = !closing; }
            "script" => { in_script = !closing; }
            "style"  => { in_style  = !closing; }

            // ── Headings ────────────────────────────────────────────────────
            "h1"|"h2"|"h3"|"h4"|"h5"|"h6" => {
                let level = (tag_name.as_bytes()[1] - b'0') as usize;
                if !closing {
                    ensure_double_newline(&mut md);
                    for _ in 0..level { md.push('#'); }
                    md.push(' ');
                } else {
                    md.push_str("\n\n");
                }
            }

            // ── Block elements ───────────────────────────────────────────────
            "p" | "div" | "article" | "section" | "main" | "header" | "footer" | "aside" => {
                if !closing && !self_closing {
                    ensure_double_newline(&mut md);
                } else if closing {
                    ensure_double_newline(&mut md);
                }
            }
            "br" => { md.push_str("  \n"); }
            "hr" => { ensure_double_newline(&mut md); md.push_str("---\n\n"); }

            // ── Inline formatting ────────────────────────────────────────────
            "strong" | "b" => {
                if !closing { bold_depth += 1; md.push_str("**"); }
                else if bold_depth > 0 { bold_depth -= 1; md.push_str("**"); }
            }
            "em" | "i" => {
                if !closing { italic_depth += 1; md.push('*'); }
                else if italic_depth > 0 { italic_depth -= 1; md.push('*'); }
            }
            "u" => {} // underline: no MD equivalent, skip markers
            "s" | "del" | "strike" => {
                if !closing { md.push_str("~~"); } else { md.push_str("~~"); }
            }
            "sup" => {
                if !closing { md.push_str("<sup>"); } else { md.push_str("</sup>"); }
            }
            "sub" => {
                if !closing { md.push_str("<sub>"); } else { md.push_str("</sub>"); }
            }

            // ── Code ─────────────────────────────────────────────────────────
            "code" => {
                if !closing {
                    code_depth += 1;
                    if !in_pre { md.push('`'); }
                } else if code_depth > 0 {
                    code_depth -= 1;
                    if !in_pre { md.push('`'); }
                }
            }
            "pre" => {
                if !closing {
                    in_pre = true;
                    // Try to extract language from class="language-xxx"
                    let lang = extract_attr(tag_inner, "class")
                        .and_then(|c| c.strip_prefix("language-").map(str::to_string))
                        .unwrap_or_default();
                    ensure_double_newline(&mut md);
                    md.push_str("```");
                    md.push_str(&lang);
                    md.push('\n');
                } else {
                    in_pre = false;
                    if !md.ends_with('\n') { md.push('\n'); }
                    md.push_str("```\n\n");
                }
            }

            // ── Lists ─────────────────────────────────────────────────────────
            "ul" => {
                if !closing {
                    list_stack.push(false);
                    ord_counters.push(0);
                    ensure_newline(&mut md);
                } else {
                    list_stack.pop();
                    ord_counters.pop();
                    md.push('\n');
                }
            }
            "ol" => {
                if !closing {
                    list_stack.push(true);
                    ord_counters.push(0);
                    ensure_newline(&mut md);
                } else {
                    list_stack.pop();
                    ord_counters.pop();
                    md.push('\n');
                }
            }
            "li" => {
                if !closing {
                    let depth  = list_stack.len().saturating_sub(1);
                    let indent = "  ".repeat(depth);
                    let is_ord = list_stack.last().copied().unwrap_or(false);
                    if is_ord {
                        if let Some(n) = ord_counters.last_mut() { *n += 1; }
                        let n = ord_counters.last().copied().unwrap_or(1);
                        md.push_str(&format!("{}{}. ", indent, n));
                    } else {
                        md.push_str(&format!("{}- ", indent));
                    }
                } else {
                    ensure_newline(&mut md);
                }
            }

            // ── Blockquote ────────────────────────────────────────────────────
            "blockquote" => {
                if !closing {
                    ensure_double_newline(&mut md);
                    md.push_str("> ");
                } else {
                    md.push_str("\n\n");
                }
            }

            // ── Links ─────────────────────────────────────────────────────────
            "a" => {
                // Simplified: emit link text only (href reconstruction requires
                // lookahead state machine - out of scope for best-effort import)
            }

            // ── Images ────────────────────────────────────────────────────────
            "img" => {
                let src = extract_attr(tag_inner, "src").unwrap_or_default();
                let alt = extract_attr(tag_inner, "alt").unwrap_or_default();
                md.push_str(&format!("![{}]({})", alt, src));
            }

            // ── Table (basic: emit pipe-separated text) ───────────────────────
            "table" => {}
            "tr" => {
                if !closing { md.push('\n'); } else { md.push_str(" |"); }
            }
            "th" | "td" => {
                if !closing { md.push_str("| "); }
            }

            // ── Ignore everything else ────────────────────────────────────────
            _ => {}
        }
    }

    Ok(collapse_blank_lines(&fix_gfm_tables(&md)))
}

/// Insert the GFM delimiter row (`| --- | --- |`) after the header of each pipe
/// table that lacks one. The streaming HTML table handler emits header and body
/// rows but no delimiter, so without this the output is not valid GFM and renders
/// as plain text instead of a table.
fn fix_gfm_tables(md: &str) -> String {
    let is_row = |l: &str| l.trim_start().starts_with('|');
    let is_delim = |l: &str| {
        let t = l.trim();
        t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
            && t.contains('-')
    };
    let lines: Vec<&str> = md.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Start of a table block: a row whose previous emitted line is not a row.
        let prev_is_row = out.last().map(|l| is_row(l)).unwrap_or(false);
        if is_row(line) && !prev_is_row {
            out.push(line.to_string());
            // If the next line is not already a delimiter, synthesize one.
            let next_is_delim = lines.get(i + 1).map(|l| is_delim(l)).unwrap_or(false);
            if !next_is_delim {
                let cols = line.split('|').filter(|c| !c.trim().is_empty()).count().max(1);
                let delim = format!("|{}", " --- |".repeat(cols));
                out.push(delim);
            }
            i += 1;
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

// ── HTML math pre-processing ─────────────────────────────────────────────────

/// Replace math blocks in HTML with plain `$...$` / `$$...$$` before tag stripping.
///
/// Handles (in priority order):
///   1. KaTeX/MathJax3 `<annotation encoding="application/x-tex">` - exact LaTeX source
///   2. MathJax2 `<script type="math/tex">` / `type="math/tex; mode=display"`
///   3. Pandoc `<span class="math inline">` / `class="math display"`
///   4. Generic `data-latex` / `data-tex` / `data-formula` attributes
fn preprocess_html_math(html: &str) -> String {
    let _out  = String::with_capacity(html.len());
    let rest = html;

    // ── Pass 1: replace <math>...</math> blocks (KaTeX/MathML) ──────────────
    // Extract <annotation encoding="application/x-tex">LATEX</annotation>
    // and drop the entire surrounding <math>...</math> block.
    let mut buf = String::with_capacity(rest.len());
    let mut scan = rest;
    while let Some(p) = scan.find("<math") {
        buf.push_str(&scan[..p]);
        scan = &scan[p..];
        // Find end of <math...>
        if let Some(open_end) = scan.find('>') {
            let math_open = &scan[..open_end+1];
            scan = &scan[open_end+1..];
            // Find matching </math>
            if let Some(close) = scan.find("</math>") {
                let math_body = &scan[..close];
                scan = &scan[close+7..];
                // Try to extract LaTeX annotation
                let latex_opt = extract_math_annotation(math_body);
                // Determine display vs inline from opening tag
                let is_display = math_open.contains("display") || math_open.contains("block");
                if let Some(latex) = latex_opt {
                    if is_display {
                        buf.push_str(&format!("\n$$\n{}\n$$\n", latex));
                    } else {
                        buf.push_str(&format!("${}$", latex));
                    }
                } else {
                    // No annotation - fall back to extracting <mi><mn><mo> text
                    let text = extract_mathml_text(math_body);
                    if !text.trim().is_empty() {
                        if is_display { buf.push_str(&format!("\n$$\n{}\n$$\n", text.trim())); }
                        else { buf.push_str(&format!("${}$", text.trim())); }
                    }
                }
            } else {
                buf.push_str(math_open);
            }
        } else {
            buf.push_str(scan);
            scan = "";
        }
    }
    buf.push_str(scan);
    let html = buf;

    // ── Pass 2: MathJax2 <script type="math/tex..."> ────────────────────────
    let mut buf2 = String::with_capacity(html.len());
    let mut scan = html.as_str();
    while let Some(p) = scan.find("<script") {
        buf2.push_str(&scan[..p]);
        scan = &scan[p..];
        let tag_end = scan.find('>').unwrap_or(scan.len());
        let tag = &scan[..tag_end+1];
        let is_math   = tag.contains("math/tex");
        let is_display = tag.contains("mode=display") || tag.contains("mode%3Ddisplay");
        scan = &scan[tag_end+1..];
        if is_math {
            if let Some(close) = scan.find("</script>") {
                let content = scan[..close].trim();
                if is_display {
                    buf2.push_str(&format!("\n$$\n{}\n$$\n", content));
                } else {
                    buf2.push_str(&format!("${}$", content));
                }
                scan = &scan[close+9..];
            }
        } else {
            buf2.push_str(tag);
        }
    }
    buf2.push_str(scan);
    let html = buf2;

    // ── Pass 3: Pandoc <span class="math inline/display">...</span> ──────────
    let mut buf3 = String::with_capacity(html.len());
    let mut scan = html.as_str();
    while let Some(p) = scan.find("<span") {
        buf3.push_str(&scan[..p]);
        scan = &scan[p..];
        let tag_end = scan.find('>').unwrap_or(scan.len());
        let tag = &scan[..tag_end+1];
        let tl = tag.to_lowercase();
        let is_inline  = tl.contains("math inline") || tl.contains("math-inline");
        let is_display = tl.contains("math display") || tl.contains("math-display");
        if is_inline || is_display {
            scan = &scan[tag_end+1..];
            if let Some(close) = scan.find("</span>") {
                let content = scan[..close].trim();
                // Content from Pandoc is already \(...\) or \[...\] - pass through
                if is_display { buf3.push_str(&format!("\n{}\n", content)); }
                else { buf3.push_str(content); }
                scan = &scan[close+7..];
            } else {
                buf3.push_str(tag);
            }
        } else {
            buf3.push_str(tag);
            scan = &scan[tag_end+1..];
        }
    }
    buf3.push_str(scan);
    buf3
}

fn extract_math_annotation(math_body: &str) -> Option<String> {
    let open = "<annotation encoding=\"application/x-tex\">";
    let open2 = "<annotation encoding='application/x-tex'>";
    let start = math_body.find(open).map(|p| (p, open.len()))
        .or_else(|| math_body.find(open2).map(|p| (p, open2.len())));
    if let Some((p, len)) = start {
        let after = &math_body[p+len..];
        if let Some(end) = after.find("</annotation>") {
            return Some(after[..end].trim().to_string());
        }
    }
    None
}

fn extract_mathml_text(xml: &str) -> String {
    // Extract text content from MathML presentation elements mi, mn, mo, mtext
    let mut text = String::new();
    let leaves = ["<mi>", "<mn>", "<mo>", "<mtext>"];
    let closes = ["</mi>", "</mn>", "</mo>", "</mtext>"];
    let mut pos = 0;
    while pos < xml.len() {
        let mut found = false;
        for (open, close) in leaves.iter().zip(closes.iter()) {
            if xml[pos..].starts_with(open) {
                let after = &xml[pos+open.len()..];
                if let Some(e) = after.find(close) {
                    text.push_str(&after[..e]);
                    pos += open.len() + e + close.len();
                    found = true;
                    break;
                }
            }
        }
        if !found { pos += 1; }
    }
    text
}

// ── HTML helpers ──────────────────────────────────────────────────────────────

fn decode_entities(s: &str) -> String {
    s.replace("&amp;",   "&")
     .replace("&lt;",    "<")
     .replace("&gt;",    ">")
     .replace("&quot;",  "\"")
     .replace("&#39;",   "'")
     .replace("&apos;",  "'")
     .replace("&nbsp;",  " ")
     .replace("&mdash;", "-")
     .replace("&ndash;", "-")
     .replace("&hellip;","...")
     .replace("&copy;",  "©")
     .replace("&reg;",   "®")
     .replace("&trade;", "™")
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            if !last_space { out.push(' '); last_space = true; }
        } else {
            last_space = false;
            out.push(c);
        }
    }
    out
}

fn ensure_newline(md: &mut String) {
    if !md.ends_with('\n') { md.push('\n'); }
}

fn ensure_double_newline(md: &mut String) {
    if md.ends_with("\n\n") || md.is_empty() { return; }
    if md.ends_with('\n') { md.push('\n'); } else { md.push_str("\n\n"); }
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    for pat in [format!("{}=\"", attr), format!("{}='", attr)] {
        if let Some(p) = tag.to_lowercase().find(&pat.to_lowercase()) {
            let start = p + pat.len();
            let close = if pat.ends_with('"') { '"' } else { '\'' };
            if let Some(end) = tag[start..].find(close) {
                return Some(tag[start..start+end].to_string());
            }
        }
    }
    // Also handle unquoted value (href=foo)
    let bare = format!("{}=", attr);
    if let Some(p) = tag.to_lowercase().find(&bare.to_lowercase()) {
        let start = p + bare.len();
        let end = tag[start..].find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                              .unwrap_or(tag.len() - start);
        return Some(tag[start..start+end].to_string());
    }
    None
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out   = String::with_capacity(s.len());
    let mut blank = 0u32;
    for line in s.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank <= 2 { out.push('\n'); }
        } else {
            blank = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    let result = out.trim_start_matches('\n').trim_end_matches('\n');
    format!("{}\n", result)
}

fn extract_xml_attr(xml: &str, attr: &str) -> Option<String> {
    for pat in [format!("{}=\"", attr), format!("{}='", attr)] {
        if let Some(p) = xml.find(&pat) {
            let start = p + pat.len();
            let close = if pat.ends_with('"') { '"' } else { '\'' };
            if let Some(end) = xml[start..].find(close) {
                return Some(xml[start..start+end].to_string());
            }
        }
    }
    None
}

// ═════════════════════════════════════════════════════════════════════════════
// EPUB → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert an EPUB file to Markdown.
///
/// Unpacks the ZIP, reads the OPF manifest + spine, converts each XHTML
/// chapter via `html_to_md`, and joins them with horizontal rules.
pub fn epub_to_md(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let file    = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("EPUB ZIP error: {}", e))?;

    // 1. Read META-INF/container.xml → find OPF path
    let opf_path = {
        let mut s = String::new();
        zip.by_name("META-INF/container.xml")
            .map_err(|_| "EPUB: META-INF/container.xml missing".to_string())?
            .read_to_string(&mut s)
            .map_err(|e| e.to_string())?;
        extract_xml_attr(&s, "full-path")
            .ok_or_else(|| "EPUB: could not find OPF path in container.xml".to_string())?
    };

    // Base directory of the OPF file (for resolving relative hrefs)
    let opf_dir = std::path::Path::new(&opf_path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();

    // 2. Read OPF → manifest + spine
    let opf = {
        let mut s = String::new();
        zip.by_name(&opf_path)
            .map_err(|e| format!("EPUB OPF not found: {}", e))?
            .read_to_string(&mut s)
            .map_err(|e| e.to_string())?;
        s
    };

    let manifest = epub_manifest(&opf);  // id → href
    let spine    = epub_spine(&opf);     // ordered idrefs

    // 3. Convert chapters in spine order
    let mut parts: Vec<String> = Vec::new();
    for idref in &spine {
        let href = match manifest.get(idref.as_str()) {
            Some(h) => h,
            None    => continue,
        };
        let chapter_path = if opf_dir.is_empty() {
            href.clone()
        } else {
            normalize_path(&format!("{}/{}", opf_dir, href))
        };
        let html = match zip.by_name(&chapter_path) {
            Ok(mut e) => {
                let mut s = String::new();
                e.read_to_string(&mut s).map_err(|e| e.to_string())?;
                s
            }
            Err(_) => continue,
        };
        if let Ok(md) = html_to_md(&html) {
            let trimmed = md.trim().to_string();
            if !trimmed.is_empty() {
                parts.push(trimmed);
            }
        }
    }

    if parts.is_empty() {
        return Err("EPUB: no readable HTML chapters found".into());
    }
    Ok(parts.join("\n\n---\n\n") + "\n")
}

fn epub_manifest(opf: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut pos = 0;
    while let Some(p) = opf[pos..].find("<item ") {
        let abs   = pos + p;
        let end   = opf[abs..].find('>').map(|e| abs+e+1).unwrap_or(opf.len());
        let item  = &opf[abs..end];
        if let (Some(id), Some(href)) = (extract_xml_attr(item, "id"), extract_xml_attr(item, "href")) {
            // Only add HTML/XHTML items
            let mt = extract_xml_attr(item, "media-type").unwrap_or_default();
            if mt.contains("html") || href.ends_with(".xhtml") || href.ends_with(".html") || href.ends_with(".htm") {
                map.insert(id, href);
            }
        }
        pos = end;
    }
    map
}

fn epub_spine(opf: &str) -> Vec<String> {
    let mut spine = Vec::new();
    let start = match opf.find("<spine") { Some(p) => p, None => return spine };
    let end   = opf[start..].find("</spine>").map(|e| start+e).unwrap_or(opf.len());
    let body  = &opf[start..end];
    let mut pos = 0;
    while let Some(p) = body[pos..].find("<itemref ") {
        let abs = pos + p;
        let e   = body[abs..].find('>').map(|x| abs+x+1).unwrap_or(body.len());
        if let Some(idref) = extract_xml_attr(&body[abs..e], "idref") {
            spine.push(idref);
        }
        pos = e;
    }
    spine
}

fn normalize_path(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg { ".." => { parts.pop(); } "." | "" => {} s => parts.push(s) }
    }
    parts.join("/")
}

// ═════════════════════════════════════════════════════════════════════════════
// ODT → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert an ODT (OpenDocument Text) file to Markdown.
///
/// Reads `content.xml` from the ZIP, maps `text:h`, `text:p`, `text:span`
/// and list elements to Markdown equivalents.
pub fn odt_to_md(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let file  = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(file)
        .map_err(|e| format!("ODT ZIP error: {}", e))?;
    let mut xml = String::new();
    z.by_name("content.xml")
        .map_err(|_| "ODT: content.xml not found".to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;

    // Primary: quick-xml parser with real style-resolved bold/italic + tables.
    // Fallback: legacy scanner (never blank the user's document).
    match crate::import_xml::odt_content_to_md(&xml) {
        Ok(md) if !md.trim().is_empty() => Ok(md),
        _ => odt_xml_to_md(&xml),
    }
}

fn odt_xml_to_md(xml: &str) -> Result<String, String> {
    let mut md    = String::new();
    let bytes     = xml.as_bytes();
    let len       = bytes.len();
    let mut pos   = 0usize;
    let mut bold   = false;
    let mut italic = false;
    let mut in_text_block = false; // inside text:p or text:h
    let mut list_depth: u32 = 0;

    while pos < len {
        if bytes[pos] != b'<' {
            if in_text_block {
                let start = pos;
                while pos < len && bytes[pos] != b'<' { pos += 1; }
                let text = decode_entities(&xml[start..pos]);
                md.push_str(&text);
            } else {
                while pos < len && bytes[pos] != b'<' { pos += 1; }
            }
            continue;
        }
        pos += 1;
        let ts = pos;
        while pos < len && bytes[pos] != b'>' { pos += 1; }
        let tag = &xml[ts..pos];
        if pos < len { pos += 1; }

        let closing  = tag.starts_with('/');
        let tag_body = if closing { tag[1..].trim() } else { tag.trim() };
        let tag_name = tag_body.split(|c: char| !c.is_alphanumeric() && c != ':')
                               .next().unwrap_or("").to_string();

        match tag_name.as_str() {
            "text:h" => {
                if !closing {
                    let level = extract_xml_attr(tag, "text:outline-level")
                        .and_then(|s| s.parse::<u8>().ok())
                        .unwrap_or(1)
                        .clamp(1, 6);
                    ensure_double_newline(&mut md);
                    for _ in 0..level { md.push('#'); }
                    md.push(' ');
                    in_text_block = true;
                } else {
                    md.push_str("\n\n");
                    in_text_block = false;
                }
            }
            "text:p" => {
                if !closing {
                    if list_depth == 0 { ensure_double_newline(&mut md); }
                    in_text_block = true;
                } else {
                    if list_depth == 0 { md.push_str("\n\n"); }
                    else { md.push('\n'); }
                    in_text_block = false;
                }
            }
            "text:span" => {
                let style = extract_xml_attr(tag, "text:style-name")
                    .unwrap_or_default().to_lowercase();
                if !closing {
                    if style.contains("bold") || style.contains("strong") {
                        bold = true; md.push_str("**");
                    } else if style.contains("italic") || style.contains("emph") {
                        italic = true; md.push('*');
                    }
                } else {
                    if bold   { bold = false;   md.push_str("**"); }
                    if italic { italic = false;  md.push('*'); }
                }
            }
            "text:list" => {
                if !closing { list_depth += 1; ensure_newline(&mut md); }
                else { list_depth = list_depth.saturating_sub(1); md.push('\n'); }
            }
            "text:list-item" => {
                if !closing {
                    let indent = "  ".repeat(list_depth.saturating_sub(1) as usize);
                    md.push_str(&format!("{}- ", indent));
                }
            }
            "text:s" => { md.push(' '); }
            "text:tab" => { md.push('\t'); }
            "text:line-break" => { md.push_str("  \n"); }
            _ => {}
        }
    }

    Ok(collapse_blank_lines(&md))
}

// ═════════════════════════════════════════════════════════════════════════════
// DOCX generic → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert a generic DOCX (not an MD -> ALL export) to Markdown.
///
/// Reads `word/document.xml`, walks `<w:p>` paragraphs, maps paragraph
/// styles (Heading1-6, Title) to `#` headers and extracts run text with
/// basic bold/italic detection.
pub fn docx_generic_to_md(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let file  = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(file)
        .map_err(|e| format!("DOCX ZIP error: {}", e))?;
    let mut xml = String::new();
    z.by_name("word/document.xml")
        .map_err(|_| "DOCX: word/document.xml not found".to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;

    // Hyperlink targets (optional).
    let mut rels_xml = String::new();
    if let Ok(mut r) = z.by_name("word/_rels/document.xml.rels") {
        let _ = r.read_to_string(&mut rels_xml);
    }
    let rels = crate::import_xml::parse_docx_rels(&rels_xml);

    // Primary: namespace-aware quick-xml parser (tables, hyperlinks, lists,
    // run formatting, OMML). Fallback: legacy string scan - never hand the user
    // a blank document if the new parser hits an edge case.
    match crate::import_xml::docx_document_to_md(&xml, &rels) {
        Ok(md) if !md.trim().is_empty() => Ok(md),
        _ => docx_xml_to_md(&xml),
    }
}

pub(crate) fn docx_xml_to_md(xml: &str) -> Result<String, String> {
    let mut md  = String::new();
    let mut pos = 0;

    // Process paragraph by paragraph
    while let Some(p_rel) = xml[pos..].find("<w:p>").or_else(|| xml[pos..].find("<w:p ")) {
        let abs_start = pos + p_rel;
        // Verify it is actually a <w:p> or <w:p ...> (not <w:pPr> etc.)
        let ch = xml[abs_start + 4..].chars().next().unwrap_or('X');
        if ch != '>' && ch != ' ' && ch != '\n' && ch != '\r' && ch != '\t' {
            pos = abs_start + 4;
            continue;
        }
        let abs_end = match xml[abs_start..].find("</w:p>") {
            Some(e) => abs_start + e + 6,
            None    => { pos = abs_start + 4; continue; }
        };
        let para = &xml[abs_start..abs_end];

        // ── Style ────────────────────────────────────────────────────────────
        let style = docx_pstyle(para);
        let heading: Option<u8> = {
            let s = style.to_lowercase();
            if s == "title" { Some(1) }
            else if s == "subtitle" { None }
            else if s.starts_with("heading") {
                s.chars().last().and_then(|c| c.to_digit(10)).map(|n| (n as u8).clamp(1, 6))
            } else { None }
        };

        // ── Is list item? ────────────────────────────────────────────────────
        let is_list = para.contains("<w:numPr>");

        // ── Extract text from all <w:r> runs ─────────────────────────────────
        let text = docx_extract_runs(para);
        if text.trim().is_empty() {
            if !para.contains("<w:numPr>") { md.push('\n'); }
            pos = abs_end;
            continue;
        }

        if let Some(level) = heading {
            ensure_double_newline(&mut md);
            for _ in 0..level { md.push('#'); }
            md.push(' ');
            md.push_str(text.trim());
            md.push_str("\n\n");
        } else if is_list {
            ensure_newline(&mut md);
            md.push_str("- ");
            md.push_str(text.trim());
            md.push('\n');
        } else {
            let t = text.trim();
            if !t.is_empty() {
                md.push_str(t);
                md.push_str("\n\n");
            }
        }
        pos = abs_end;
    }

    Ok(collapse_blank_lines(&md))
}

fn docx_pstyle(para: &str) -> String {
    // Find <w:pStyle w:val="...">
    if let Some(p) = para.find("<w:pStyle ") {
        let rest = &para[p..];
        if let Some(val) = extract_xml_attr(rest, "w:val") {
            return val;
        }
    }
    String::new()
}

/// One in-order piece of a paragraph: formatted text or an equation.
pub(crate) enum DocxSeg {
    Text { s: String, bold: bool, italic: bool },
    Math { latex: String, display: bool },
}

/// Detect bold / italic for the `<w:r>` run that owns the `<w:t>` at `before`.
/// Looks back to the nearest run start and inspects its run-properties for
/// `<w:b>` / `<w:i>` (honoring `w:val="false|0|off"` which turn them OFF).
fn docx_run_fmt(para: &str, before: usize) -> (bool, bool) {
    let rs1 = para[..before].rfind("<w:r>");
    let rs2 = para[..before].rfind("<w:r ");
    let rstart = match (rs1, rs2) {
        (Some(a), Some(b)) => a.max(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return (false, false),
    };
    let region = &para[rstart..before];
    let on = |tag: &str| -> bool {
        for pat in [format!("<{}/>", tag), format!("<{}>", tag), format!("<{} ", tag)] {
            if let Some(i) = region.find(&pat) {
                let seg_end = region[i..].find('>').map(|e| i + e).unwrap_or(region.len());
                let seg = &region[i..seg_end];
                if seg.contains("w:val=\"false\"")
                    || seg.contains("w:val=\"0\"")
                    || seg.contains("w:val=\"off\"")
                {
                    return false;
                }
                return true;
            }
        }
        false
    };
    (on("w:b"), on("w:i"))
}

/// Coalesce adjacent same-format text segments, then render to Markdown with
/// emphasis markers placed outside surrounding whitespace (so `**` never sits
/// next to a space, which would break the emphasis).
pub(crate) fn render_docx_segs(segs: Vec<DocxSeg>) -> String {
    let mut merged: Vec<DocxSeg> = Vec::new();
    for seg in segs {
        if let DocxSeg::Text { s, bold, italic } = &seg {
            if let Some(DocxSeg::Text { s: ps, bold: pb, italic: pi }) = merged.last_mut() {
                if *pb == *bold && *pi == *italic {
                    ps.push_str(s);
                    continue;
                }
            }
        }
        merged.push(seg);
    }

    let mut out = String::new();
    for seg in merged {
        match seg {
            DocxSeg::Text { s, bold, italic } => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    out.push_str(&s);
                    continue;
                }
                let lead = &s[..s.len() - s.trim_start().len()];
                let trail = &s[s.trim_end().len()..];
                let (open, close) = match (bold, italic) {
                    (true, true) => ("***", "***"),
                    (true, false) => ("**", "**"),
                    (false, true) => ("*", "*"),
                    (false, false) => ("", ""),
                };
                out.push_str(lead);
                out.push_str(open);
                out.push_str(trimmed);
                out.push_str(close);
                out.push_str(trail);
            }
            DocxSeg::Math { latex, display } => {
                let l = latex.trim();
                if l.is_empty() {
                    continue;
                }
                if display {
                    out.push_str(&format!("\n$$\n{}\n$$\n", l));
                } else {
                    out.push_str(&format!("${}$", l));
                }
            }
        }
    }
    out
}

fn docx_extract_runs(para: &str) -> String {
    // Collect text from <w:t> elements AND inline OMML equations (<m:oMath>),
    // in document order, capturing per-run bold/italic so emphasis survives import.
    let mut segs: Vec<DocxSeg> = Vec::new();
    let mut pos  = 0;

    while pos < para.len() {
        // Find next interesting element: <w:t or <m:oMath
        let wt    = para[pos..].find("<w:t").map(|p| (pos + p, false));
        let omath = para[pos..].find("<m:oMath").map(|p| (pos + p, true));

        let next = match (wt, omath) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None)    => Some(a),
            (None,    Some(b)) => Some(b),
            (None,    None)    => None,
        };

        match next {
            None => break,
            Some((abs, false)) => {
                // <w:t> text run - skip false matches like <w:tbl>, <w:tr>, <w:tc>
                // "<w:t" is 4 chars; abs+4 is the character immediately after,
                // which must be '>' or ' ' for a real <w:t> or <w:t ...> element.
                let after_wt = &para[abs+4..];
                let ch = after_wt.chars().next().unwrap_or('X');
                if ch != '>' && ch != ' ' && ch != '\n' && ch != '\r' && ch != '/' {
                    pos = abs + 4;
                    continue;
                }
                let close = para[abs..].find('>').map(|e| abs + e + 1).unwrap_or(para.len());
                let end   = para[close..].find("</w:t>").map(|e| close + e).unwrap_or(para.len());
                let t = decode_entities(&para[close..end]);
                let (bold, italic) = docx_run_fmt(para, abs);
                segs.push(DocxSeg::Text { s: t, bold, italic });
                pos = end + 6;
            }
            Some((abs, true)) => {
                // <m:oMath> OMML equation
                // Determine if it's inside <m:oMathPara> (display) - check a small lookback
                let is_display = abs >= 13 && para[abs-13..abs].contains("<m:oMathPara");
                let tag_end = para[abs..].find('>').map(|e| abs + e + 1).unwrap_or(para.len());
                let close_tag = if para[abs..].starts_with("<m:oMathPara") {
                    "</m:oMathPara>"
                } else {
                    "</m:oMath>"
                };
                if let Some(end_rel) = para[tag_end..].find(close_tag) {
                    let body = &para[tag_end..tag_end + end_rel];
                    let latex = omml_to_latex(body);
                    segs.push(DocxSeg::Math { latex, display: is_display });
                    pos = tag_end + end_rel + close_tag.len();
                } else {
                    pos = tag_end;
                }
            }
        }
    }
    render_docx_segs(segs)
}

/// Convert OMML (Office Math Markup Language) to LaTeX.
///
/// Implements structural mapping for the most common OMML elements.
/// Fallback: concatenate `<m:t>` leaf text, which gives a readable
/// (if not perfect) representation for unknown structures.
pub(crate) fn omml_to_latex(xml: &str) -> String {
    omml_node(xml)
}

fn omml_node(xml: &str) -> String {
    let mut out = String::new();
    let mut pos = 0;

    while pos < xml.len() {
        if !xml[pos..].starts_with('<') {
            // Text node
            let end = xml[pos..].find('<').unwrap_or(xml.len() - pos);
            out.push_str(&xml[pos..pos+end]);
            pos += end;
            continue;
        }
        pos += 1; // skip <
        let _ts = pos;
        let tag_end = xml[pos..].find('>').unwrap_or(xml.len() - pos);
        let tag = &xml[pos..pos+tag_end];
        pos += tag_end + 1;

        if tag.starts_with('/') { break; } // closing tag - stop this level
        let self_closing = tag.ends_with('/');
        if self_closing { continue; }

        let tag_name = tag.split(|c: char| !c.is_alphanumeric() && c != ':')
                          .next().unwrap_or("").to_string();

        // Find the matching closing tag, honoring nested same-name tags (a naive
        // first-match truncated bodies that contained nested `<m:e>` etc.).
        let close_tag = format!("</{}>", tag_name);
        let body = if let Some(end) = omml_matching_close(xml, &tag_name, pos) {
            let b = xml[pos..end].to_string();
            pos = end + close_tag.len();
            b
        } else {
            String::new()
        };

        match tag_name.as_str() {
            // Fraction: \frac{num}{den}
            "m:f" => {
                let num = omml_child(&body, "m:num");
                let den = omml_child(&body, "m:den");
                out.push_str(&format!("\\frac{{{}}}{{{}}}", omml_node(&num), omml_node(&den)));
            }
            // Superscript: base^{exp}
            "m:sSup" => {
                let base = omml_child(&body, "m:e");
                let sup  = omml_child(&body, "m:sup");
                out.push_str(&format!("{}^{{{}}}", omml_node(&base), omml_node(&sup)));
            }
            // Subscript: base_{sub}
            "m:sSub" => {
                let base = omml_child(&body, "m:e");
                let sub  = omml_child(&body, "m:sub");
                out.push_str(&format!("{}_{{{}}}",  omml_node(&base), omml_node(&sub)));
            }
            // Sub+superscript: base_{sub}^{sup}
            "m:sSubSup" => {
                let base = omml_child(&body, "m:e");
                let sub  = omml_child(&body, "m:sub");
                let sup  = omml_child(&body, "m:sup");
                out.push_str(&format!("{}_{{{}}}", omml_node(&base), omml_node(&sub)));
                out.push_str(&format!("^{{{}}}", omml_node(&sup)));
            }
            // Radical: \sqrt[deg]{body}
            "m:rad" => {
                let deg  = omml_child(&body, "m:deg");
                let e    = omml_child(&body, "m:e");
                let deg_latex = omml_node(&deg);
                if deg_latex.trim().is_empty() {
                    out.push_str(&format!("\\sqrt{{{}}}", omml_node(&e)));
                } else {
                    out.push_str(&format!("\\sqrt[{}]{{{}}}", deg_latex.trim(), omml_node(&e)));
                }
            }
            // n-ary operator (sum, product, integral, etc.)
            "m:nary" => {
                let chr   = omml_nary_chr(&body);
                let sub   = omml_child(&body, "m:sub");
                let sup   = omml_child(&body, "m:sup");
                let e     = omml_child(&body, "m:e");
                out.push_str(&chr);
                if !sub.is_empty()  { out.push_str(&format!("_{{{}}}", omml_node(&sub))); }
                if !sup.is_empty()  { out.push_str(&format!("^{{{}}}", omml_node(&sup))); }
                // Separate the operator command from its operand: "\sum A", never
                // "\sumA" (which LaTeX/KaTeX/Typst read as one unknown command).
                out.push(' ');
                out.push_str(&omml_node(&e));
            }
            // Delimiter: \left(...\right) or similar
            "m:d" => {
                let (open_d, close_d) = omml_delimiters(&body);
                let e = omml_child(&body, "m:e");
                out.push_str(&format!("\\left{}{}\\right{}", open_d, omml_node(&e), close_d));
            }
            // Matrix (array)
            "m:m" => {
                let rows: Vec<String> = omml_children_all(&body, "m:mr")
                    .into_iter()
                    .map(|row| {
                        let cells: Vec<String> = omml_children_all(&row, "m:e")
                            .into_iter().map(|c| omml_node(&c)).collect();
                        cells.join(" & ")
                    })
                    .collect();
                out.push_str(&format!("\\begin{{matrix}}{}\n\\end{{matrix}}", rows.join(" \\\\\n")));
            }
            // Text run leaf - the actual characters
            "m:r" => {
                out.push_str(&omml_run_text(&body));
            }
            // Math properties - skip
            "m:rPr" | "m:rSPr" | "m:sPrePr" | "m:sSupPr" | "m:sSubPr" |
            "m:sSubSupPr" | "m:radPr" | "m:fPr" | "m:dPr" | "m:naryPr" |
            "m:mPr" | "m:mrPr" | "m:limLocPr" | "m:groupChrPr" => {}
            // Everything else: recurse into children
            _ => { out.push_str(&omml_node(&body)); }
        }
    }
    out
}

/// True when `name_end` (index just past a tag name) is a real tag-name
/// boundary, so `<m:e` does not match `<m:endChr` / `<m:eqArr`.
fn omml_name_boundary(xml: &str, name_end: usize) -> bool {
    matches!(
        xml[name_end..].chars().next(),
        Some(' ') | Some('>') | Some('/') | Some('\t') | Some('\n') | Some('\r')
    )
}

/// Find the `</tag>` matching an open already consumed, honoring nesting of the
/// SAME tag and ignoring self-closing `<tag/>`. `from` is just past the opening
/// tag's `>`. Returns the byte offset of the `<` of the matching close. Without
/// this, OMML with nested same-name elements (e.g. `<m:e>` inside `<m:e>`) lost
/// its content and produced empty `( )` equations.
fn omml_matching_close(xml: &str, tag: &str, from: usize) -> Option<usize> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut depth = 1usize;
    let mut i = from;
    loop {
        let no = xml[i..].find(&open).map(|p| i + p);
        let nc = xml[i..].find(&close).map(|p| i + p);
        match (no, nc) {
            (Some(o), maybe_c) if maybe_c.map_or(true, |c| o < c) => {
                if omml_name_boundary(xml, o + open.len()) {
                    let gt = xml[o..].find('>').map(|g| o + g)?;
                    if !xml[..gt].ends_with('/') {
                        depth += 1;
                    }
                    i = gt + 1;
                } else {
                    i = o + open.len();
                }
            }
            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(c);
                }
                i = c + close.len();
            }
            _ => return None,
        }
    }
}

/// Content of the first real `<tag ...>...</tag>` (boundary- and nesting-aware).
fn omml_child(xml: &str, tag: &str) -> String {
    let open = format!("<{}", tag);
    let mut i = 0;
    while let Some(p) = xml[i..].find(&open).map(|x| i + x) {
        if omml_name_boundary(xml, p + open.len()) {
            if let Some(gt) = xml[p..].find('>').map(|g| p + g) {
                if xml[..gt].ends_with('/') {
                    return String::new(); // self-closing: no body
                }
                let body_start = gt + 1;
                if let Some(end) = omml_matching_close(xml, tag, body_start) {
                    return xml[body_start..end].to_string();
                }
            }
        }
        i = p + open.len();
    }
    String::new()
}

/// Extract all top-level occurrences of `<tag ...>...</tag>` (boundary- and
/// nesting-aware, so a nested same-name tag does not split a parent).
fn omml_children_all(xml: &str, tag: &str) -> Vec<String> {
    let mut results = Vec::new();
    let open  = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut pos = 0;
    while let Some(p) = xml[pos..].find(&open).map(|x| pos + x) {
        if !omml_name_boundary(xml, p + open.len()) {
            pos = p + open.len();
            continue;
        }
        let Some(gt) = xml[p..].find('>').map(|g| p + g) else { break };
        if xml[..gt].ends_with('/') { pos = gt + 1; continue; } // self-closing
        let body_start = gt + 1;
        match omml_matching_close(xml, tag, body_start) {
            Some(end) => {
                results.push(xml[body_start..end].to_string());
                pos = end + close.len();
            }
            None => break,
        }
    }
    results
}

/// Extract the LaTeX character from `<m:nary>` based on the `<m:chr>` value.
fn omml_nary_chr(body: &str) -> &'static str {
    if let Some(p) = body.find("m:chr=\"") {
        let val = &body[p+7..];
        let end = val.find('"').unwrap_or(val.len());
        return match &val[..end] {
            "∑" | "Σ" => "\\sum",
            "∏" | "Π" => "\\prod",
            "∫"       => "\\int",
            "∬"       => "\\iint",
            "∭"       => "\\iiint",
            "∮"       => "\\oint",
            _         => "\\sum",
        };
    }
    "\\sum"
}

/// Extract opening/closing delimiter characters from `<m:d>`.
fn omml_delimiters(body: &str) -> (&'static str, &'static str) {
    let open  = extract_xml_attr(body, "m:begChr").unwrap_or_default();
    let close = extract_xml_attr(body, "m:endChr").unwrap_or_default();
    let open_d  = match open.as_str()  { "(" => "(", "[" => "[", "{" => "\\{", "|" => "|", _ => "(" };
    let close_d = match close.as_str() { ")" => ")", "]" => "]", "}" => "\\}", "|" => "|", _ => ")" };
    (open_d, close_d)
}

/// Extract text content from an OMML run `<m:r>...</m:r>`.
/// Maps common Unicode math symbols to LaTeX commands.
fn omml_run_text(body: &str) -> String {
    // A run may carry several <m:t> leaves - gather them all (not just the first),
    // otherwise multi-part runs lose text.
    let parts = omml_children_all(body, "m:t");
    let raw: String = if parts.is_empty() {
        omml_child(body, "m:t")
    } else {
        parts.join("")
    };
    if raw.is_empty() {
        return String::new();
    }

    let is_normal = body.contains("<m:nor") || body.contains("m:val=\"p\"");

    // Map math symbols / Greek to LaTeX FIRST so Greek (non-ASCII) becomes ASCII
    // (\alpha) and is not mistaken for accented prose in the check below.
    let mapped: String = raw.chars().map(|c| {
        let m = map_math_char(c);
        // Commands map to multi-letter Typst tokens; a LEADING space stops them
        // gluing onto a preceding letter ("h≈" -> "h\approx" -> Typst "happrox").
        // Math-mode whitespace is insignificant, so this is safe for LaTeX/KaTeX.
        if m.starts_with('\\') { format!(" {m}") } else { m }
    }).collect();

    // Upright \text{} for plain-text runs, or when ANY non-ASCII char remains
    // after mapping (accents like 'é', Unicode super/subscripts like '⁻²', stray
    // arrows). Real math symbols are already mapped to ASCII \commands, so a
    // leftover non-ASCII would hit Typst's math font ("current font does not
    // support math") - wrapping the run in \text{...} renders it with the text font.
    let needs_text = is_normal || mapped.chars().any(|c| !c.is_ascii());
    if needs_text {
        let escaped: String = raw.chars().map(|c| match c {
            '\\' => "\\textbackslash{}".to_string(),
            '{' => "\\{".to_string(),
            '}' => "\\}".to_string(),
            '%' => "\\%".to_string(),
            '&' => "\\&".to_string(),
            '#' => "\\#".to_string(),
            '_' => "\\_".to_string(),
            '$' => "\\$".to_string(),
            other => other.to_string(),
        }).collect();
        return format!("\\text{{{}}}", escaped);
    }
    mapped
}

/// Map a single math char to its LaTeX command (Greek letters, operators); any
/// other char maps to itself.
fn map_math_char(c: char) -> String {
    match c {
        '×' => "\\times ".to_string(),
        '·' | '⋅' => "\\cdot ".to_string(),
        '÷' => "\\div ".to_string(),
        '±' => "\\pm ".to_string(),
        '∓' => "\\mp ".to_string(),
        '−' => "-".to_string(),            // U+2212 minus sign → ASCII hyphen
        '∝' => "\\propto ".to_string(),
        '≡' => "\\equiv ".to_string(),
        '≅' => "\\cong ".to_string(),
        '∼' => "\\sim ".to_string(),
        '∘' => "\\circ ".to_string(),
        '∗' => "*".to_string(),
        '⊆' => "\\subseteq ".to_string(),
        '⊇' => "\\supseteq ".to_string(),
        '⊕' => "\\oplus ".to_string(),
        '⊗' => "\\otimes ".to_string(),
        '⊙' => "\\odot ".to_string(),
        '∧' => "\\wedge ".to_string(),
        '∨' => "\\vee ".to_string(),
        '∀' => "\\forall ".to_string(),
        '∃' => "\\exists ".to_string(),
        '↦' => "\\mapsto ".to_string(),
        '⟨' => "\\langle ".to_string(),
        '⟩' => "\\rangle ".to_string(),
        '≤' => "\\leq ".to_string(),
        '≥' => "\\geq ".to_string(),
        '≠' => "\\neq ".to_string(),
        '≈' => "\\approx ".to_string(),
        '∞' => "\\infty ".to_string(),
        '∂' => "\\partial ".to_string(),
        '∇' => "\\nabla ".to_string(),
        '∈' => "\\in ".to_string(),
        '∉' => "\\notin ".to_string(),
        '⊂' => "\\subset ".to_string(),
        '⊃' => "\\supset ".to_string(),
        '∩' => "\\cap ".to_string(),
        '∪' => "\\cup ".to_string(),
        '→' => "\\rightarrow ".to_string(),
        '←' => "\\leftarrow ".to_string(),
        '↔' => "\\leftrightarrow ".to_string(),
        '⇒' => "\\Rightarrow ".to_string(),
        '⇔' => "\\Leftrightarrow ".to_string(),
        'α' => "\\alpha ".to_string(),
        'β' => "\\beta ".to_string(),
        'γ' => "\\gamma ".to_string(),
        'δ' => "\\delta ".to_string(),
        'ε' => "\\epsilon ".to_string(),
        'θ' => "\\theta ".to_string(),
        'λ' => "\\lambda ".to_string(),
        'μ' => "\\mu ".to_string(),
        'π' => "\\pi ".to_string(),
        'σ' => "\\sigma ".to_string(),
        'φ' | 'ϕ' => "\\phi ".to_string(),
        'ω' => "\\omega ".to_string(),
        'Γ' => "\\Gamma ".to_string(),
        'Δ' => "\\Delta ".to_string(),
        'Θ' => "\\Theta ".to_string(),
        'Λ' => "\\Lambda ".to_string(),
        'Π' => "\\Pi ".to_string(),
        'Σ' => "\\Sigma ".to_string(),
        'Φ' => "\\Phi ".to_string(),
        'Ψ' => "\\Psi ".to_string(),
        'Ω' => "\\Omega ".to_string(),
        '√' => "\\sqrt".to_string(),
        '∑' => "\\sum ".to_string(),
        '∏' => "\\prod ".to_string(),
        '∫' => "\\int ".to_string(),
        c   => c.to_string(),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RTF → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert an RTF file to Markdown.
///
/// Best-effort: strips control words, maps `\b`/`\i` to `**`/`*`,
/// maps `\pard` / `\par` to paragraph breaks, decodes `\'xx` hex escapes.
/// Complex RTF features (tables, fields, embedded objects) are silently ignored.
pub fn rtf_to_md(path: &Path) -> Result<String, String> {
    let rtf = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(rtf_strip(&rtf))
}

fn rtf_strip(rtf: &str) -> String {
    let mut out    = String::new();
    let bytes      = rtf.as_bytes();
    let len        = bytes.len();
    let mut i      = 0usize;
    let mut depth  = 0i32;   // { nesting depth
    let mut bold   = false;
    let mut italic = false;
    let mut skip_group = 0i32; // depth at which we entered a skip group (\*)

    while i < len {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                if skip_group > 0 && depth <= skip_group { skip_group = 0; }
                depth -= 1;
                i += 1;
            }
            b'\\' if i + 1 < len => {
                i += 1;
                if bytes[i].is_ascii_alphabetic() {
                    // Read control word
                    let start = i;
                    while i < len && bytes[i].is_ascii_alphabetic() { i += 1; }
                    // Optional numeric parameter (may be negative)
                    let param_start = i;
                    if i < len && bytes[i] == b'-' { i += 1; }
                    while i < len && bytes[i].is_ascii_digit() { i += 1; }
                    let param_end = i;
                    // Skip optional trailing space delimiter
                    if i < len && bytes[i] == b' ' { i += 1; }

                    let word  = &rtf[start..param_start];
                    let param = &rtf[param_start..param_end];

                    if skip_group > 0 { continue; }

                    match word {
                        "b"   => {
                            let on = param != "0";
                            if on && !bold   { bold = true;  if depth <= 1 { out.push_str("**"); } }
                            if !on && bold   { bold = false; if depth <= 1 { out.push_str("**"); } }
                        }
                        "i"   => {
                            let on = param != "0";
                            if on && !italic   { italic = true;  if depth <= 1 { out.push('*'); } }
                            if !on && italic   { italic = false; if depth <= 1 { out.push('*'); } }
                        }
                        "par" | "pard" => {
                            if depth <= 1 { out.push_str("\n\n"); }
                        }
                        "line" => { if depth <= 1 { out.push('\n'); } }
                        "tab"  => { if depth <= 1 { out.push('\t'); } }
                        "bullet" => { if depth <= 1 { out.push_str("- "); } }
                        "sect" | "page" => { if depth <= 1 { out.push_str("\n---\n\n"); } }
                        _ => {}
                    }
                } else if bytes[i] == b'\'' && i + 2 < len {
                    // \'xx hex-encoded character
                    let val = hex_nibble(bytes[i+1]) * 16 + hex_nibble(bytes[i+2]);
                    i += 3;
                    if skip_group > 0 { continue; }
                    // Windows-1252 → approximate ASCII mapping for common chars
                    match val {
                        0x20..=0x7E => { if depth <= 1 { out.push(val as char); } }
                        0x85 => { if depth <= 1 { out.push('\u{2026}'); } }
                        0x91 => { if depth <= 1 { out.push('\u{2018}'); } }
                        0x92 => { if depth <= 1 { out.push('\u{2019}'); } }
                        0x93 => { if depth <= 1 { out.push('"'); } }
                        0x94 => { if depth <= 1 { out.push('"'); } }
                        0x96 => { if depth <= 1 { out.push('-'); } }
                        0x97 => { if depth <= 1 { out.push('-'); } }
                        _    => {}
                    }
                } else if bytes[i] == b'*' {
                    // \* destination - skip everything until matching }
                    skip_group = depth;
                    i += 1;
                } else {
                    // Special characters: \\ \{ \} \- \_ \~ etc.
                    if skip_group == 0 && depth <= 1 {
                        match bytes[i] {
                            b'\\' => out.push('\\'),
                            b'{' =>  out.push('{'),
                            b'}' =>  out.push('}'),
                            b'-' =>  {} // optional hyphen
                            b'_' =>  out.push('\u{00A0}'), // non-breaking hyphen
                            b'~' =>  out.push('\u{00A0}'), // non-breaking space
                            b'\n'|b'\r' => out.push('\n'),
                            _ =>     {}
                        }
                    }
                    i += 1;
                }
            }
            b'\n' | b'\r' => {
                // Bare newlines in RTF have no semantic meaning
                i += 1;
            }
            b => {
                if skip_group == 0 && depth <= 1 {
                    out.push(b as char);
                }
                i += 1;
            }
        }
    }
    collapse_blank_lines(&out)
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _            => 0,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// CSV / TSV → Markdown table
// ═════════════════════════════════════════════════════════════════════════════

/// Convert CSV or TSV content to a GFM pipe table.
/// First row is treated as the header. Delimiter auto-detected (tab vs comma).
pub fn csv_to_md(content: &str) -> Result<String, String> {
    let delimiter = if content.contains('\t') { '\t' } else { ',' };
    let rows: Vec<Vec<String>> = content.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| parse_csv_row(line, delimiter))
        .collect();

    if rows.is_empty() { return Ok(String::new()); }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(1);
    let mut md = String::new();

    // Header row
    md.push('|');
    for i in 0..col_count {
        md.push(' ');
        md.push_str(rows[0].get(i).map(|s| s.as_str()).unwrap_or(""));
        md.push_str(" |");
    }
    md.push('\n');

    // Separator row
    md.push('|');
    for _ in 0..col_count { md.push_str(" --- |"); }
    md.push('\n');

    // Data rows
    for row in rows.iter().skip(1) {
        md.push('|');
        for i in 0..col_count {
            md.push(' ');
            md.push_str(row.get(i).map(|s| s.as_str()).unwrap_or(""));
            md.push_str(" |");
        }
        md.push('\n');
    }
    Ok(md)
}

fn parse_csv_row(line: &str, delim: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field  = String::new();
    let mut in_q   = false;
    for ch in line.chars() {
        if ch == '"' { in_q = !in_q; }
        else if ch == delim && !in_q { fields.push(std::mem::take(&mut field)); }
        else { field.push(ch); }
    }
    fields.push(field);
    fields
}

// ═════════════════════════════════════════════════════════════════════════════
// Source code → fenced Markdown code block
// ═════════════════════════════════════════════════════════════════════════════

/// Wrap source code in a fenced code block with the given language identifier.
pub fn code_to_md(content: &str, lang: &str) -> Result<String, String> {
    Ok(format!("```{}\n{}\n```\n", lang, content.trim_end()))
}

// ═════════════════════════════════════════════════════════════════════════════
// LaTeX (.tex) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert a LaTeX source file to Markdown.
/// Handles: \section/\subsection, \textbf, \textit, \emph, \texttt,
/// \begin{equation}/itemize/enumerate/verbatim/abstract/figure, \href,
/// inline $...$, display \[...\], \(...\).
/// Collect the raw `\newcommand` / `\renewcommand` / `\providecommand` / `\def`
/// definition lines from a LaTeX source (typically the preamble) so they can be
/// preserved verbatim into the converted Markdown for later macro expansion.
fn extract_preamble_macros(content: &str) -> String {
    let mut out = String::new();
    for line in content.lines() {
        let t = line.trim_start();
        if t.starts_with("\\newcommand")
            || t.starts_with("\\renewcommand")
            || t.starts_with("\\providecommand")
            || t.starts_with("\\def")
        {
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// Collapse newlines (and surrounding whitespace) inside inline math to a single
/// space. In LaTeX, `$...$` may span physical lines (a newline is just a space),
/// but Markdown breaks inline `$...$` at a block boundary - and a continuation
/// line starting with "- " would even be parsed as a bullet list item, leaving
/// the `$` literal in the output. Keeping inline math on one line is faithful
/// (whitespace is insignificant in math mode) and prevents both failures.
fn collapse_math_newlines(s: &str) -> String {
    if !s.contains('\n') && !s.contains('\r') { return s.to_string(); }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' || c == '\r' {
            while matches!(chars.peek(), Some(' ' | '\t' | '\n' | '\r')) { chars.next(); }
            while out.ends_with(' ') || out.ends_with('\t') { out.pop(); }
            if !out.is_empty() { out.push(' '); }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn tex_to_md(content: &str) -> Result<String, String> {
    let mut src = content;

    // Capture preamble macro definitions (\newcommand/\def) BEFORE stripping the
    // preamble, so custom macros like \sket survive into the Markdown and can be
    // expanded at equation-render time. They are emitted as an HTML comment block
    // (invisible in the rendered document) that `MacroTable::collect` still scans.
    let preamble_macros = extract_preamble_macros(content);

    // Strip preamble (\documentclass ... \begin{document})
    if let Some(p) = src.find("\\begin{document}") {
        src = &src[p + 16..];
    }
    if let Some(p) = src.find("\\end{document}") {
        src = &src[..p];
    }

    let mut md    = String::new();
    if !preamble_macros.is_empty() {
        md.push_str("<!-- mdall:latex-macros\n");
        md.push_str(&preamble_macros);
        md.push_str("\n-->\n\n");
    }
    let bytes     = src.as_bytes();
    let len       = src.len();
    let mut i     = 0usize;

    while i < len {
        // LaTeX line comment
        if bytes[i] == b'%' {
            while i < len && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        // Inline math $...$  (not $$)
        if bytes[i] == b'$' {
            if i + 1 < len && bytes[i+1] == b'$' {
                // Display $$ ... $$
                i += 2;
                let start = i;
                while i + 1 < len && !(bytes[i] == b'$' && bytes[i+1] == b'$') { i += 1; }
                let latex = src[start..i].trim();
                md.push_str(&format!("\n$$\n{}\n$$\n\n", latex));
                if i + 1 < len { i += 2; }
                continue;
            } else {
                // Inline $...$
                i += 1;
                let start = i;
                while i < len && bytes[i] != b'$' { i += 1; }
                let latex = collapse_math_newlines(&src[start..i]);
                md.push_str(&format!("${}$", latex));
                if i < len { i += 1; }
                continue;
            }
        }
        if bytes[i] != b'\\' {
            match bytes[i] {
                // LaTeX grouping braces in prose - skip, don't emit
                b'{' | b'}' => { i += 1; }
                // ASCII passes through directly.
                b if b < 0x80 => { md.push(b as char); i += 1; }
                // Non-ASCII: copy the WHOLE UTF-8 character. Pushing each byte
                // as a char (`b as char`) would split 'ö' (C3 B6) into 'Ã' + '¶'
                // mojibake - the bug that corrupted accented prose on import.
                _ => {
                    let ch = src[i..].chars().next().unwrap_or('\u{FFFD}');
                    md.push(ch);
                    i += ch.len_utf8();
                }
            }
            continue;
        }
        // Control sequence
        i += 1;
        if i >= len { break; }

        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < len && bytes[i].is_ascii_alphabetic() { i += 1; }
            let word = &src[start..i];
            // Skip optional whitespace / *
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'*') { i += 1; }

            match word {
                "title"         => { let a = tex_braced(src, &mut i); ensure_double_newline(&mut md); md.push_str(&format!("# {}\n\n", a)); }
                "author"        => { let a = tex_braced(src, &mut i); md.push_str(&format!("*{}*\n\n", a)); }
                "date"          => { let a = tex_braced(src, &mut i); md.push_str(&format!("*{}*\n\n", a)); }
                "section"       => { let a = tex_braced(src, &mut i); md.push_str(&format!("\n## {}\n\n", a)); }
                "subsection"    => { let a = tex_braced(src, &mut i); md.push_str(&format!("\n### {}\n\n", a)); }
                "subsubsection" => { let a = tex_braced(src, &mut i); md.push_str(&format!("\n#### {}\n\n", a)); }
                "paragraph"     => { let a = tex_braced(src, &mut i); md.push_str(&format!("\n##### {}\n\n", a)); }
                "chapter"       => { let a = tex_braced(src, &mut i); md.push_str(&format!("\n# {}\n\n", a)); }
                "includegraphics" => {
                    // Skip optional [width=...]
                    if i < len && bytes[i] == b'[' {
                        while i < len && bytes[i] != b']' { i += 1; }
                        if i < len { i += 1; }
                    }
                    let path = tex_braced(src, &mut i);
                    md.push_str(&format!("\n![]({})\n\n", path));
                }
                "begin" => {
                    let env = tex_braced(src, &mut i);
                    // Skip optional argument [...]
                    if i < len && bytes[i] == b'[' {
                        while i < len && bytes[i] != b']' { i += 1; }
                        if i < len { i += 1; }
                    }
                    let env = env.trim().to_string();
                    match env.as_str() {
                        "equation" | "equation*" | "align" | "align*" |
                        "gather" | "gather*" | "multline" | "multline*" => {
                            let body = tex_until_end(src, &mut i, &env);
                            md.push_str(&format!("\n$$\n{}\n$$\n\n", body.trim()));
                        }
                        "itemize" => {
                            let body = tex_until_end(src, &mut i, "itemize");
                            md.push_str(&tex_list_to_md(&body, false));
                        }
                        "enumerate" => {
                            let body = tex_until_end(src, &mut i, "enumerate");
                            md.push_str(&tex_list_to_md(&body, true));
                        }
                        "verbatim" | "lstlisting" | "minted" | "alltt" => {
                            let body = tex_until_end(src, &mut i, &env);
                            md.push_str(&format!("\n```\n{}\n```\n\n", body.trim()));
                        }
                        "abstract" => {
                            let body = tex_until_end(src, &mut i, "abstract");
                            md.push_str("\n> **Abstract**\n>\n");
                            // Convert each PARAGRAPH as one fragment (blank line =
                            // LaTeX paragraph break) so inline commands like
                            // \textbf{...} that span soft line breaks stay intact,
                            // then flatten soft wraps into a single blockquote line.
                            for para in body.split("\n\n") {
                                if para.trim().is_empty() { continue; }
                                let converted = tex_fragment_to_md(para);
                                let flat = converted.split_whitespace().collect::<Vec<_>>().join(" ");
                                if !flat.is_empty() {
                                    md.push_str(&format!("> {}\n>\n", flat));
                                }
                            }
                            md.push('\n');
                        }
                        "figure" | "figure*" | "wrapfigure" => {
                            // Use &env directly - "figure*" must find \end{figure*}, not \end{figure}
                            let body = tex_until_end(src, &mut i, &env);
                            let img = tex_extract_cmd(&body, "includegraphics");
                            let cap = tex_fragment_to_md(&tex_extract_cmd(&body, "caption"));
                            if !img.is_empty() {
                                md.push_str(&format!("\n![{}]({})\n\n", cap.trim(), img));
                            }
                        }
                        "table" | "table*" => {
                            // Use &env directly to avoid swallowing entire document on table*
                            let body = tex_until_end(src, &mut i, &env);
                            let cap = tex_fragment_to_md(&tex_extract_cmd(&body, "caption"));
                            if !cap.trim().is_empty() {
                                md.push_str(&format!("\n*Table: {}*\n\n", cap.trim()));
                            }
                        }
                        "tabular" | "tabular*" => {
                            // Tabular = raw data table - skip silently, keep caption if nested in table
                            tex_until_end(src, &mut i, &env);
                        }
                        "quote" | "quotation" | "displayquote" => {
                            let body = tex_until_end(src, &mut i, &env);
                            // Per-paragraph (not per-line) so inline commands spanning
                            // soft line breaks (e.g. \textbf{...}) survive intact.
                            for para in body.split("\n\n") {
                                if para.trim().is_empty() { continue; }
                                let converted = tex_fragment_to_md(para);
                                let flat = converted.split_whitespace().collect::<Vec<_>>().join(" ");
                                if !flat.is_empty() {
                                    md.push_str(&format!("> {}\n>\n", flat));
                                }
                            }
                            md.push('\n');
                        }
                        "document" => {} // nested \begin{document} - ignore
                        // Layout environments: just recurse (process content normally)
                        "center" | "flushleft" | "flushright" | "minipage" |
                        "multicols" | "column" | "columns" | "frame" |
                        "block" | "alertblock" | "exampleblock" |
                        "theorem" | "lemma" | "proof" | "corollary" | "definition" |
                        "remark" | "example" | "exercise" | "solution" => {
                            // These environments contain normal text - process it recursively
                            // by NOT consuming it here; the main loop will handle the content.
                            // We just skip the optional argument if present.
                            if i < len && bytes[i] == b'[' {
                                while i < len && bytes[i] != b']' { i += 1; }
                                if i < len { i += 1; }
                            }
                            if i < len && bytes[i] == b'{' {
                                while i < len && bytes[i] != b'}' { i += 1; }
                                if i < len { i += 1; }
                            }
                            // Content processed by main loop; \end{env} handled by "end" arm
                        }
                        _ => {
                            // Truly unknown env - skip it to avoid dumping raw LaTeX
                            let _body = tex_until_end(src, &mut i, &env);
                            // Do not emit raw LaTeX
                        }
                    }
                }
                "end" => { tex_braced(src, &mut i); }
                "newline" | "linebreak" => md.push_str("  \n"),
                "newpage" | "clearpage" | "cleardoublepage" => md.push_str("\n---\n\n"),
                "maketitle" | "tableofcontents" | "listoffigures" | "listoftables" => {}
                "index" => { tex_braced(src, &mut i); }
                "vspace" | "hspace" | "vskip" | "hskip" | "setlength" | "addtolength" => { tex_braced(src, &mut i); }
                "noindent" | "indent" | "par" => {}
                "item" => {}
                // All other backslash-letter commands are treated as inline:
                // formatting, citations, references, accents, special letters,
                // and a generic "keep the text, drop the command" fallback.
                _ => {
                    if let Some(piece) = tex_inline_word_to_md(word, src, &mut i) {
                        md.push_str(&piece);
                    }
                }
            }
        } else if bytes[i] == b'[' {
            // \[ display math
            i += 1;
            if let Some(end) = src[i..].find("\\]") {
                let latex = src[i..i+end].trim();
                md.push_str(&format!("\n$$\n{}\n$$\n\n", latex));
                i += end + 2;
            }
        } else if bytes[i] == b'(' {
            // \( inline math
            i += 1;
            if let Some(end) = src[i..].find("\\)") {
                let latex = collapse_math_newlines(src[i..i+end].trim());
                md.push_str(&format!("${}$", latex));
                i += end + 2;
            }
        } else {
            // Escaped specials, accent symbols (\'e, \"u, ...), spacing.
            md.push_str(&tex_inline_symbol_to_md(src, &mut i));
        }
    }

    Ok(collapse_blank_lines(&md))
}

// ── Shared LaTeX inline conversion ───────────────────────────────────────────
//
// The same character loop is used by the prose path of `tex_to_md` and by every
// environment-body handler (abstract, captions, list items, quotes). Routing all
// of them through one engine guarantees inline commands like \textbf, \emph,
// \cite, accents and escaped specials are converted everywhere, not just at the
// document top level.

/// Map a single-token LaTeX letter command (no braces) to its Unicode letter.
/// Returns None for unknown tokens. Covers the common Latin special-letter set.
fn tex_simple_letter(word: &str) -> Option<&'static str> {
    Some(match word {
        "l" => "\u{0142}",  // l with stroke
        "L" => "\u{0141}",
        "o" => "\u{00F8}",  // o with stroke
        "O" => "\u{00D8}",
        "ss" => "\u{00DF}", // sharp s
        "ae" => "\u{00E6}",
        "AE" => "\u{00C6}",
        "oe" => "\u{0153}",
        "OE" => "\u{0152}",
        "aa" => "\u{00E5}",
        "AA" => "\u{00C5}",
        "i" => "\u{0131}",  // dotless i
        "j" => "\u{0237}",  // dotless j
        _ => return None,
    })
}

/// Compose a LaTeX accent (e.g. \'e -> e-acute) given the accent symbol byte and
/// the base letter. Returns None when no precomposed mapping is known.
fn tex_accent_compose(accent: char, base: char) -> Option<char> {
    let mapped = match (accent, base) {
        ('\'', 'a') => '\u{00E1}', ('\'', 'e') => '\u{00E9}', ('\'', 'i') => '\u{00ED}',
        ('\'', 'o') => '\u{00F3}', ('\'', 'u') => '\u{00FA}', ('\'', 'y') => '\u{00FD}',
        ('\'', 'c') => '\u{0107}', ('\'', 'n') => '\u{0144}', ('\'', 's') => '\u{015B}',
        ('\'', 'z') => '\u{017A}',
        ('\'', 'A') => '\u{00C1}', ('\'', 'E') => '\u{00C9}', ('\'', 'I') => '\u{00CD}',
        ('\'', 'O') => '\u{00D3}', ('\'', 'U') => '\u{00DA}',
        ('`', 'a') => '\u{00E0}', ('`', 'e') => '\u{00E8}', ('`', 'i') => '\u{00EC}',
        ('`', 'o') => '\u{00F2}', ('`', 'u') => '\u{00F9}',
        ('`', 'A') => '\u{00C0}', ('`', 'E') => '\u{00C8}', ('`', 'O') => '\u{00D2}',
        ('^', 'a') => '\u{00E2}', ('^', 'e') => '\u{00EA}', ('^', 'i') => '\u{00EE}',
        ('^', 'o') => '\u{00F4}', ('^', 'u') => '\u{00FB}',
        ('^', 'A') => '\u{00C2}', ('^', 'E') => '\u{00CA}', ('^', 'O') => '\u{00D4}',
        ('"', 'a') => '\u{00E4}', ('"', 'e') => '\u{00EB}', ('"', 'i') => '\u{00EF}',
        ('"', 'o') => '\u{00F6}', ('"', 'u') => '\u{00FC}', ('"', 'y') => '\u{00FF}',
        ('"', 'A') => '\u{00C4}', ('"', 'O') => '\u{00D6}', ('"', 'U') => '\u{00DC}',
        ('~', 'a') => '\u{00E3}', ('~', 'n') => '\u{00F1}', ('~', 'o') => '\u{00F5}',
        ('~', 'A') => '\u{00C3}', ('~', 'N') => '\u{00D1}', ('~', 'O') => '\u{00D5}',
        ('c', 'c') => '\u{00E7}', ('c', 'C') => '\u{00C7}', // cedilla \c{c}
        _ => return None,
    };
    Some(mapped)
}

/// Read the base letter for an accent command. The base may be `{x}` or a single
/// following letter (optionally preceded by spaces). Advances `i` past it.
/// Returns the base char, or None if nothing usable follows.
fn tex_accent_base(src: &str, i: &mut usize) -> Option<char> {
    let bytes = src.as_bytes();
    let len = src.len();
    while *i < len && (bytes[*i] == b' ' || bytes[*i] == b'\t' || bytes[*i] == b'\n') { *i += 1; }
    if *i >= len { return None; }
    if bytes[*i] == b'{' {
        let inner = tex_braced(src, i);
        return inner.chars().next();
    }
    let ch = src[*i..].chars().next()?;
    *i += ch.len_utf8();
    Some(ch)
}

/// Handle a backslash-letter command that is INLINE (prose-level): formatting,
/// links, citations, references, accents, special letters, generic fallback.
/// `i` is positioned just after the optional whitespace/star that follows the
/// command word. Returns Some(markdown) if handled, or None if the command is a
/// structural/block command that the caller's own dispatcher must handle.
fn tex_inline_word_to_md(word: &str, src: &str, i: &mut usize) -> Option<String> {
    let s = match word {
        "textbf" | "mathbf" | "textsc" => format!("**{}**", tex_fragment_to_md(&tex_braced(src, i))),
        "textit" | "emph" | "textsl"   => format!("*{}*", tex_fragment_to_md(&tex_braced(src, i))),
        "texttt" => format!("`{}`", tex_braced(src, i)),
        "underline" => format!("<u>{}</u>", tex_fragment_to_md(&tex_braced(src, i))),
        "textsuperscript" => format!("<sup>{}</sup>", tex_fragment_to_md(&tex_braced(src, i))),
        "textsubscript" => format!("<sub>{}</sub>", tex_fragment_to_md(&tex_braced(src, i))),
        "href" => {
            let url  = tex_braced(src, i);
            let text = tex_fragment_to_md(&tex_braced(src, i));
            format!("[{}]({})", text, url)
        }
        "url" => format!("<{}>", tex_braced(src, i)),
        "footnote" => {
            let a = tex_braced(src, i);
            format!(" [^{}]", a.chars().take(20).collect::<String>())
        }
        // Citations: drop entirely (cleanest for prose). Consume optional [..] args.
        "cite" | "citep" | "citet" | "citeauthor" | "citeyear" | "citealt"
        | "citealp" | "citenum" | "Citep" | "Citet" => {
            let bytes = src.as_bytes();
            let len = src.len();
            while *i < len && bytes[*i] == b'[' {
                while *i < len && bytes[*i] != b']' { *i += 1; }
                if *i < len { *i += 1; }
            }
            tex_braced(src, i);
            String::new()
        }
        // References and labels: drop, emit nothing.
        "ref" | "eqref" | "autoref" | "cref" | "Cref" | "pageref" | "label"
        | "vref" | "nameref" => { tex_braced(src, i); String::new() }
        // Spelled-out symbols.
        "LaTeX" => "LaTeX".to_string(),
        "TeX" => "TeX".to_string(),
        "dots" | "ldots" | "cdots" => "\u{2026}".to_string(),
        "textbackslash" => "\\".to_string(),
        "textquotedblleft" => "\u{201C}".to_string(),
        "textquotedblright" => "\u{201D}".to_string(),
        _ => {
            // Special Latin letters with no argument (\l, \o, \ss, ...).
            if let Some(letter) = tex_simple_letter(word) {
                return Some(letter.to_string());
            }
            // Cedilla accent \c{c}: 'c' is a word, base follows in braces.
            if word == "c" || word == "v" || word == "u" || word == "H"
                || word == "r" || word == "d" || word == "b" || word == "k" {
                let bytes = src.as_bytes();
                if *i < src.len() && bytes[*i] == b'{' {
                    let base = tex_braced(src, i);
                    if let Some(bc) = base.chars().next() {
                        if let Some(c) = tex_accent_compose(word.chars().next().unwrap(), bc) {
                            return Some(c.to_string());
                        }
                        return Some(base);
                    }
                    return Some(String::new());
                }
                return None;
            }
            // Generic fallback: unknown \command{arg} -> keep arg, drop command.
            let bytes = src.as_bytes();
            if *i < src.len() && bytes[*i] == b'{' {
                return Some(tex_fragment_to_md(&tex_braced(src, i)));
            }
            // Unknown bare \command -> drop.
            return Some(String::new());
        }
    };
    Some(s)
}

/// Handle a backslash followed by a non-letter (escaped special, accent symbol,
/// line break, spacing). `i` points at the non-letter byte. Advances `i`.
/// Returns the markdown to emit (possibly empty).
fn tex_inline_symbol_to_md(src: &str, i: &mut usize) -> String {
    let bytes = src.as_bytes();
    let _len = src.len();
    let b = bytes[*i];
    match b {
        b'\\' => { *i += 1; "  \n".to_string() }       // \\ line break
        b'{' => { *i += 1; "{".to_string() }
        b'}' => { *i += 1; "}".to_string() }
        b'&' => { *i += 1; "&".to_string() }
        b'%' => { *i += 1; "%".to_string() }
        b'$' => { *i += 1; "$".to_string() }
        b'#' => { *i += 1; "#".to_string() }
        b'_' => { *i += 1; "_".to_string() }
        b'~' => { *i += 1; " ".to_string() }            // \~ outside accent: space
        b' ' => { *i += 1; " ".to_string() }            // \  control space
        // Accent symbols that take a base letter: \'e \`a \^o \"u \~n
        b'\'' | b'`' | b'^' | b'"' => {
            let accent = b as char;
            *i += 1;
            if let Some(base) = tex_accent_base(src, i) {
                if let Some(c) = tex_accent_compose(accent, base) {
                    c.to_string()
                } else {
                    base.to_string()
                }
            } else {
                String::new()
            }
        }
        b',' | b';' | b'!' | b'.' | b':' => { *i += 1; " ".to_string() } // spacing
        _ => {
            // Unknown escaped char: emit it literally (best effort).
            let ch = src[*i..].chars().next().unwrap_or('\u{FFFD}');
            *i += ch.len_utf8();
            ch.to_string()
        }
    }
    .to_string()
}

/// Convert plain-text punctuation ligatures used by LaTeX in prose:
/// `---` em dash, `--` en dash, ``` `` ``` left double quote, `''` right double
/// quote. Applied as a post-pass over already inline-converted text.
fn tex_apply_ligatures(s: &str) -> String {
    let s = s.replace("---", "\u{2014}");
    let s = s.replace("--", "\u{2013}");
    let s = s.replace("``", "\u{201C}");
    s.replace("''", "\u{201D}")
}

/// Convert a LaTeX prose fragment (NO preamble stripping, NO macro-comment
/// wrapping) to Markdown. Handles inline `$...$` math, braces, formatting
/// commands, citations/refs, accents and escaped specials. Block-level commands
/// (\section, \begin, ...) are not expected inside a fragment; if encountered
/// their text argument is kept via the generic fallback.
pub fn tex_fragment_to_md(fragment: &str) -> String {
    let bytes = fragment.as_bytes();
    let len = fragment.len();
    let mut i = 0usize;
    let mut out = String::new();

    while i < len {
        // Inline math $...$ (not $$)
        if bytes[i] == b'$' {
            if i + 1 < len && bytes[i + 1] == b'$' {
                i += 2;
                let start = i;
                while i + 1 < len && !(bytes[i] == b'$' && bytes[i + 1] == b'$') { i += 1; }
                let latex = fragment[start..i].trim();
                out.push_str(&format!("\n$$\n{}\n$$\n\n", latex));
                if i + 1 < len { i += 2; }
                continue;
            }
            i += 1;
            let start = i;
            while i < len && bytes[i] != b'$' { i += 1; }
            out.push_str(&format!("${}$", collapse_math_newlines(&fragment[start..i])));
            if i < len { i += 1; }
            continue;
        }
        if bytes[i] != b'\\' {
            match bytes[i] {
                b'{' | b'}' => { i += 1; }
                b if b < 0x80 => { out.push(b as char); i += 1; }
                _ => {
                    let ch = fragment[i..].chars().next().unwrap_or('\u{FFFD}');
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            continue;
        }
        // Backslash command
        i += 1;
        if i >= len { break; }
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < len && bytes[i].is_ascii_alphabetic() { i += 1; }
            let word = &fragment[start..i];
            // Skip optional whitespace / *
            while i < len && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'*') { i += 1; }
            if let Some(piece) = tex_inline_word_to_md(word, fragment, &mut i) {
                out.push_str(&piece);
            }
        } else {
            out.push_str(&tex_inline_symbol_to_md(fragment, &mut i));
        }
    }

    tex_apply_ligatures(&out)
}

fn tex_braced(src: &str, i: &mut usize) -> String {
    let bytes = src.as_bytes();
    let len   = src.len();
    while *i < len && (bytes[*i] == b' ' || bytes[*i] == b'\n' || bytes[*i] == b'\t') { *i += 1; }
    if *i >= len || bytes[*i] != b'{' { return String::new(); }
    *i += 1;
    let start = *i;
    let mut depth = 1i32;
    while *i < len {
        match bytes[*i] {
            b'{' => { depth += 1; *i += 1; }
            b'}' => { depth -= 1; *i += 1; if depth == 0 { break; } }
            _ => { *i += 1; }
        }
    }
    src[start..*i-1].to_string()
}

fn tex_until_end(src: &str, i: &mut usize, env: &str) -> String {
    let end_marker = format!("\\end{{{}}}", env);
    let rest = &src[*i..];
    if let Some(p) = rest.find(&end_marker) {
        let body = rest[..p].to_string();
        *i += p + end_marker.len();
        body
    } else {
        *i = src.len();
        rest.to_string()
    }
}

fn tex_list_to_md(body: &str, ordered: bool) -> String {
    let mut md = String::new();
    let mut n  = 0u32;
    for part in body.split("\\item") {
        let raw = part.trim();
        if raw.is_empty() { continue; }
        // Strip optional \item[label] - only if the string actually starts with '['
        let t = if raw.starts_with('[') {
            if let Some(end) = raw.find(']') { raw[end + 1..].trim() }
            else { raw }
        } else {
            raw
        };
        if t.is_empty() { continue; }
        // Inline-convert the item text so \textbf, \cite, accents etc. inside
        // list items are rendered, not emitted raw. Collapse the multi-line
        // item body into a single line for clean Markdown list output.
        let converted = tex_fragment_to_md(t);
        let line: String = converted.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() { continue; }
        if ordered { n += 1; md.push_str(&format!("{}. {}\n", n, line)); }
        else       { md.push_str(&format!("- {}\n", line)); }
    }
    md.push('\n');
    md
}

fn tex_extract_cmd(body: &str, cmd: &str) -> String {
    let search = format!("\\{}", cmd);
    if let Some(p) = body.find(&search) {
        let mut i = p + search.len();
        let bytes = body.as_bytes();
        if i < body.len() && bytes[i] == b'[' {
            while i < body.len() && bytes[i] != b']' { i += 1; }
            if i < body.len() { i += 1; }
        }
        return tex_braced(body, &mut i);
    }
    String::new()
}

// ── Shared inline helpers (used by org/rst/wiki/adoc/typ) ────────────────────

fn replace_delim_pair(s: &str, delim: &str, md: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    let mut open = false;
    while let Some(p) = rest.find(delim) {
        out.push_str(&rest[..p]);
        rest = &rest[p + delim.len()..];
        out.push_str(md);
        open = !open;
    }
    out.push_str(rest);
    out
}

fn regex_replace_inline(s: &str, open: &str, close: &str, md_open: &str, md_close: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find(open) {
        out.push_str(&rest[..p]);
        rest = &rest[p + open.len()..];
        if let Some(e) = rest.find(close) {
            out.push_str(md_open);
            out.push_str(&rest[..e]);
            out.push_str(md_close);
            rest = &rest[e + close.len()..];
        }
    }
    out.push_str(rest);
    out
}

fn xml_tag_to_inline(s: &str, tag: &str, open_md: &str, close_md: &str) -> String {
    let open_tag  = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find(&open_tag) {
        out.push_str(&rest[..p]);
        rest = &rest[p + open_tag.len()..];
        if let Some(e) = rest.find(&close_tag) {
            out.push_str(open_md);
            out.push_str(&rest[..e]);
            out.push_str(close_md);
            rest = &rest[e + close_tag.len()..];
        }
    }
    out.push_str(rest);
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// Emacs Org-mode (.org) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn org_to_md(content: &str) -> Result<String, String> {
    let mut md       = String::new();
    let mut in_src   = false;
    let mut in_quote = false;
    let mut in_example = false;

    for line in content.lines() {
        let t  = line.trim();
        let tl = t.to_lowercase();

        // Global directives
        if tl.starts_with("#+title:") {
            md.push_str(&format!("# {}\n\n", t.splitn(2,':').nth(1).unwrap_or("").trim()));
            continue;
        }
        if tl.starts_with("#+author:") {
            md.push_str(&format!("*{}*\n\n", t.splitn(2,':').nth(1).unwrap_or("").trim()));
            continue;
        }
        if tl.starts_with("#+date:") {
            md.push_str(&format!("*{}*\n\n", t.splitn(2,':').nth(1).unwrap_or("").trim()));
            continue;
        }
        // Skip other directives (but not begin/end)
        if t.starts_with("#+") && !tl.starts_with("#+begin") && !tl.starts_with("#+end") {
            continue;
        }

        // Block markers
        if tl.starts_with("#+begin_src") {
            let lang = t.split_whitespace().nth(1).unwrap_or("").to_lowercase();
            in_src = true;
            md.push_str(&format!("```{}\n", lang));
            continue;
        }
        if tl.starts_with("#+end_src") { in_src = false; md.push_str("```\n\n"); continue; }
        if tl.starts_with("#+begin_example") || tl.starts_with("#+begin_verbatim") {
            in_example = true; md.push_str("```\n"); continue;
        }
        if tl.starts_with("#+end_example") || tl.starts_with("#+end_verbatim") {
            in_example = false; md.push_str("```\n\n"); continue;
        }
        if tl.starts_with("#+begin_quote") || tl.starts_with("#+begin_abstract") {
            in_quote = true; continue;
        }
        if tl.starts_with("#+end_quote") || tl.starts_with("#+end_abstract") {
            in_quote = false; md.push('\n'); continue;
        }

        if in_src || in_example { md.push_str(line); md.push('\n'); continue; }
        if in_quote { md.push_str(&format!("> {}\n", org_inline(t))); continue; }

        // Headings: * ** *** (must have space after stars)
        let stars = t.bytes().take_while(|&b| b == b'*').count();
        if stars > 0 && stars < t.len() && t.as_bytes()[stars] == b' ' {
            let title = t[stars+1..].trim()
                .split_whitespace()
                .take_while(|w| !w.starts_with(':'))
                .collect::<Vec<_>>()
                .join(" ");
            md.push_str(&format!("{} {}\n\n", "#".repeat(stars.min(6)), title));
            continue;
        }

        if t == "-----" || t == "---" { md.push_str("---\n\n"); continue; }

        if t.starts_with("- ") || t.starts_with("+ ") {
            md.push_str(&format!("- {}\n", org_inline(&t[2..]))); continue;
        }
        // Ordered: N. or N)
        let digits = t.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 && t.len() > digits + 1 && (t.as_bytes()[digits] == b'.' || t.as_bytes()[digits] == b')') {
            md.push_str(&format!("1. {}\n", org_inline(t[digits+2..].trim()))); continue;
        }

        if t.is_empty() { md.push('\n'); continue; }
        md.push_str(&org_inline(t));
        md.push('\n');
    }

    Ok(collapse_blank_lines(&md))
}

fn org_inline(s: &str) -> String {
    // *bold* → **bold**  /italic/ → *italic*  =code= → `code`  ~verbatim~ → `verbatim`
    let s = org_replace_span(s, '*', "**");
    let s = org_replace_span(&s, '/', "*");
    let s = org_replace_span(&s, '=', "`");
    let s = org_replace_span(&s, '~', "`");
    // [[url][text]] or [[url]]
    let s = org_replace_links(&s);
    s
}

fn org_replace_span(s: &str, delim: char, md: &str) -> String {
    let mut out  = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == delim {
            // Org-mode word boundary rules:
            //   opening: preceded by whitespace / start / ( and followed by non-whitespace
            //   closing: preceded by non-whitespace
            let preceded_ok = i == 0
                || chars[i-1].is_whitespace()
                || "([{\"'".contains(chars[i-1]);
            let followed_ok = i + 1 < chars.len() && !chars[i+1].is_whitespace();

            if preceded_ok && followed_ok {
                if let Some(j) = chars[i+1..].iter().position(|&c| c == delim) {
                    let content: String = chars[i+1..i+1+j].iter().collect();
                    // Content must be non-empty, not surrounded by spaces, single-line,
                    // and not contain the pattern "://" (URL false positive)
                    if !content.is_empty()
                        && !content.starts_with(' ')
                        && !content.ends_with(' ')
                        && !content.contains('\n')
                        && !content.contains("://")
                    {
                        out.push_str(md);
                        out.push_str(&content);
                        out.push_str(md);
                        i += j + 2;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn org_replace_links(s: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("[[") {
        out.push_str(&rest[..p]);
        rest = &rest[p+2..];
        if let Some(e) = rest.find("]]") {
            let inner = &rest[..e];
            rest = &rest[e+2..];
            if let Some(sep) = inner.find("][") {
                out.push_str(&format!("[{}]({})", &inner[sep+2..], &inner[..sep]));
            } else {
                out.push_str(&format!("[{}]({})", inner, inner));
            }
        }
    }
    out.push_str(rest);
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// reStructuredText (.rst) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn rst_to_md(content: &str) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();
    let n = lines.len();
    let mut md  = String::new();
    let mut i   = 0;
    let mut heading_levels: Vec<char> = Vec::new();
    let heading_chars = ['=', '-', '~', '^', '"', '\'', '#', '+'];

    while i < n {
        let line = lines[i];
        let t    = line.trim();

        // Heading: check if next line is all the same underline char
        if i + 1 < n && !t.is_empty() {
            let next = lines[i+1].trim();
            if !next.is_empty() {
                let uc = next.chars().next().unwrap_or(' ');
                if heading_chars.contains(&uc) && next.len() >= t.len() && next.chars().all(|c| c == uc) {
                    let level = if let Some(p) = heading_levels.iter().position(|&c| c == uc) {
                        p + 1
                    } else { heading_levels.push(uc); heading_levels.len() };
                    md.push_str(&format!("{} {}\n\n", "#".repeat(level.min(6)), t));
                    i += 2;
                    continue;
                }
            }
        }

        // Directives: .. xxx::
        if t.starts_with(".. ") {
            let dir = &t[3..];
            if dir.starts_with("math::") {
                let body = rst_collect_indented(&lines, &mut i);
                md.push_str(&format!("\n$$\n{}\n$$\n\n", body.trim()));
                continue;
            }
            if dir.starts_with("code-block::") || dir.starts_with("code::") || dir.starts_with("sourcecode::") {
                let lang = dir.splitn(2,"::").nth(1).unwrap_or("").trim();
                let body = rst_collect_indented(&lines, &mut i);
                md.push_str(&format!("```{}\n{}\n```\n\n", lang, body.trim_end()));
                continue;
            }
            if dir.starts_with("figure::") || dir.starts_with("image::") {
                let path = dir.splitn(2,"::").nth(1).unwrap_or("").trim();
                let body = rst_collect_indented(&lines, &mut i);
                let alt = body.lines()
                    .find(|l| l.trim_start().starts_with(":alt:"))
                    .and_then(|l| l.splitn(3,':').nth(2))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                md.push_str(&format!("![{}]({})\n\n", alt, path));
                continue;
            }
            if dir.starts_with("note::") || dir.starts_with("warning::") || dir.starts_with("tip::") || dir.starts_with("important::") {
                let kind = dir.splitn(2,"::").next().unwrap_or("note");
                let body = rst_collect_indented(&lines, &mut i);
                md.push_str(&format!("> **{}:** {}\n\n", kind, body.trim()));
                continue;
            }
            // Skip other directives
            rst_collect_indented(&lines, &mut i);
            continue;
        }

        // Literal block ends with ::
        if t.ends_with("::") && t.len() > 2 {
            md.push_str(&format!("{}\n\n", rst_inline(&t[..t.len()-2])));
            let body = rst_collect_indented(&lines, &mut i);
            md.push_str(&format!("```\n{}\n```\n\n", body.trim_end()));
            continue;
        }
        if t == "::" {
            let body = rst_collect_indented(&lines, &mut i);
            md.push_str(&format!("```\n{}\n```\n\n", body.trim_end()));
            continue;
        }

        // Horizontal rule (4+ dashes on their own)
        if t.len() >= 4 && t.chars().all(|c| c == '-') { md.push_str("---\n\n"); i += 1; continue; }

        // Lists
        if t.starts_with("- ") || t.starts_with("* ") {
            md.push_str(&format!("- {}\n", rst_inline(&t[2..]))); i += 1; continue;
        }
        let dig = t.bytes().take_while(|b| b.is_ascii_digit()).count();
        if dig > 0 && t.len() > dig + 1 && t.as_bytes()[dig] == b'.' {
            md.push_str(&format!("1. {}\n", rst_inline(t[dig+1..].trim()))); i += 1; continue;
        }
        if t.starts_with("#. ") { md.push_str(&format!("1. {}\n", rst_inline(&t[3..]))); i += 1; continue; }

        if t.is_empty() { md.push('\n'); } else { md.push_str(&rst_inline(t)); md.push('\n'); }
        i += 1;
    }

    Ok(collapse_blank_lines(&md))
}

fn rst_collect_indented(lines: &[&str], i: &mut usize) -> String {
    *i += 1;
    while *i < lines.len() && lines[*i].trim().is_empty() { *i += 1; }
    let mut body = String::new();
    while *i < lines.len() {
        let l = lines[*i];
        if l.trim().is_empty() { body.push('\n'); *i += 1; }
        else if l.starts_with("   ") || l.starts_with('\t') {
            let stripped = if l.starts_with("   ") { &l[3..] } else { &l[1..] };
            body.push_str(stripped);
            body.push('\n');
            *i += 1;
        } else { break; }
    }
    body
}

fn rst_inline(s: &str) -> String {
    // ``code`` → `code`  (open="`\`", close="`\`" - safe because ``x`` pairs unambiguously)
    let s = regex_replace_inline(s, "``", "``", "`", "`");
    // :math:`expr` → $expr$
    let s = regex_replace_inline(&s, ":math:`", "`", "$", "$");
    // :code:`x` → `x`
    let s = regex_replace_inline(&s, ":code:`", "`", "`", "`");
    // :ref:`x` :class:`x` etc. - strip role, keep text
    let s = rst_strip_role(&s);
    s
}

fn rst_strip_role(s: &str) -> String {
    // Remove :rolename:`text` → text (for unknown roles)
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find(":`") {
        // Check if there's a :word before the backtick
        let before = &rest[..p];
        if let Some(colon) = before.rfind(':') {
            let role = &before[colon+1..];
            if role.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                out.push_str(&before[..colon]);
                rest = &rest[p+2..]; // skip :`
                if let Some(e) = rest.find('`') {
                    out.push_str(&rest[..e]);
                    rest = &rest[e+1..];
                }
                continue;
            }
        }
        out.push_str(&rest[..p+2]);
        rest = &rest[p+2..];
    }
    out.push_str(rest);
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// MediaWiki (.wiki) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn wiki_to_md(content: &str) -> Result<String, String> {
    let mut md     = String::new();
    let mut in_pre = false;

    for line in content.lines() {
        let t = line.trim();

        // Headings == Heading ==
        if t.starts_with('=') && t.ends_with('=') {
            let level = t.bytes().take_while(|&b| b == b'=').count().min(6);
            // Guard against a line made only of `=` (e.g. "==="): the closing
            // run would overlap the opening one and the slice would be invalid.
            let end = t.len().saturating_sub(level);
            if end > level {
                let title = t[level..end].trim();
                if !title.is_empty() {
                    md.push_str(&format!("{} {}\n\n", "#".repeat(level), title));
                    continue;
                }
            }
        }

        // Pre/source blocks
        if t.starts_with("<pre") || t.starts_with("<syntaxhighlight") || t.starts_with("<source") {
            in_pre = true; md.push_str("```\n"); continue;
        }
        if t.starts_with("</pre>") || t.starts_with("</syntaxhighlight>") || t.starts_with("</source>") {
            in_pre = false; md.push_str("```\n\n"); continue;
        }
        if in_pre { md.push_str(line); md.push('\n'); continue; }

        if t == "----" { md.push_str("---\n\n"); continue; }
        if t.starts_with("* ")  { md.push_str(&format!("- {}\n",   wiki_inline(&t[2..]))); continue; }
        if t.starts_with("** ") { md.push_str(&format!("  - {}\n", wiki_inline(&t[3..]))); continue; }
        if t.starts_with("# ")  { md.push_str(&format!("1. {}\n",  wiki_inline(&t[2..]))); continue; }
        if t.starts_with(": ")  { md.push_str(&format!("> {}\n",   wiki_inline(&t[2..]))); continue; }

        if t.is_empty() { md.push('\n'); }
        else { md.push_str(&wiki_inline(t)); md.push('\n'); }
    }

    Ok(collapse_blank_lines(&md))
}

fn wiki_inline(s: &str) -> String {
    let s = replace_delim_pair(s, "'''", "**");
    let s = replace_delim_pair(&s, "''", "*");
    let s = xml_tag_to_inline(&s, "math", "$", "$");
    let s = xml_tag_to_inline(&s, "code", "`", "`");
    let s = xml_tag_to_inline(&s, "ref",  "",  "");  // strip ref tags
    let s = replace_wiki_links(&s);
    strip_templates(&s)
}

fn replace_wiki_links(s: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("[[") {
        out.push_str(&rest[..p]);
        rest = &rest[p+2..];
        if let Some(e) = rest.find("]]") {
            let inner = &rest[..e];
            rest = &rest[e+2..];
            if let Some(sep) = inner.find('|') {
                out.push_str(&format!("[{}]({})", &inner[sep+1..], &inner[..sep]));
            } else {
                out.push_str(&format!("[{}]({})", inner, inner));
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_templates(s: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("{{") {
        out.push_str(&rest[..p]);
        rest = &rest[p+2..];
        if let Some(e) = rest.find("}}") { rest = &rest[e+2..]; }
    }
    out.push_str(rest);
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// AsciiDoc (.adoc) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn adoc_to_md(content: &str) -> Result<String, String> {
    let mut md       = String::new();
    let mut in_src   = false;
    let mut in_quote = false;
    let mut in_list  = false;
    let mut in_stem  = false;
    let mut stem_buf = String::new();
    let mut next_lang = String::new();

    for line in content.lines() {
        let t = line.trim();

        // Source annotation [source,lang] or [source.lang]
        if t.starts_with("[source") {
            let lang = t.split(',').nth(1)
                .or_else(|| t.split('.').nth(1))
                .unwrap_or("").trim_end_matches(']').trim();
            next_lang = lang.to_string();
            continue;
        }
        if t == "[stem]" || t == "[latexmath]" || t == "[asciimath]" { in_stem = true; continue; }

        // Block delimiters
        if t == "----" {
            if in_src { md.push_str("```\n\n"); in_src = false; }
            else { md.push_str(&format!("```{}\n", std::mem::take(&mut next_lang))); in_src = true; }
            continue;
        }
        if t == "====" {
            if in_quote { md.push('\n'); in_quote = false; }
            else { in_quote = true; }
            continue;
        }
        if t == "++++" {
            if in_stem {
                in_stem = false;
                md.push_str(&format!("\n$$\n{}\n$$\n\n", stem_buf.trim()));
                stem_buf.clear();
            }
            continue;
        }
        if t == "...." {
            if in_src { md.push_str("```\n\n"); in_src = false; }
            else { md.push_str("```\n"); in_src = true; }
            continue;
        }

        if in_src   { md.push_str(line); md.push('\n'); continue; }
        if in_stem  { stem_buf.push_str(line); stem_buf.push('\n'); continue; }
        if in_quote { md.push_str(&format!("> {}\n", adoc_inline(t))); continue; }

        // Skip AsciiDoc document attribute lines: :attr: or :attr: value
        // Examples: :toc:  :stem: latexmath  :author: Jane  :!numbered:
        if t.starts_with(':') && t.len() > 1 {
            let rest = &t[1..];
            if let Some(end) = rest.find(':') {
                let attr_name = &rest[..end];
                // Valid attribute names: alphanumeric, hyphens, underscores, leading !
                let name_clean = attr_name.trim_start_matches('!');
                if !name_clean.is_empty()
                   && name_clean.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    continue; // it's an attribute line
                }
            }
        }

        // Headings = to ======
        let eq_count = t.bytes().take_while(|&b| b == b'=').count();
        if eq_count > 0 && eq_count < t.len() && t.as_bytes()[eq_count] == b' ' {
            let title = t[eq_count+1..].trim();
            md.push_str(&format!("{} {}\n\n", "#".repeat(eq_count.min(6)), title));
            in_list = false;
            continue;
        }

        if t == "'''" || (t.len() >= 3 && t.chars().all(|c| c == '-')) {
            md.push_str("---\n\n"); continue;
        }

        // Lists * (bullets) and . (ordered)
        let star_count = t.bytes().take_while(|&b| b == b'*').count();
        if star_count > 0 && star_count < t.len() && t.as_bytes()[star_count] == b' ' {
            let indent = "  ".repeat(star_count.saturating_sub(1));
            md.push_str(&format!("{}- {}\n", indent, adoc_inline(t[star_count+1..].trim())));
            in_list = true;
            continue;
        }
        let dot_count = t.bytes().take_while(|&b| b == b'.').count();
        if dot_count > 0 && dot_count < t.len() && t.as_bytes()[dot_count] == b' ' {
            let indent = "  ".repeat(dot_count.saturating_sub(1));
            md.push_str(&format!("{}1. {}\n", indent, adoc_inline(t[dot_count+1..].trim())));
            in_list = true;
            continue;
        }

        // Admonitions - use break + bool so continue exits the OUTER loop
        let mut handled = false;
        for admon in &["NOTE", "TIP", "WARNING", "IMPORTANT", "CAUTION"] {
            let tag = format!("{}:", admon);
            if t.starts_with(&tag) {
                md.push_str(&format!("> **{}:** {}\n\n", admon, adoc_inline(t[tag.len()..].trim())));
                handled = true;
                break;
            }
        }
        if handled { continue; }

        // Images
        if t.starts_with("image::") {
            let rest = &t[7..];
            let path = rest.split('[').next().unwrap_or("").trim();
            let alt  = rest.find('[').and_then(|p| rest[p+1..].find(']').map(|e| &rest[p+1..p+1+e])).unwrap_or("");
            md.push_str(&format!("![{}]({})\n\n", alt, path));
            in_list = false;
            continue;
        }

        if t.is_empty() {
            if in_list { md.push('\n'); in_list = false; }
            else { md.push('\n'); }
        } else {
            md.push_str(&adoc_inline(t));
            md.push('\n');
        }
    }

    Ok(collapse_blank_lines(&md))
}

fn adoc_inline(s: &str) -> String {
    // **bold** or *bold* → **bold**
    let s = if s.contains("**") {
        regex_replace_inline(s, "**", "**", "**", "**")
    } else {
        replace_delim_pair(s, "*", "**")
    };
    // _italic_ → *italic*
    let s = replace_delim_pair(&s, "_", "*");
    // `code` stays the same
    // stem:[...] → $...$
    let s = regex_replace_inline(&s, "stem:[", "]", "$", "$");
    let s = regex_replace_inline(&s, "latexmath:[", "]", "$", "$");
    // link:url[text] → [text](url)
    replace_adoc_links(&s)
}

fn replace_adoc_links(s: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("link:") {
        out.push_str(&rest[..p]);
        rest = &rest[p+5..];
        let url_end = rest.find('[').unwrap_or(rest.len());
        let url = rest[..url_end].to_string();
        if url_end < rest.len() {
            rest = &rest[url_end+1..];
            if let Some(rb) = rest.find(']') {
                out.push_str(&format!("[{}]({})", &rest[..rb], url));
                rest = &rest[rb+1..];
                continue;
            }
        }
        out.push_str("link:");
        out.push_str(&url);
    }
    out.push_str(rest);
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// Typst source (.typ) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn typ_to_md(content: &str) -> Result<String, String> {
    let mut md     = String::new();
    let mut in_raw = false;
    let _in_math_block = false;
    let _math_buf = String::new();

    for line in content.lines() {
        let t = line.trim();

        // Raw code block ```lang
        if t.starts_with("```") && !in_raw {
            in_raw = true;
            md.push_str(line);
            md.push('\n');
            continue;
        }
        if t == "```" && in_raw {
            in_raw = false;
            md.push_str("```\n\n");
            continue;
        }
        if in_raw { md.push_str(line); md.push('\n'); continue; }

        // Skip Typst-specific directives
        if t.starts_with("#set ") || t.starts_with("#show ") || t.starts_with("#let ")
        || t.starts_with("#import") || t.starts_with("#include")
        || t.starts_with("#align(") || t.starts_with("#v(")
        || t.starts_with("#pagebreak") { continue; }

        // Headings = to ======
        let eq_count = t.bytes().take_while(|&b| b == b'=').count();
        if eq_count > 0 && eq_count < t.len() && t.as_bytes()[eq_count] == b' ' {
            let title = t[eq_count+1..].trim();
            if !title.is_empty() {
                md.push_str(&format!("{} {}\n\n", "#".repeat(eq_count.min(6)), title));
                continue;
            }
        }

        // Horizontal rule
        if t.starts_with("#line(") { md.push_str("---\n\n"); continue; }

        // Lists
        if t.starts_with("- ") { md.push_str(&format!("- {}\n", typ_inline(&t[2..]))); continue; }
        if t.starts_with("+ ") { md.push_str(&format!("1. {}\n", typ_inline(&t[2..]))); continue; }

        if t.is_empty() { md.push('\n'); }
        else { md.push_str(&typ_inline(t)); md.push('\n'); }
    }

    Ok(collapse_blank_lines(&md))
}

fn typ_inline(s: &str) -> String {
    // *bold* → **bold**
    let s = replace_delim_pair(s, "*", "**");
    // _italic_ → *italic*
    let s = replace_delim_pair(&s, "_", "*");
    // #strike[text] → ~~text~~
    let s = regex_replace_inline(&s, "#strike[", "]", "~~", "~~");
    // #underline[text] - no MD equivalent, keep as-is
    // #link("url")[text] → [text](url)
    typ_replace_link(&s)
}

fn typ_replace_link(s: &str) -> String {
    let mut out  = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("#link(\"") {
        out.push_str(&rest[..p]);
        rest = &rest[p+7..];
        if let Some(eu) = rest.find('"') {
            let url  = rest[..eu].to_string();
            rest = &rest[eu+1..];
            if rest.starts_with(")[") {
                rest = &rest[2..];
                if let Some(et) = rest.find(']') {
                    out.push_str(&format!("[{}]({})", &rest[..et], url));
                    rest = &rest[et+1..];
                    continue;
                }
            }
            out.push_str(&format!("<{}>", url));
        }
    }
    out.push_str(rest);
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// Jupyter Notebook (.ipynb) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

/// Convert a Jupyter Notebook JSON string to Markdown.
/// Markdown cells → as-is. Code cells → fenced code blocks with language.
/// Text outputs included as additional code blocks.
pub fn ipynb_to_md(content: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| format!("Jupyter JSON error: {}", e))?;

    let lang = v["metadata"]["kernelspec"]["language"]
        .as_str().unwrap_or("python").to_string();

    let cells = v["cells"].as_array()
        .ok_or("Jupyter: 'cells' array not found")?;

    let mut md = String::new();

    for cell in cells {
        let cell_type = cell["cell_type"].as_str().unwrap_or("raw");
        let source    = ipynb_join_source(&cell["source"]);

        match cell_type {
            "markdown" => {
                md.push_str(&source);
                if !source.ends_with('\n') { md.push('\n'); }
                md.push('\n');
            }
            "code" => {
                let src = source.trim_end();
                if !src.is_empty() {
                    md.push_str(&format!("```{}\n{}\n```\n\n", lang, src));
                }
                // Include stream / execute_result outputs as text blocks
                if let Some(outputs) = cell["outputs"].as_array() {
                    for out in outputs {
                        let ot = out["output_type"].as_str().unwrap_or("");
                        let text = match ot {
                            "stream" => ipynb_join_source(&out["text"]),
                            "execute_result" | "display_data" => ipynb_join_source(&out["data"]["text/plain"]),
                            _ => String::new(),
                        };
                        if !text.trim().is_empty() {
                            md.push_str(&format!("```\n{}\n```\n\n", text.trim_end()));
                        }
                    }
                }
            }
            _ => {} // raw cells: skip
        }
    }

    Ok(md)
}

fn ipynb_join_source(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a)  => a.iter().filter_map(|x| x.as_str()).collect(),
        _                            => String::new(),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// BibTeX (.bib) → Markdown reference list
// ═════════════════════════════════════════════════════════════════════════════

/// Convert BibTeX source to a Markdown reference list.
/// Each entry becomes a formatted bullet with bold title, author, year, venue.
pub fn bib_to_md(content: &str) -> Result<String, String> {
    let mut md      = String::new();
    let mut i       = 0usize;
    let bytes       = content.as_bytes();
    let len         = content.len();

    md.push_str("# References\n\n");

    while i < len {
        if bytes[i] != b'@' { i += 1; continue; }
        i += 1;
        let ts = i;
        while i < len && bytes[i] != b'{' && bytes[i] != b'(' { i += 1; }
        let entry_type = content[ts..i].trim().to_lowercase();
        if entry_type == "string" || entry_type == "preamble" || entry_type == "comment" {
            // skip to closing }
            if i < len { i += 1; }
            let mut d = 1i32;
            while i < len && d > 0 { match bytes[i] { b'{' => d += 1, b'}' => d -= 1, _ => {} } i += 1; }
            continue;
        }
        if i >= len { break; }
        i += 1; // skip {
        // Skip key
        while i < len && bytes[i] != b',' { i += 1; }
        if i < len { i += 1; }

        let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let depth = 1i32;
        loop {
            while i < len && bytes[i].is_ascii_whitespace() { i += 1; }
            if i >= len || depth <= 0 { break; }
            if bytes[i] == b'}' { i += 1; break; }

            let fs = i;
            while i < len && bytes[i] != b'=' && bytes[i] != b'}' { i += 1; }
            if i >= len || bytes[i] == b'}' { i += 1; break; }
            let fname = content[fs..i].trim().to_lowercase();
            i += 1; // skip =
            while i < len && bytes[i].is_ascii_whitespace() { i += 1; }

            let fval = if i < len && bytes[i] == b'{' {
                i += 1; let vs = i; let mut d = 1;
                while i < len { match bytes[i] { b'{' => d += 1, b'}' => { d -= 1; if d == 0 { break; } } _ => {} } i += 1; }
                let v = content[vs..i].replace('\n', " "); if i < len { i += 1; } v
            } else if i < len && bytes[i] == b'"' {
                i += 1; let vs = i;
                while i < len && bytes[i] != b'"' { i += 1; }
                let v = content[vs..i].to_string(); if i < len { i += 1; } v
            } else {
                let vs = i;
                while i < len && bytes[i] != b',' && bytes[i] != b'}' { i += 1; }
                content[vs..i].trim().to_string()
            };
            while i < len && (bytes[i] == b',' || bytes[i].is_ascii_whitespace()) { i += 1; }

            if !fname.trim().is_empty() {
                fields.insert(fname.trim().to_string(), fval.trim().to_string());
            }
        }

        let author  = fields.get("author").cloned().unwrap_or_default();
        let title   = fields.get("title").cloned().unwrap_or_default();
        let year    = fields.get("year").cloned().unwrap_or_default();
        let venue   = fields.get("journal").or_else(|| fields.get("booktitle"))
                            .cloned().unwrap_or_default();
        let pages   = fields.get("pages").cloned().unwrap_or_default();
        let volume  = fields.get("volume").cloned().unwrap_or_default();
        let url     = fields.get("url").cloned().unwrap_or_default();
        let doi     = fields.get("doi").cloned().unwrap_or_default();

        let author_fmt = bib_format_authors(&author);
        let mut entry = format!("- **{}**", title);
        if !author_fmt.is_empty() { entry.push_str(&format!(" - {}", author_fmt)); }
        if !year.is_empty()  { entry.push_str(&format!(" ({})", year)); }
        if !venue.is_empty() { entry.push_str(&format!(". *{}*", venue)); }
        if !volume.is_empty(){ entry.push_str(&format!(", **{}**", volume)); }
        if !pages.is_empty() { entry.push_str(&format!(", pp. {}", pages.replace("--", "-"))); }
        if !doi.is_empty()   { entry.push_str(&format!(". [doi:{}](https://doi.org/{})", doi, doi)); }
        else if !url.is_empty() { entry.push_str(&format!(". [Link]({})", url)); }
        entry.push('\n');
        md.push_str(&entry);
    }

    Ok(md)
}

fn bib_format_authors(authors: &str) -> String {
    authors.split(" and ")
        .map(|a| {
            let a = a.trim();
            if let Some(comma) = a.find(',') {
                let last  = a[..comma].trim();
                let first = a[comma+1..].trim();
                let init  = first.chars().next().map(|c| format!("{}.", c)).unwrap_or_default();
                format!("{} {}", last, init)
            } else { a.to_string() }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ═════════════════════════════════════════════════════════════════════════════
// FictionBook (.fb2) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn fb2_to_md(path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    fb2_xml_to_md(&content)
}

fn fb2_xml_to_md(xml: &str) -> Result<String, String> {
    let mut md    = String::new();
    let bytes     = xml.as_bytes();
    let len       = xml.len();
    let mut pos   = 0usize;
    let mut in_body   = false;
    let mut bold      = false;
    let mut italic    = false;
    let mut section_depth = 0u32;

    while pos < len {
        if bytes[pos] != b'<' {
            if in_body {
                let start = pos;
                while pos < len && bytes[pos] != b'<' { pos += 1; }
                md.push_str(&decode_entities(&xml[start..pos]));
            } else {
                while pos < len && bytes[pos] != b'<' { pos += 1; }
            }
            continue;
        }
        pos += 1;
        let ts = pos;
        while pos < len && bytes[pos] != b'>' { pos += 1; }
        let tag = &xml[ts..pos];
        if pos < len { pos += 1; }

        let closing  = tag.starts_with('/');
        let tag_body = if closing { tag[1..].trim() } else { tag.trim() };
        let tag_name = tag_body.split(|c: char| !c.is_alphanumeric() && c != ':')
                               .next().unwrap_or("").to_lowercase();

        match tag_name.as_str() {
            "body"    => { in_body = !closing; }
            "section" => {
                if !closing { section_depth += 1; }
                else { section_depth = section_depth.saturating_sub(1); md.push('\n'); }
            }
            "title"  => {
                if !closing { md.push_str(&"#".repeat(section_depth.max(1).min(6) as usize)); md.push(' '); }
                else { md.push_str("\n\n"); }
            }
            "p"      => { if closing { md.push_str("\n\n"); } }
            "strong" | "b" => {
                if !closing { bold = true; md.push_str("**"); }
                else if bold { bold = false; md.push_str("**"); }
            }
            "emphasis" | "i" | "em" => {
                if !closing { italic = true; md.push('*'); }
                else if italic { italic = false; md.push('*'); }
            }
            "code"       => { if !closing { md.push('`'); } else { md.push('`'); } }
            "poem"       => { if !closing { md.push_str("\n> "); } else { md.push_str("\n\n"); } }
            "epigraph"   => { if !closing { md.push_str("\n> "); } else { md.push_str("\n\n"); } }
            "cite"       => { if !closing { md.push_str("\n> "); } else { md.push_str("\n\n"); } }
            "v"          => { md.push_str("  \n"); }
            "subtitle"   => { if !closing { md.push('*'); } else { md.push_str("*\n\n"); } }
            "empty-line" => { md.push_str("\n\n"); }
            _ => {}
        }
    }

    Ok(collapse_blank_lines(&md))
}

// ═════════════════════════════════════════════════════════════════════════════
// PowerPoint (.pptx) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn pptx_to_md(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let file    = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("PPTX ZIP: {}", e))?;

    // Find all slide*.xml files (exclude _rels)
    let mut slide_names: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            let name = zip.by_index(i).ok()?.name().to_string();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
               && !name.contains("_rels") { Some(name) } else { None }
        })
        .collect();
    slide_names.sort_by_key(|n| {
        n.trim_start_matches("ppt/slides/slide")
         .trim_end_matches(".xml")
         .parse::<u32>().unwrap_or(0)
    });

    let mut md = String::new();

    for (idx, slide_name) in slide_names.iter().enumerate() {
        let xml = {
            let mut entry = zip.by_name(slide_name)
                .map_err(|e| format!("PPTX slide: {}", e))?;
            let mut s = String::new();
            entry.read_to_string(&mut s).map_err(|e| e.to_string())?;
            s
        };

        let (title, bullets) = pptx_extract_slide(&xml);
        let num = idx + 1;

        if !title.is_empty() {
            md.push_str(&format!("## Slide {}: {}\n\n", num, title));
        } else {
            md.push_str(&format!("## Slide {}\n\n", num));
        }

        for b in &bullets {
            let t = b.trim();
            if !t.is_empty() {
                if bullets.len() > 1 { md.push_str(&format!("- {}\n", t)); }
                else { md.push_str(&format!("{}\n\n", t)); }
            }
        }
        md.push('\n');
    }

    Ok(collapse_blank_lines(&md))
}

fn pptx_extract_slide(xml: &str) -> (String, Vec<String>) {
    let mut title   = String::new();
    let mut bullets : Vec<String> = Vec::new();
    let mut ph_type = String::new();
    let mut in_ph   = false;
    let mut para_text = String::new();
    let bytes = xml.as_bytes();
    let len   = xml.len();
    let mut pos = 0;

    while pos < len {
        if bytes[pos] != b'<' { pos += 1; continue; }
        pos += 1;
        let ts = pos;
        while pos < len && bytes[pos] != b'>' { pos += 1; }
        let tag = &xml[ts..pos];
        if pos < len { pos += 1; }
        let closing  = tag.starts_with('/');
        let tag_body = if closing { tag[1..].trim() } else { tag.trim() };
        let tag_name = tag_body.split(|c: char| !c.is_alphanumeric() && c != ':')
                               .next().unwrap_or("").to_lowercase();

        match tag_name.as_str() {
            "p:ph" => { ph_type = extract_xml_attr(tag, "type").unwrap_or_default(); in_ph = true; }
            "p:sp" => {
                if closing {
                    let t = para_text.trim().to_string();
                    if !t.is_empty() {
                        if ph_type == "title" || ph_type == "ctrTitle" { title = t; }
                        else { bullets.push(t); }
                    }
                    para_text.clear();
                    in_ph = false;
                }
            }
            "a:t"  => {
                if !closing && in_ph {
                    let end = xml[pos..].find("</a:t>").unwrap_or(0);
                    para_text.push_str(&decode_entities(&xml[pos..pos+end]));
                }
            }
            "a:p"  => { if closing && !para_text.trim().is_empty() { para_text.push('\n'); } }
            _ => {}
        }
    }

    (title, bullets)
}

// ═════════════════════════════════════════════════════════════════════════════
// Email (.eml) → Markdown
// ═════════════════════════════════════════════════════════════════════════════

pub fn eml_to_md(content: &str) -> Result<String, String> {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut current_key   = String::new();
    let mut current_val   = String::new();
    let mut in_body       = false;
    let mut body_lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        if !in_body {
            if line.is_empty() {
                if !current_key.is_empty() {
                    headers.push((std::mem::take(&mut current_key), std::mem::take(&mut current_val)));
                }
                in_body = true;
                continue;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                current_val.push(' ');
                current_val.push_str(line.trim());
            } else if let Some(colon) = line.find(':') {
                if !current_key.is_empty() {
                    headers.push((std::mem::take(&mut current_key), std::mem::take(&mut current_val)));
                }
                current_key = line[..colon].trim().to_string();
                current_val = line[colon+1..].trim().to_string();
            }
        } else {
            body_lines.push(line);
        }
    }
    if !current_key.is_empty() { headers.push((current_key, current_val)); }

    let subject = headers.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("subject"))
        .map(|(_, v)| v.as_str()).unwrap_or("(no subject)");
    let mut md = format!("# {}\n\n", subject);

    for (key, val) in &headers {
        let k = key.to_lowercase();
        if k == "from" || k == "to" || k == "cc" || k == "date" || k == "reply-to" {
            md.push_str(&format!("**{}:** {}  \n", key, val));
        }
    }
    md.push_str("\n---\n\n");

    let body = body_lines.join("\n");
    let ct = headers.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_lowercase())
        .unwrap_or_default();

    if ct.contains("text/html") {
        if let Ok(body_md) = html_to_md(&body) { md.push_str(&body_md); }
        else { md.push_str(&body); }
    } else {
        md.push_str(&body);
    }

    Ok(md)
}

#[cfg(test)]
mod import_utf8_tests {
    use super::{tex_to_md, tex_fragment_to_md, omml_to_latex};

    #[test]
    fn omml_nested_delimiter_keeps_content() {
        // Real OMML from a Word doc: d[ERV_a]/dt. The delimiter <m:d> wraps an
        // <m:e> that itself contains nested <m:e> (in the subscript), and <m:dPr>
        // has <m:endChr> whose name starts with "m:e". The old naive matcher lost
        // the content -> empty "( )". It must now survive.
        let xml = r#"<m:f><m:num><m:r><m:t>d</m:t></m:r><m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/></m:dPr><m:e><m:sSub><m:e><m:r><m:t>ERV</m:t></m:r></m:e><m:sub><m:r><m:t>a</m:t></m:r></m:sub></m:sSub></m:e></m:d></m:num><m:den><m:r><m:t>dt</m:t></m:r></m:den></m:f>"#;
        let latex = omml_to_latex(xml);
        assert!(latex.contains("ERV"), "delimiter content lost: {latex}");
        assert!(latex.contains("dt"), "denominator lost: {latex}");
        assert!(latex.contains("\\frac"), "fraction lost: {latex}");
        // Subscript captured (a) and not truncated.
        assert!(latex.contains('a'), "subscript lost: {latex}");
        // Balanced \left / \right (no dangling delimiter).
        assert_eq!(latex.matches("\\left").count(), latex.matches("\\right").count(),
            "unbalanced delimiters: {latex}");
    }

    #[test]
    fn tex_preserves_utf8_accents() {
        let src = "\\begin{document}\nSchr\u{F6}dinger caf\u{E9} \u{3B1}\u{3B2}\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(md.contains("Schr\u{F6}dinger"), "umlaut lost/mangled: {md}");
        assert!(md.contains("caf\u{E9}"), "accent lost: {md}");
        assert!(md.contains('\u{3B1}'), "greek alpha lost: {md}");
        // The byte-splitting bug produced A-tilde (U+00C3); must be gone.
        assert!(!md.contains('\u{C3}'), "UTF-8 split into mojibake: {md}");
    }

    #[test]
    fn tex_textbf_spanning_lines_keeps_bold() {
        // Regression: \textbf{...} spanning a soft line break in abstract/quote
        // used to be split per line -> "**very important an**" + lost bold.
        let src = "\\begin{document}\n\\begin{abstract}\n\
            We present a \\textbf{very important and\nmulti-line bold} result.\n\
            \\end{abstract}\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(md.contains("**very important and multi-line bold**"),
            "multi-line bold lost or split: {md:?}");
    }

    #[test]
    fn html_table_gets_gfm_delimiter_row() {
        // Without a delimiter row the pipe table is not valid GFM and renders as
        // plain text. html_to_md must synthesize one after the header.
        let html = "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>";
        let md = super::html_to_md(html).unwrap();
        let delim = md.lines().find(|l| {
            let t = l.trim();
            t.starts_with('|') && t.contains("---")
        });
        assert!(delim.is_some(), "no GFM delimiter row synthesized: {md:?}");
        // Two columns -> two --- groups.
        assert_eq!(delim.unwrap().matches("---").count(), 2, "wrong column count: {md:?}");
        // An existing delimiter (real GFM input) must not be duplicated.
        let already = "| A | B |\n| --- | --- |\n| 1 | 2 |\n";
        assert_eq!(super::fix_gfm_tables(already).matches("---").count(), 2);
    }

    #[test]
    fn tex_inline_math_spanning_lines_stays_one_line() {
        // Regression: inline $...$ that spans physical lines in LaTeX used to keep
        // the newline in Markdown -> the $...$ broke at the block boundary and a
        // continuation line starting with "- " became a bullet, leaving the `$`
        // literal in the exported HTML/PDF (would discredit a submitted paper).
        let src = "\\begin{document}\n\
            where $\\mathcal{L}[\\rho] = \\sum_n \\gamma_n\\left(L_n\\rho L_n^\\dagger\n\
            - \\frac{1}{2}\\{L_n, \\rho\\}\\right)$ with rates ok.\n\
            \\end{document}";
        let md = tex_to_md(src).unwrap();
        // The whole inline equation must be on a single line.
        let math_line = md.lines().find(|l| l.contains("\\mathcal{L}")).unwrap_or("");
        assert!(math_line.contains("\\right)$"), "inline math split across lines: {md:?}");
        // No continuation line may start with a bullet marker derived from "- ".
        assert!(!md.lines().any(|l| l.trim_start().starts_with("- \\frac")),
            "continuation line became a bullet: {md:?}");
    }

    #[test]
    fn tex_abstract_body_converts_inline() {
        let src = "\\begin{document}\n\\begin{abstract}\n\
            We use the \\textbf{exact} \\emph{only} formalism \\cite{Wallraff2004}.\n\
            \\end{abstract}\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(md.contains("**exact**"), "textbf not converted in abstract: {md}");
        assert!(md.contains("*only*"), "emph not converted in abstract: {md}");
        assert!(!md.contains("\\textbf"), "raw textbf survived: {md}");
        assert!(!md.contains("\\emph"), "raw emph survived: {md}");
    }

    #[test]
    fn tex_cite_is_dropped() {
        let src = "\\begin{document}\nText \\cite{Smith2020} more.\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(!md.contains("\\cite"), "raw cite survived: {md}");
        assert!(!md.contains("Smith2020"), "cite key leaked: {md}");
        assert!(md.contains("Text") && md.contains("more."), "surrounding text lost: {md}");
    }

    #[test]
    fn tex_special_letter_lslash() {
        let src = "\\begin{document}\nW\\l odek\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(md.contains('\u{0142}'), "\\l not converted to l-stroke: {md}");
    }

    #[test]
    fn tex_accent_acute_e() {
        let src = "\\begin{document}\ncaf\\'e\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(md.contains("caf\u{00E9}"), "\\'e not converted to e-acute: {md}");
    }

    #[test]
    fn tex_unknown_command_keeps_text() {
        let src = "\\begin{document}\n\\fakecmd{kept text} end\n\\end{document}";
        let md = tex_to_md(src).unwrap();
        assert!(md.contains("kept text"), "unknown command text dropped: {md}");
        assert!(!md.contains("\\fakecmd"), "raw unknown command survived: {md}");
    }

    #[test]
    fn tex_fragment_handles_texttt_and_escapes() {
        let out = tex_fragment_to_md("call \\texttt{[UNDERIVABLE]} at 50\\% load");
        assert!(out.contains("`[UNDERIVABLE]`"), "texttt not converted: {out}");
        assert!(out.contains("50% load"), "escaped percent not handled: {out}");
        assert!(!out.contains("\\texttt"), "raw texttt survived: {out}");
    }
}
