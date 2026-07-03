// Source embedding in image files and document archives.
//
// Three layers of reversibility:
//   1. PNG  → tEXt ancillary chunk  (keyword = "MD-TO-ALL:latex")
//   2. SVG  → <metadata> / CDATA    (xmlns:m = "https://hopenmind.com/md-to-all/ns#")
//   3. DOCX → custom ZIP entry      ("md-to-all-source.xml" - full markdown)
//
// All formats remain valid, standards-compliant files.  The source data
// is "invisible cargo": Word / browsers / image viewers ignore it.

// ── CRC-32 (PNG chunk integrity) ─────────────────────────────────────────────

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = make_crc_table();

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC_TABLE[idx];
    }
    !crc
}

// ── PNG ───────────────────────────────────────────────────────────────────────

/// PNG file signature (8 bytes).
const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
/// tEXt chunk keyword - max 79 chars per PNG spec.
const PNG_KEYWORD: &[u8] = b"MD-TO-ALL:latex";

/// Inject a `tEXt` chunk into *png* carrying the LaTeX source.
/// The chunk is inserted right after the mandatory IHDR chunk.
/// Returns the original bytes unchanged if the input is not a valid PNG.
pub fn embed_latex_in_png(png: &[u8], latex: &str) -> Vec<u8> {
    // Validate signature + IHDR presence (8 sig + 4 len + 4 type + 13 data + 4 crc = 33).
    if png.len() < 33 || &png[..8] != PNG_SIG {
        return png.to_vec();
    }

    // Build tEXt chunk data: keyword NUL text
    let mut chunk_data: Vec<u8> = Vec::new();
    chunk_data.extend_from_slice(PNG_KEYWORD);
    chunk_data.push(0x00);
    chunk_data.extend_from_slice(latex.as_bytes());

    // CRC covers chunk type + chunk data
    let mut crc_input: Vec<u8> = b"tEXt".to_vec();
    crc_input.extend_from_slice(&chunk_data);
    let crc = crc32(&crc_input);

    let mut chunk: Vec<u8> = Vec::with_capacity(12 + chunk_data.len());
    chunk.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&chunk_data);
    chunk.extend_from_slice(&crc.to_be_bytes());

    // Inject right after the IHDR chunk (at byte offset 33).
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..33]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[33..]);
    out
}

