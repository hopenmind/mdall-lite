use crate::render;
use genpdf::Element as _;
use serde::Deserialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const KATEX_CSS: &str = include_str!("../assets/katex/katex.embedded.css");

/// PDF engine preference. When `true`, the Native converter (pure-Rust Typst)
/// is used and the bundled rendering engine (Tier 1) is skipped. It is
/// process-global because it is a single user setting, not a per-call option:
/// the app sets it at startup and whenever the option changes.
static NATIVE_PDF: AtomicBool = AtomicBool::new(false);

/// Select the Native (pure-Rust) PDF converter instead of the General one.
pub fn set_native_pdf(on: bool) {
    NATIVE_PDF.store(on, Ordering::Relaxed);
}

/// Whether the Native (pure-Rust) PDF converter is currently selected.
pub fn native_pdf() -> bool {
    NATIVE_PDF.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PdfMetadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub keywords: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub creation_date: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub mod_date: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub license: String,
}

impl PdfMetadata {
    pub fn has_any(&self) -> bool {
        !self.title.is_empty() || !self.author.is_empty() || !self.subject.is_empty()
    }
}

/// Which PDF rendering tier produced the output. Lower tiers are progressively
/// less faithful: `Genpdf` renders equations as Unicode approximations, which is
/// not acceptable for journal submission, so the caller should warn on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfTier {
    Engine,
    Typst,
    Genpdf,
}

impl PdfTier {
    pub fn label(self) -> &'static str {
        match self {
            PdfTier::Engine => "rendering engine",
            PdfTier::Typst => "Typst",
            PdfTier::Genpdf => "basic fallback",
        }
    }

    /// True when equations are not rendered at full fidelity (a silent downgrade
    /// the user must be told about).
    pub fn is_degraded(self) -> bool {
        matches!(self, PdfTier::Genpdf)
    }
}

pub fn export_pdf(markdown: &str, output_path: &Path, metadata: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    export_pdf_with_tier(markdown, output_path, metadata, source_dir).map(|_| ())
}

/// Like [`export_pdf`] but reports which tier produced the file, so the caller can
/// warn the user when the output fell back to a lower-fidelity renderer.
pub fn export_pdf_with_tier(markdown: &str, output_path: &Path, metadata: &PdfMetadata, source_dir: Option<&Path>) -> Result<PdfTier, String> {
    // Resolve cross-references + citations before any tier renders the markdown.
    let prepared = crate::convert::apply_scholarly_passes(markdown, source_dir);
    let markdown = prepared.as_str();
    let mut tier_errors: Vec<String> = Vec::new();

    // Tier 1: bundled rendering engine (CDP, perfect KaTeX, zero system deps).
    // Skipped when the Native converter is selected, so PDF stays pure-Rust.
    if !native_pdf() {
        match crate::export_engine::export_pdf_engine(markdown, output_path, metadata, source_dir) {
            Ok(()) => return Ok(PdfTier::Engine),
            Err(e) => tier_errors.push(format!("Engine: {}", e)),
        }
    }

    // Tier 2: pure-Rust Typst backend (correct math rendering, no system deps)
    match crate::export_typst::export_pdf_typst(markdown, output_path, metadata, source_dir) {
        Ok(()) => return Ok(PdfTier::Typst),
        Err(e) => tier_errors.push(format!("Typst: {}", e)),
    }

    // Tier 3: genpdf fallback (unicode approximations - always works)
    match export_pdf_genpdf(markdown, output_path, metadata, source_dir) {
        Ok(()) => Ok(PdfTier::Genpdf),
        Err(e) => {
            tier_errors.push(format!("genpdf: {}", e));
            Err(format!("All PDF tiers failed - {}", tier_errors.join(" | ")))
        }
    }
}

// Kept as an alternative PDF path (spawn a system browser with --print-to-pdf),
// superseded by the bundled rendering-engine CDP tier but retained as an option.
#[allow(dead_code)]
fn find_browser() -> Option<std::path::PathBuf> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    candidates.iter()
        .map(std::path::Path::new)
        .find(|p| p.exists())
        .map(|p| p.to_path_buf())
}

