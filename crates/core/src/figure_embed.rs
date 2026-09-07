//! Shared helpers for embedding document figures (non-equation images) into
//! self-contained export targets: HTML `data:` URIs and DOCX media parts.
//!
//! Equation images have their own pipeline (`export_formats::render_eq_images`);
//! this module handles author-supplied `![](path)` figures so they survive
//! into portable exports instead of dangling as external file references.

use pulldown_cmark::{Event, Parser, Tag};
use std::path::{Path, PathBuf};

/// A resolved, re-encoded author figure ready to embed into a container export.
/// Keyed by the markdown `src` so body builders can match references back to it.
pub struct Figure {
    /// Path exactly as written in the markdown (`![](src)`).
    pub src: String,
    /// Re-encoded PNG bytes (uniform `image/png` across all containers).
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// Scan markdown for author figures, resolve each against `source_dir`, decode
/// and re-encode to PNG, and capture pixel dimensions. Remote / unreadable /
/// undecodable sources are skipped; the result is deduped by `src`.
///
/// Shared by every container exporter (DOCX / ODT / EPUB / RTF) so figure
/// handling stays uniform: resolve once here, embed per format's own packaging.
pub fn collect_figures(markdown: &str, source_dir: Option<&Path>) -> Vec<Figure> {
    let mut figs: Vec<Figure> = Vec::new();
    for event in Parser::new(markdown) {
        if let Event::Start(Tag::Image { dest_url, .. }) = event {
            let src = dest_url.to_string();
            if figs.iter().any(|f| f.src == src) {
                continue;
            }
            let Some(path) = resolve_local_image(&src, source_dir) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(img) = image::load_from_memory(&bytes) else { continue };
            let (w, h) = (img.width(), img.height());
            let mut png: Vec<u8> = Vec::new();
            if img
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .is_err()
            {
                continue;
            }
            figs.push(Figure { src, png, w, h });
        }
    }
    figs
}

/// Fit `(w, h)` px within `max_w` px, preserving aspect ratio. Returns the
/// display size in px (each exporter converts to its own unit: EMU, cm, twips).
pub fn fit_width(w: u32, h: u32, max_w: u32) -> (u32, u32) {
    let w = w.max(1);
    let h = h.max(1);
    if w <= max_w {
        (w, h)
    } else {
        (max_w, ((h as u64 * max_w as u64) / w as u64) as u32)
    }
}

/// Resolve an image `src` (as written in markdown/HTML) to a readable local
/// file path. Returns `None` for remote (`http(s)://`) or already-inlined
/// (`data:`) sources - callers leave those untouched.
pub fn resolve_local_image(src: &str, source_dir: Option<&Path>) -> Option<PathBuf> {
    let s = src.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return None;
    }

    // Strip a file:// scheme if present (file:///abs or file://host/path).
    let raw = s
        .strip_prefix("file:///")
        .or_else(|| s.strip_prefix("file://"))
        .unwrap_or(s);
    // Minimal percent-decoding for the common case of spaces.
    let raw = raw.replace("%20", " ");

    let p = Path::new(&raw);
    let candidate = if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(dir) = source_dir {
        dir.join(p)
    } else {
        p.to_path_buf()
    };

    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Best-effort MIME type from the file extension (defaults to `image/png`).
pub fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("tif") | Some("tiff") => "image/tiff",
        Some("svg") => "image/svg+xml",
        _ => "image/png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_and_data_sources_are_skipped() {
        assert!(resolve_local_image("https://example.com/a.png", None).is_none());
        assert!(resolve_local_image("data:image/png;base64,AAAA", None).is_none());
        assert!(resolve_local_image("", None).is_none());
    }

    #[test]
    fn mime_by_extension() {
        assert_eq!(image_mime(Path::new("a.PNG")), "image/png");
        assert_eq!(image_mime(Path::new("a.jpeg")), "image/jpeg");
        assert_eq!(image_mime(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(image_mime(Path::new("a.unknown")), "image/png");
    }
}