/// Scan a PNG byte stream for a `tEXt` chunk with keyword `MD-TO-ALL:latex`.
/// Returns the embedded LaTeX source string, or `None` if not found.
pub fn extract_latex_from_png(png: &[u8]) -> Option<String> {
    if png.len() < 33 || &png[..8] != PNG_SIG {
        return None;
    }
    let mut pos = 8usize; // skip signature
    loop {
        if pos + 8 > png.len() {
            break;
        }
        let length = u32::from_be_bytes(png[pos..pos + 4].try_into().ok()?) as usize;
        let chunk_type = &png[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start + length;
        if data_end + 4 > png.len() {
            break;
        }
        if chunk_type == b"tEXt" {
            let data = &png[data_start..data_end];
            if let Some(sep) = data.iter().position(|&b| b == 0) {
                if &data[..sep] == PNG_KEYWORD {
                    return String::from_utf8(data[sep + 1..].to_vec()).ok();
                }
            }
        }
        if chunk_type == b"IEND" {
            break;
        }
        pos = data_end + 4; // advance past CRC
    }
    None
}

// ── SVG ───────────────────────────────────────────────────────────────────────

const SVG_NS_PREFIX: &str = "m";
const SVG_NS_URI: &str = "https://hopenmind.com/md-to-all/ns#";

/// Inject a `<metadata>` element with the LaTeX source (CDATA) into an SVG string.
/// Inserted immediately after the closing `>` of the root `<svg` opening tag.
pub fn embed_latex_in_svg(svg: &str, latex: &str) -> String {
    // Find the end of the root <svg ...> opening tag.
    let tag_end = match svg.find('>') {
        Some(p) => p + 1,
        None => return svg.to_string(),
    };

    // Escape ]]> in the LaTeX source (cannot appear inside a CDATA section).
    // Split across adjacent CDATA sections.
    let escaped = latex.replace("]]>", "]]]]><![CDATA[>");

    let metadata = format!(
        "<metadata xmlns:{p}=\"{ns}\"><{p}:latex><![CDATA[{src}]]></{p}:latex></metadata>",
        p = SVG_NS_PREFIX,
        ns = SVG_NS_URI,
        src = escaped,
    );

    format!("{}{}{}", &svg[..tag_end], metadata, &svg[tag_end..])
}

/// Extract the LaTeX source embedded by `embed_latex_in_svg`.
/// Returns `None` if no embedded source is found.
pub fn extract_latex_from_svg(svg: &str) -> Option<String> {
    let open_tag = format!("<{}:latex><![CDATA[", SVG_NS_PREFIX);
    let close_tag = format!("]]></{p}:latex>", p = SVG_NS_PREFIX);

    let start = svg.find(&open_tag)? + open_tag.len();
    let end = svg[start..].find(&close_tag)? + start;

    // Unescape split CDATA sections used for ]]> in source.
    let raw = &svg[start..end];
    Some(raw.replace("]]]]><![CDATA[>", "]]>"))
}

// ── DOCX custom ZIP entry ─────────────────────────────────────────────────────

/// The ZIP entry name used to embed the full markdown source inside a DOCX.
pub const DOCX_SOURCE_ENTRY: &str = "md-to-all-source.xml";

/// Build the XML content of the custom source entry.
pub fn build_source_xml(markdown: &str) -> String {
    // Escape ]]> (cannot appear in CDATA section).
    let escaped = markdown.replace("]]>", "]]]]><![CDATA[>");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <source xmlns=\"https://hopenmind.com/md-to-all/2024/docx\"\n\
                 generator=\"md-to-all\"\n\
                 format=\"markdown/commonmark\">\n\
           <content><![CDATA[{}]]></content>\n\
         </source>\n",
        escaped
    )
}

/// Extract the markdown source from the XML content of a custom source entry.
pub fn parse_source_xml(xml: &str) -> Option<String> {
    let open = "<content><![CDATA[";
    let close = "]]></content>";
    let start = xml.find(open)? + open.len();
    let end = xml[start..].find(close)? + start;
    let raw = &xml[start..end];
    Some(raw.replace("]]]]><![CDATA[>", "]]>"))
}

// ── DOCX round-trip import ────────────────────────────────────────────────────

/// Prepended to a reconstructed (non-lossless) recovery so the user knows the
/// result is incomplete before they overwrite their `.md` source. It is a valid
/// Markdown comment, so it is invisible in the rendered document but visible in
/// the source, and it survives a save.
pub const PARTIAL_RECOVERY_WARNING: &str = "<!-- MD -> ALL: partial recovery. The full editable source was not found in this DOCX (Word may have re-saved it). Headings, text and equations were reconstructed from the document and its equation images; other formatting may be missing. Review before overwriting your .md source. -->\n\n";

/// Attempt to extract the original markdown source from a DOCX file.
/// Returns just the Markdown; see [`import_docx_source_detailed`] for the
/// fidelity flag.
pub fn import_docx_source(path: &std::path::Path) -> Result<String, String> {
    import_docx_source_detailed(path).map(|(md, _)| md)
}

/// Recover the Markdown from a DOCX and report whether it was a lossless recovery.
///
/// Strategy, in order of fidelity:
///   1. The `md-to-all-source.xml` ZIP entry - the exact original source (lossless).
///   2. Reconstruct from `word/document.xml` plus per-equation LaTeX recovered
///      from the Word comments AND the PNG `tEXt` / SVG `<metadata>` of the
///      equation images in `word/media` (recovery layers 2 and 3). This survives
///      Word "Save As" stripping the custom entry, but is not lossless.
///
/// The boolean is `true` only for strategy 1.
pub fn import_docx_source_detailed(path: &std::path::Path) -> Result<(String, bool), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    // Strategy 1 - embedded source entry (lossless).
    if let Ok(mut entry) = archive.by_name(DOCX_SOURCE_ENTRY) {
        use std::io::Read;
        let mut xml = String::new();
        entry.read_to_string(&mut xml).map_err(|e| e.to_string())?;
        drop(entry);
        if let Some(md) = parse_source_xml(&xml) {
            return Ok((md, true));
        }
    }

    // Strategy 2 - lossy reconstruction (document text + equations from comments
    // and from the equation-image metadata). Reopen for a fresh read pass.
    drop(archive);
    let file2 = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive2 = zip::ZipArchive::new(file2).map_err(|e| e.to_string())?;
    let md = reconstruct_lossy(&mut archive2)?;
    Ok((format!("{}{}", PARTIAL_RECOVERY_WARNING, md), false))
}