#[allow(dead_code)]
fn export_pdf_via_browser(
    markdown: &str,
    output_path: &Path,
    metadata: &PdfMetadata,
    source_dir: Option<&Path>,
) -> Result<(), String> {
    let browser = find_browser().ok_or("No headless browser found")?;

    // Write HTML to a temp file - reuses the exact same pipeline as export_html()
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let tmp_path = std::env::temp_dir().join(format!("md2all_{}.html", ts));
    export_html(markdown, &tmp_path, metadata, source_dir)?;

    // Resolve absolute output path (file may not exist yet, so canonicalize the parent)
    let pdf_abs = {
        let parent = output_path.parent().unwrap_or(Path::new("."));
        let abs_parent = parent.canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(parent));
        let fname = output_path.file_name().unwrap_or_default();
        abs_parent.join(fname)
    };

    let tmp_url = format!("file:///{}",
        tmp_path.display().to_string().replace('\\', "/"));

    // Isolated user-data-dir: avoids conflicts when Edge is already open.
    let tmp_udd = std::env::temp_dir().join(format!("md2all_browser_{}", ts));
    let _ = std::fs::create_dir_all(&tmp_udd);
    let udd_arg = format!("--user-data-dir={}", tmp_udd.display());

    let pdf_arg = format!("--print-to-pdf={}", pdf_abs.display());

    let mut cmd = std::process::Command::new(&browser);
    cmd.args([
        "--headless=new",          // Edge 109+ / Chrome 112+ new headless (no visible window)
        "--disable-gpu",
        "--no-sandbox",
        "--disable-extensions",
        "--no-pdf-header-footer",
        "--run-all-compositor-stages-before-draw",
        "--virtual-time-budget=8000", // 8 s for KaTeX JS to render
        &udd_arg,
        &pdf_arg,
        &tmp_url,
    ]);

    // WIN32 CREATE_NO_WINDOW - prevents ANY Edge window from flashing on screen,
    // even when Edge ignores --headless or crashes internally.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let status = cmd.status().map_err(|e| format!("Browser launch error: {}", e))?;

    let _ = std::fs::remove_file(&tmp_path);
    let _ = std::fs::remove_dir_all(&tmp_udd);

    if !status.success() {
        return Err(format!("Browser exited with status {}", status));
    }
    if !pdf_abs.exists() || std::fs::metadata(&pdf_abs).map(|m| m.len() == 0).unwrap_or(true) {
        return Err("Browser produced no PDF output".to_string());
    }

    // Move to requested path if different (e.g. relative vs absolute resolution)
    if pdf_abs != output_path {
        std::fs::rename(&pdf_abs, output_path)
            .map_err(|e| format!("PDF move error: {}", e))?;
    }

    if metadata.has_any() {
        let _ = inject_pdf_metadata(output_path, metadata);
    }
    Ok(())
}

fn export_pdf_genpdf(markdown: &str, output_path: &Path, metadata: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    let fonts = load_pdf_fonts()?;
    let mut doc = genpdf::Document::new(fonts);
    doc.set_title(if metadata.title.is_empty() { "MD -> ALL Export" } else { &metadata.title });

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(20);
    doc.set_page_decorator(decorator);

    let processed = preprocess_equations(markdown);
    render_markdown_to_doc(&mut doc, &processed, source_dir);

    // Render to memory buffer first - never write empty/corrupt file
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut buf: Vec<u8> = Vec::new();
        doc.render(&mut buf).map(|_| buf)
    }));

    let buf = match result {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return Err(format!("PDF render error: {}", e)),
        Err(panic) => {
            let msg = panic.downcast_ref::<String>()
                .map(|s| s.as_str())
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            return Err(format!("PDF render crashed: {}", msg));
        }
    };

    if buf.is_empty() {
        return Err("PDF render produced empty output".to_string());
    }

    std::fs::write(output_path, &buf)
        .map_err(|e| format!("PDF write error: {}", e))?;

    if metadata.has_any() {
        let _ = inject_pdf_metadata(output_path, metadata);
    }
    Ok(())
}

pub fn export_html(markdown: &str, output_path: &Path, metadata: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    let prepared = crate::convert::apply_scholarly_passes(markdown, source_dir);
    let body_html = render::markdown_to_html(&prepared);
    // Inline author figures as base64 data URIs so the exported .html is fully
    // self-contained (portable by email/upload), not just viewable in place.
    let body_html = inline_local_images(&body_html, source_dir);
    let full_html = wrap_export_html(&body_html, metadata, source_dir);
    std::fs::write(output_path, full_html).map_err(|e| format!("Write error: {}", e))
}