/// Recover per-equation LaTeX embedded by this tool in the DOCX media parts:
/// PNG `tEXt` chunks (layer 2) and SVG `<metadata>` (layer 3). Used when the
/// Word comments were stripped, so equations are not lost with them. Returns the
/// equations in a deterministic (file-name) order.
fn recover_equations_from_media(archive: &mut zip::ZipArchive<std::fs::File>) -> Vec<String> {
    use std::io::Read;
    let mut names: Vec<String> = archive
        .file_names()
        .filter(|n| {
            let l = n.to_lowercase();
            l.starts_with("word/media/") && (l.ends_with(".png") || l.ends_with(".svg"))
        })
        .map(|s| s.to_string())
        .collect();
    names.sort();

    let mut equations = Vec::new();
    for name in names {
        let is_png = name.to_lowercase().ends_with(".png");
        if let Ok(mut entry) = archive.by_name(&name) {
            if is_png {
                let mut buf = Vec::new();
                if entry.read_to_end(&mut buf).is_ok() {
                    if let Some(latex) = extract_latex_from_png(&buf) {
                        equations.push(latex);
                    }
                }
            } else {
                let mut svg = String::new();
                if entry.read_to_string(&mut svg).is_ok() {
                    if let Some(latex) = extract_latex_from_svg(&svg) {
                        equations.push(latex);
                    }
                }
            }
        }
    }
    equations
}