/// Rewrite every `<img src="local/path">` in `html` to an embedded
/// `data:<mime>;base64,...` URI. Remote (`http(s)://`) and already-inlined
/// (`data:`) sources, and any path that fails to resolve or read, are left
/// untouched so the export never loses a reference it cannot embed.
fn inline_local_images(html: &str, source_dir: Option<&Path>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find("<img") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let end = match after.find('>') {
            Some(e) => e + 1,
            None => {
                out.push_str(after);
                return out;
            }
        };
        out.push_str(&rewrite_img_tag(&after[..end], source_dir));
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn rewrite_img_tag(tag: &str, source_dir: Option<&Path>) -> String {
    use base64::Engine as _;
    let lower = tag.to_ascii_lowercase();
    let Some(key) = lower.find("src=\"") else { return tag.to_string() };
    let val_start = key + 5;
    let Some(rel_end) = tag[val_start..].find('"') else { return tag.to_string() };
    let val_end = val_start + rel_end;
    let src = &tag[val_start..val_end];

    let Some(path) = crate::figure_embed::resolve_local_image(src, source_dir) else {
        return tag.to_string();
    };
    let Ok(data) = std::fs::read(&path) else { return tag.to_string() };
    let mime = crate::figure_embed::image_mime(&path);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    format!(
        "{}data:{};base64,{}{}",
        &tag[..val_start],
        mime,
        b64,
        &tag[val_end..]
    )
}

fn load_pdf_fonts() -> Result<genpdf::fonts::FontFamily<genpdf::fonts::FontData>, String> {
    let try_load = |r: &str, b: &str, i: &str, bi: &str| -> Option<genpdf::fonts::FontFamily<genpdf::fonts::FontData>> {
        Some(genpdf::fonts::FontFamily {
            regular: genpdf::fonts::FontData::new(std::fs::read(r).ok()?, None).ok()?,
            bold: genpdf::fonts::FontData::new(std::fs::read(b).ok()?, None).ok()?,
            italic: genpdf::fonts::FontData::new(std::fs::read(i).ok()?, None).ok()?,
            bold_italic: genpdf::fonts::FontData::new(std::fs::read(bi).ok()?, None).ok()?,
        })
    };

    if let Some(f) = try_load(
        r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\segoeuii.ttf", r"C:\Windows\Fonts\segoeuiz.ttf",
    ) { return Ok(f); }

    if let Some(f) = try_load(
        r"C:\Windows\Fonts\arial.ttf", r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\ariali.ttf", r"C:\Windows\Fonts\arialbi.ttf",
    ) { return Ok(f); }

    Err("No suitable font found in C:\\Windows\\Fonts".to_string())
}

fn escape_md_equation(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '_' | '*' | '[' | ']' | '~' | '`' => { out.push('\\'); out.push(ch); }
            _ => out.push(ch),
        }
    }
    out
}

fn preprocess_equations(markdown: &str) -> String {
    let mut result = String::new();
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

        if line.trim_start().starts_with("$$") && !in_equation {
            let after = line.trim_start().trim_start_matches("$$").trim();
            if !after.is_empty() && after.ends_with("$$") {
                let content = after.trim_end_matches("$$").trim();
                result.push('\n');
                result.push_str(&escape_md_equation(&render::latex_to_unicode(content)));
                result.push_str("\n\n");
                i += 1;
                continue;
            }
            in_equation = true;
            equation_buf.clear();
            if !after.is_empty() {
                equation_buf.push_str(after);
            }
            i += 1;
            continue;
        }

        if in_equation {
            if line.trim() == "$$" || line.trim().ends_with("$$") {
                let before = line.trim().trim_end_matches("$$").trim();
                if !before.is_empty() {
                    if !equation_buf.is_empty() { equation_buf.push('\n'); }
                    equation_buf.push_str(before);
                }
                in_equation = false;
                result.push('\n');
                result.push_str(&escape_md_equation(&render::latex_to_unicode(&equation_buf)));
                result.push_str("\n\n");
            } else {
                if !equation_buf.is_empty() { equation_buf.push('\n'); }
                equation_buf.push_str(line.trim());
            }
            i += 1;
            continue;
        }

        result.push_str(&pdf_inline_math(line));
        result.push('\n');
        i += 1;
    }

    if in_equation && !equation_buf.is_empty() {
        result.push('\n');
        result.push_str(&escape_md_equation(&render::latex_to_unicode(&equation_buf)));
        result.push_str("\n\n");
    }

    result
}

fn pdf_inline_math(line: &str) -> String {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut result = String::new();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'$' {
            if i + 1 < len && bytes[i + 1] == b'$' {
                result.push('$');
                result.push('$');
                i += 2;
                continue;
            }
            if i + 1 >= len {
                result.push('$');
                i += 1;
                continue;
            }
            let next = bytes[i + 1];
            if next == b' ' || next == b'\n' || next == b'\t' {
                result.push('$');
                i += 1;
                continue;
            }
            let start = i + 1;
            let mut j = start;
            let mut found = false;
            while j < len {
                if bytes[j] == b'$' && j > start {
                    found = true;
                    break;
                }
                j += 1;
            }
            if found {
                let math = &line[start..j];
                result.push_str(&escape_md_equation(&render::latex_to_unicode(math)));
                i = j + 1;
            } else {
                result.push('$');
                i += 1;
            }
        } else {
            let ch = line[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
    }
    result
}

fn render_markdown_to_doc(doc: &mut genpdf::Document, markdown: &str, source_dir: Option<&Path>) {
    use pulldown_cmark::{Event, Tag, TagEnd, Options, Parser, HeadingLevel};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut text_buf = String::new();
    let mut heading_level: Option<HeadingLevel> = None;
    let mut in_code_block = false;
    let mut in_list = false;
    let mut ordered_list = false;
    let mut list_num = 0u64;
    let mut pending_image: Option<String> = None;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                pdf_flush(&mut text_buf, doc);
                heading_level = Some(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                let size: u8 = match heading_level.take() {
                    Some(HeadingLevel::H1) => 20,
                    Some(HeadingLevel::H2) => 16,
                    Some(HeadingLevel::H3) => 14,
                    _ => 12,
                };
                let text = std::mem::take(&mut text_buf);
                if !text.trim().is_empty() {
                    doc.push(genpdf::elements::Paragraph::new(text.trim().to_string())
                        .styled(genpdf::style::Style::new().bold().with_font_size(size)));
                    doc.push(genpdf::elements::Break::new(0.5));
                }
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                let text = std::mem::take(&mut text_buf);
                if !text.trim().is_empty() {
                    if in_list {
                        let prefix = if ordered_list { list_num += 1; format!("  {}. ", list_num) }
                                     else { "  \u{2022} ".to_string() };
                        doc.push(genpdf::elements::Paragraph::new(format!("{}{}", prefix, text.trim())));
                    } else {
                        doc.push(genpdf::elements::Paragraph::new(text.trim().to_string()));
                        doc.push(genpdf::elements::Break::new(0.3));
                    }
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                pdf_flush(&mut text_buf, doc);
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                let code = std::mem::take(&mut text_buf);
                for line in code.lines() {
                    doc.push(genpdf::elements::Paragraph::new(line.to_string())
                        .styled(genpdf::style::Style::new().with_font_size(8)));
                }
                doc.push(genpdf::elements::Break::new(0.3));
            }
            Event::Start(Tag::BlockQuote(_)) => { pdf_flush(&mut text_buf, doc); }
            Event::End(TagEnd::BlockQuote(_)) => {
                let text = std::mem::take(&mut text_buf);
                if !text.trim().is_empty() {
                    doc.push(genpdf::elements::Paragraph::new(format!("\u{2502} {}", text.trim()))
                        .styled(genpdf::style::Style::new().italic()));
                    doc.push(genpdf::elements::Break::new(0.3));
                }
            }
            Event::Start(Tag::List(first)) => {
                pdf_flush(&mut text_buf, doc);
                in_list = true;
                ordered_list = first.is_some();
                list_num = first.unwrap_or(1).saturating_sub(1);
            }
            Event::End(TagEnd::List(_)) => {
                in_list = false;
                doc.push(genpdf::elements::Break::new(0.3));
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                pdf_flush(&mut text_buf, doc);
                pending_image = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Image) => {
                if let Some(dest) = pending_image.take() {
                    let mut embedded = false;
                    if let Some(dir) = source_dir {
                        let img_path = dir.join(&dest);
                        if img_path.exists() {
                            match genpdf::elements::Image::from_path(&img_path) {
                                Ok(img) => {
                                    doc.push(img);
                                    embedded = true;
                                }
                                Err(e) => {
                                    doc.push(genpdf::elements::Paragraph::new(
                                        format!("[Image load error: {} - {}]", img_path.display(), e)));
                                    embedded = true;
                                }
                            }
                        } else {
                            doc.push(genpdf::elements::Paragraph::new(
                                format!("[Image not found: {}]", img_path.display())));
                            embedded = true;
                        }
                    }
                    if !embedded {
                        doc.push(genpdf::elements::Paragraph::new(
                            format!("[Image: {} - no source directory]", dest)));
                    }
                    let alt = std::mem::take(&mut text_buf);
                    if !alt.trim().is_empty() {
                        doc.push(genpdf::elements::Paragraph::new(alt.trim().to_string())
                            .styled(genpdf::style::Style::new().italic().with_font_size(9)));
                    }
                    doc.push(genpdf::elements::Break::new(0.3));
                }
            }
            Event::End(TagEnd::TableRow) => {
                let text = std::mem::take(&mut text_buf);
                if !text.is_empty() {
                    doc.push(genpdf::elements::Paragraph::new(text));
                }
            }
            Event::End(TagEnd::TableCell) => { text_buf.push_str(" | "); }
            Event::Text(t) => text_buf.push_str(t.as_ref()),
            Event::Code(c) => { text_buf.push('`'); text_buf.push_str(c.as_ref()); text_buf.push('`'); }
            Event::SoftBreak => text_buf.push(if in_code_block { '\n' } else { ' ' }),
            Event::HardBreak => text_buf.push('\n'),
            Event::Rule => {
                pdf_flush(&mut text_buf, doc);
                doc.push(genpdf::elements::Paragraph::new("\u{2500}".repeat(60)));
                doc.push(genpdf::elements::Break::new(0.3));
            }
            _ => {}
        }
    }
    pdf_flush(&mut text_buf, doc);
}