/// Reconstruct a best-effort markdown document from `word/document.xml`, using
/// equations recovered from the Word comments where present, and from the
/// equation-image metadata otherwise so they are never lost with the comments.
fn reconstruct_lossy(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<String, String> {
    let not_ours = || {
        "No MD -> ALL source found in this DOCX.\n\
         Only DOCX files exported by MD -> ALL can be re-imported."
            .to_string()
    };
    let doc_xml = read_zip_entry_string(archive, "word/document.xml").map_err(|_| not_ours())?;

    // Equations from comments are position-aware (mapped by comment id).
    let comments_xml = read_zip_entry_string(archive, "word/comments.xml").unwrap_or_default();
    let equations = extract_latex_from_comments_xml(&comments_xml);

    let mut out = reconstruct_markdown(&doc_xml, &equations);

    // If the comments were stripped, recover the equations from the image
    // metadata (layers 2/3) and append them so they survive at all.
    if equations.is_empty() {
        for latex in recover_equations_from_media(archive) {
            out.push_str("\n$$\n");
            out.push_str(&latex);
            out.push_str("\n$$\n");
        }
    }

    if out.trim().is_empty() {
        return Err(not_ours());
    }
    Ok(out)
}

fn read_zip_entry_string(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<String, String> {
    use std::io::Read;
    let mut entry = archive.by_name(name).map_err(|e| e.to_string())?;
    let mut s = String::new();
    entry.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

/// Parse `word/comments.xml` and return a map of comment-id → LaTeX source.
fn extract_latex_from_comments_xml(xml: &str) -> std::collections::HashMap<u32, String> {
    let mut map = std::collections::HashMap::new();

    // Pattern: <w:comment w:id="N" w:author="MD-TO-ALL" ...>
    //            <w:p><w:r><w:t>LaTeX: SOURCE</w:t></w:r></w:p>
    //          </w:comment>
    let mut search: &str = xml;
    while let Some(p) = search.find("<w:comment ") {
        search = &search[p..];
        // Extract w:id
        let id_opt = attr_value(search, "w:id");
        // Check author
        let is_ours = attr_value(search, "w:author")
            .map(|a| a == "MD-TO-ALL")
            .unwrap_or(false);

        if is_ours {
            if let Some(id_str) = id_opt {
                if let Ok(id) = id_str.parse::<u32>() {
                    // Extract text content until </w:comment>
                    let close = "</w:comment>";
                    if let Some(end) = search.find(close) {
                        let block = &search[..end + close.len()];
                        let text = extract_w_t_text(block);
                        if let Some(latex) = text.strip_prefix("LaTeX: ") {
                            map.insert(id, latex.to_string());
                        }
                    }
                }
            }
        }

        // Advance past the opening tag to avoid infinite loop.
        if let Some(next) = search[1..].find("<w:comment ") {
            search = &search[1 + next..];
        } else {
            break;
        }
    }
    map
}

/// Extract the text content of the first `<w:t>` element found in `xml`.
fn extract_w_t_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(p) = rest.find("<w:t") {
        rest = &rest[p..];
        // Skip to > (end of opening tag)
        if let Some(tag_end) = rest.find('>') {
            rest = &rest[tag_end + 1..];
            if let Some(close) = rest.find("</w:t>") {
                out.push_str(&rest[..close]);
                rest = &rest[close + 6..];
            }
        } else {
            break;
        }
    }
    out
}

/// Extract an XML attribute value (handles both `attr="value"` and `attr='value'`).
fn attr_value<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{}=\"", attr);
    if let Some(p) = xml.find(&needle) {
        let start = p + needle.len();
        let rest = &xml[start..];
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    let needle2 = format!("{}='", attr);
    if let Some(p) = xml.find(&needle2) {
        let start = p + needle2.len();
        let rest = &xml[start..];
        let end = rest.find('\'')?;
        return Some(&rest[..end]);
    }
    None
}

/// Reconstruct a markdown document from `word/document.xml` and a LaTeX equation map.
///
/// This is a best-effort reconstruction: headings, bold, italic, equations.
/// Annotations added by the Word user (tracked changes, comments) are ignored.
fn reconstruct_markdown(
    doc_xml: &str,
    equations: &std::collections::HashMap<u32, String>,
) -> String {
    let mut out = String::new();
    let mut rest = doc_xml;

    // Walk through <w:p> paragraphs in document order.
    while let Some(p_start) = rest.find("<w:p>").or_else(|| rest.find("<w:p ")) {
        rest = &rest[p_start..];
        let p_end = match rest.find("</w:p>") {
            Some(e) => e + 6,
            None => break,
        };
        let para = &rest[..p_end];
        rest = &rest[p_end..];

        // Detect heading level from <w:pStyle w:val="Heading1"> etc.
        let heading_level = detect_heading(para);

        // Detect equation comment reference → emit $$latex$$ block.
        let eq_ids = collect_comment_references(para);

        // Collect plain text from <w:t> elements.
        let text = extract_w_t_text(para).trim().to_string();

        // Emit equations referenced in this paragraph.
        for id in &eq_ids {
            if let Some(latex) = equations.get(id) {
                out.push('\n');
                out.push_str("$$\n");
                out.push_str(latex);
                out.push_str("\n$$\n");
            }
        }

        // Emit text paragraph (only if non-empty and not just an equation paragraph).
        if !text.is_empty() && !text.starts_with("Eq. ") {
            if let Some(level) = heading_level {
                out.push('\n');
                for _ in 0..level { out.push('#'); }
                out.push(' ');
                out.push_str(&text);
                out.push('\n');
            } else {
                out.push('\n');
                out.push_str(&text);
                out.push('\n');
            }
        }
    }

    out.trim_start().to_string()
}

fn detect_heading(para_xml: &str) -> Option<usize> {
    // <w:pStyle w:val="Heading1"> / Heading2 / Heading3
    let style = attr_value(para_xml, "w:val")?;
    match style {
        "Heading1" => Some(1),
        "Heading2" => Some(2),
        "Heading3" => Some(3),
        _ => None,
    }
}

fn collect_comment_references(para_xml: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    let mut rest = para_xml;
    while let Some(p) = rest.find("<w:commentReference ") {
        rest = &rest[p..];
        if let Some(id_str) = attr_value(rest, "w:id") {
            if let Ok(id) = id_str.parse::<u32>() {
                ids.push(id);
            }
        }
        rest = &rest[1..];
    }
    ids
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip() {
        // Minimal valid PNG: 1×1 white pixel (IHDR + IDAT + IEND).
        let minimal_png: &[u8] = &[
            0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A, // signature
            0x00,0x00,0x00,0x0D, // IHDR length = 13
            0x49,0x48,0x44,0x52, // IHDR
            0x00,0x00,0x00,0x01,0x00,0x00,0x00,0x01, // 1×1
            0x08,0x02,0x00,0x00,0x00, // 8-bit RGB, no interlace
            0x90,0x77,0x53,0xDE, // CRC
            // IEND
            0x00,0x00,0x00,0x00,
            0x49,0x45,0x4E,0x44,
            0xAE,0x42,0x60,0x82,
        ];
        let source = r"\sum_{i=0}^{n} x_i";
        let embedded = embed_latex_in_png(minimal_png, source);
        let extracted = extract_latex_from_png(&embedded);
        assert_eq!(extracted.as_deref(), Some(source));
    }

    #[test]
    fn svg_round_trip() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><path d="M0,0"/></svg>"#;
        let source = r"\frac{a}{b}";
        let embedded = embed_latex_in_svg(svg, source);
        let extracted = extract_latex_from_svg(&embedded);
        assert_eq!(extracted.as_deref(), Some(source));
    }

    #[test]
    fn svg_escapes_cdata_end_sequence() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        let source = r"x]]>y"; // pathological case
        let embedded = embed_latex_in_svg(svg, source);
        let extracted = extract_latex_from_svg(&embedded);
        assert_eq!(extracted.as_deref(), Some(source));
    }

    #[test]
    fn source_xml_round_trip() {
        let md = "# Title\n\nSome **bold** text with $E=mc^2$.\n";
        let xml = build_source_xml(md);
        let recovered = parse_source_xml(&xml);
        assert_eq!(recovered.as_deref(), Some(md));
    }

    #[test]
    fn partial_recovery_recovers_media_equation_and_flags_it() {
        // Simulate a DOCX that Word "Saved As": the custom source entry and the
        // comments are gone, but the equation PNG (with embedded LaTeX) remains.
        // Recovery must fall back to layers 2/3, recover the equation, and flag
        // the result as partial so the user does not overwrite their source.
        use std::io::Write;
        use zip::{write::SimpleFileOptions, ZipWriter};

        let minimal_png: &[u8] = &[
            0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A,
            0x00,0x00,0x00,0x0D, 0x49,0x48,0x44,0x52,
            0x00,0x00,0x00,0x01,0x00,0x00,0x00,0x01,
            0x08,0x02,0x00,0x00,0x00, 0x90,0x77,0x53,0xDE,
            0x00,0x00,0x00,0x00, 0x49,0x45,0x4E,0x44, 0xAE,0x42,0x60,0x82,
        ];
        let png = embed_latex_in_png(minimal_png, r"\alpha + \beta");

        let dir = std::env::temp_dir().join(format!("mdall_se_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let docx = dir.join("stripped.docx");
        {
            let file = std::fs::File::create(&docx).unwrap();
            let mut z = ZipWriter::new(file);
            let opts = SimpleFileOptions::default();
            z.start_file("word/document.xml", opts).unwrap();
            z.write_all(b"<w:document><w:body><w:p><w:r><w:t>Hello world</w:t></w:r></w:p></w:body></w:document>").unwrap();
            z.start_file("word/media/image1.png", opts).unwrap();
            z.write_all(&png).unwrap();
            z.finish().unwrap();
        }

        let (md, full) = import_docx_source_detailed(&docx).expect("recovery failed");
        assert!(!full, "a stripped DOCX must report partial (lossy) recovery");
        assert!(md.contains("partial recovery"), "missing partial-recovery warning: {md}");
        assert!(md.contains("Hello world"), "lost document prose: {md}");
        assert!(md.contains(r"\alpha + \beta"), "equation not recovered from media: {md}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