fn pdf_flush(buf: &mut String, doc: &mut genpdf::Document) {
    let text = std::mem::take(buf);
    if !text.trim().is_empty() {
        doc.push(genpdf::elements::Paragraph::new(text.trim().to_string()));
    }
}

fn inject_pdf_metadata(pdf_path: &Path, metadata: &PdfMetadata) -> Result<(), String> {
    use lopdf::{Document, Object, StringFormat};

    let mut doc =
        Document::load(pdf_path).map_err(|e| format!("PDF load error: {}", e))?;

    let now = chrono::Local::now();
    let pdf_date = format!(
        "D:{}",
        now.format("%Y%m%d%H%M%S%:z")
            .to_string()
            .replace(':', "'")
            + "'"
    );

    let mut info = lopdf::Dictionary::new();

    if !metadata.title.is_empty() {
        info.set("Title", Object::String(metadata.title.as_bytes().to_vec(), StringFormat::Literal));
    }
    if !metadata.author.is_empty() {
        info.set("Author", Object::String(metadata.author.as_bytes().to_vec(), StringFormat::Literal));
    }
    if !metadata.subject.is_empty() {
        info.set("Subject", Object::String(metadata.subject.as_bytes().to_vec(), StringFormat::Literal));
    }

    let mut kw_parts = Vec::new();
    if !metadata.keywords.is_empty() { kw_parts.push(metadata.keywords.clone()); }
    if !metadata.doi.is_empty() { kw_parts.push(format!("DOI:{}", metadata.doi)); }
    if !metadata.license.is_empty() { kw_parts.push(format!("License:{}", metadata.license)); }
    if !kw_parts.is_empty() {
        info.set("Keywords", Object::String(kw_parts.join("; ").as_bytes().to_vec(), StringFormat::Literal));
    }

    info.set("Creator", Object::String(b"MD -> ALL v3.0".to_vec(), StringFormat::Literal));
    info.set("Producer", Object::String(b"MD -> ALL (Rust/KaTeX)".to_vec(), StringFormat::Literal));
    info.set("CreationDate", Object::String(pdf_date.as_bytes().to_vec(), StringFormat::Literal));
    info.set("ModDate", Object::String(pdf_date.as_bytes().to_vec(), StringFormat::Literal));

    // Custom metadata in Subject
    let mut custom = Vec::new();
    if !metadata.timestamp.is_empty() { custom.push(format!("Timestamp: {}", metadata.timestamp)); }
    if !metadata.signature.is_empty() { custom.push(format!("Signed-By: {}", metadata.signature)); }
    if !metadata.version.is_empty() { custom.push(format!("Doc-Version: {}", metadata.version)); }
    if !custom.is_empty() {
        let existing = metadata.subject.clone();
        let full = if existing.is_empty() {
            custom.join(" | ")
        } else {
            format!("{} [{}]", existing, custom.join(" | "))
        };
        info.set("Subject", Object::String(full.as_bytes().to_vec(), StringFormat::Literal));
    }

    let info_id = doc.add_object(Object::Dictionary(info));
    doc.trailer.set("Info", Object::Reference(info_id));
    doc.save(pdf_path).map_err(|e| format!("PDF save error: {}", e))?;

    Ok(())
}

fn wrap_export_html(body_html: &str, metadata: &PdfMetadata, source_dir: Option<&Path>) -> String {
    let title = if metadata.title.is_empty() { "MD -> ALL Export" } else { &metadata.title };
    let lang = if metadata.lang.is_empty() { "en" } else { &metadata.lang };

    let base_tag = match source_dir {
        Some(dir) => {
            let mut url = dir.display().to_string().replace('\\', "/");
            if !url.ends_with('/') { url.push('/'); }
            format!("<base href=\"file:///{}\">\n", url)
        }
        None => String::new(),
    };

    let mut meta_tags = String::new();
    if !metadata.author.is_empty() {
        meta_tags.push_str(&format!("<meta name=\"author\" content=\"{}\">\n", esc(&metadata.author)));
    }
    if !metadata.doi.is_empty() {
        meta_tags.push_str(&format!("<meta name=\"citation_doi\" content=\"{}\">\n", esc(&metadata.doi)));
    }

    let footer = build_footer(metadata);

    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="UTF-8">
{base_tag}<title>{title}</title>
{meta_tags}
<style>
{katex_css}
body {{ font-family: 'Segoe UI', Tahoma, sans-serif; max-width: 850px; margin: 0 auto; padding: 40px 24px; line-height: 1.7; color: #1a1a1a; }}
h1 {{ font-size: 2em; border-bottom: 2px solid #e0e0e0; padding-bottom: .3em; margin-top: 1.5em; }}
h2 {{ font-size: 1.5em; border-bottom: 1px solid #e8e8e8; padding-bottom: .2em; margin-top: 1.3em; }}
h3 {{ font-size: 1.25em; margin-top: 1.2em; }}
code {{ background: #f4f4f4; padding: 2px 6px; border-radius: 3px; font-family: Consolas, monospace; font-size: .9em; }}
pre {{ background: #f8f8f8; border: 1px solid #e0e0e0; border-radius: 6px; padding: 16px; overflow-x: auto; }}
pre code {{ background: none; padding: 0; }}
blockquote {{ border-left: 4px solid #4a9eff; margin: 1em 0; padding: .5em 1em; color: #555; background: #f9fbff; }}
table {{ border-collapse: collapse; width: 100%; margin: 1em 0; }}
th, td {{ border: 1px solid #ddd; padding: 8px 12px; text-align: left; }}
th {{ background: #f4f4f4; font-weight: 600; }}
img {{ max-width: 100%; height: auto; border-radius: 4px; }}
hr {{ border: none; border-top: 1px solid #e0e0e0; margin: 2em 0; }}
a {{ color: #4a9eff; }}
.eq-block {{ text-align: center; margin: 1.2em 0; overflow-x: auto; }}
.eq-inline {{ }}
.eq-error-block, .eq-error-inline {{ color: #c00; font-family: monospace; font-size: .9em; }}
.doc-footer {{ margin-top: 3em; padding-top: 1em; border-top: 1px solid #ddd; font-size: .85em; color: #666; }}
@media print {{ body {{ max-width: 100%; padding: 20px; }} }}
</style>
</head>
<body>
{body_html}
{footer}
</body>
</html>"#,
        lang = lang,
        title = esc(title),
        base_tag = base_tag,
        meta_tags = meta_tags,
        katex_css = KATEX_CSS,
        body_html = body_html,
        footer = footer,
    )
}

fn build_footer(m: &PdfMetadata) -> String {
    let mut parts = Vec::new();
    if !m.signature.is_empty() { parts.push(format!("<b>Signed:</b> {}", esc(&m.signature))); }
    if !m.timestamp.is_empty() { parts.push(format!("<b>Timestamp:</b> {}", esc(&m.timestamp))); }
    if !m.doi.is_empty() { parts.push(format!("<b>DOI:</b> {}", esc(&m.doi))); }
    if !m.version.is_empty() { parts.push(format!("<b>Version:</b> {}", esc(&m.version))); }
    if !m.license.is_empty() { parts.push(format!("<b>License:</b> {}", esc(&m.license))); }
    if parts.is_empty() { String::new() } else { format!("<div class=\"doc-footer\">{}</div>", parts.join(" · ")) }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}



