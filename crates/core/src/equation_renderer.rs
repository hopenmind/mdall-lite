// Equation rasterizer - LaTeX → Typst math → PNG via typst-render.
// Uses Typst's embedded fonts (New Computer Modern Math) for correct
// OpenType MATH rendering - no system fonts required.

use comemo::Prehashed;
use typst::diag::{FileError, FileResult};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::visualize::Color;
use typst::{Library, World};

/// Render a LaTeX block equation to RGBA PNG bytes.
/// Returns `(png_bytes, None)` on success, `(None, Some(error))` on failure.
pub fn render_equation_png(latex: &str, scale: f32) -> (Option<Vec<u8>>, Option<String>) {
    let typst_math = crate::export_typst::latex_to_typst_math(latex);

    // Display math in Typst: `$ math $` (spaces = block/display mode).
    // Auto-sized page so the image fits the equation exactly.
    let source_str = format!(
        "#set page(width: auto, height: auto, margin: (x: 10pt, y: 8pt))\n\
         #set text(size: 13pt)\n\
         $ {} $\n",
        typst_math
    );

    let world = match EquationWorld::new(&source_str) {
        Some(w) => w,
        None => return (None, Some("Font loading failed".into())),
    };

    let mut tracer = Tracer::new();
    let doc = match typst::compile(&world, &mut tracer) {
        Ok(d) => d,
        Err(errs) => {
            let msgs: Vec<String> = errs.iter().map(|e| e.message.to_string()).collect();
            return (None, Some(format!("Typst: {}", msgs.join("; "))));
        }
    };

    let page = match doc.pages.first() {
        Some(p) => p,
        None => return (None, Some("No pages produced".into())),
    };

    let pixmap = typst_render::render(&page.frame, scale, Color::WHITE);
    match pixmap.encode_png() {
        Ok(bytes) => (Some(bytes), None),
        Err(e) => (None, Some(format!("PNG encode: {}", e))),
    }
}

// ── Minimal Typst world - embedded fonts only ─────────────────────────────

struct EquationWorld {
    library: Prehashed<Library>,
    book: Prehashed<FontBook>,
    fonts: Vec<Font>,
    source: Source,
}

impl EquationWorld {
    fn new(source_str: &str) -> Option<Self> {
        let mut book = FontBook::new();
        let mut fonts: Vec<Font> = Vec::new();

        // Load Typst's bundled fonts (includes New Computer Modern Math -
        // the OpenType MATH table font required for correct math rendering).
        for data in typst_assets::fonts() {
            let bytes = Bytes::from_static(data);
            for face_idx in 0u32.. {
                match Font::new(bytes.clone(), face_idx) {
                    Some(f) => {
                        book.push(f.info().clone());
                        fonts.push(f);
                    }
                    None => break,
                }
            }
        }

        if fonts.is_empty() {
            return None;
        }

        let source = Source::detached(source_str.to_string());

        Some(Self {
            library: Prehashed::new(Library::builder().build()),
            book: Prehashed::new(book),
            fonts,
            source,
        })
    }
}

impl World for EquationWorld {
    fn library(&self) -> &Prehashed<Library> { &self.library }
    fn book(&self) -> &Prehashed<FontBook> { &self.book }
    fn main(&self) -> Source { self.source.clone() }
    fn source(&self, id: FileId) -> FileResult<Source> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().to_path_buf()))
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().to_path_buf()))
    }
    fn font(&self, index: usize) -> Option<Font> { self.fonts.get(index).cloned() }
    fn today(&self, _offset: Option<i64>) -> Option<Datetime> { None }
}

/// Render a LaTeX equation to SVG via Typst.
/// Returns the SVG string on success, None on failure.
pub fn render_equation_svg(latex: &str) -> Option<String> {
    let typst_math = crate::export_typst::latex_to_typst_math(latex);
    let source_str = format!(
        "#set page(width: auto, height: auto, margin: (x: 10pt, y: 8pt))\n\
         #set text(size: 13pt)\n\
         $ {} $\n",
        typst_math
    );
    let world = EquationWorld::new(&source_str)?;
    let mut tracer = Tracer::new();
    let doc = typst::compile(&world, &mut tracer).ok()?;
    let page = doc.pages.first()?;
    Some(typst_svg::svg(&page.frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hostile / malformed / pathological LaTeX a user could type or paste. The
    /// renderer feeds Typst with arbitrary input on every keystroke, so it must
    /// never panic - it returns a value or an error, never aborts.
    fn fuzz_cases() -> Vec<&'static str> {
        vec![
            "", "   ", "{", "}", "\\frac{1}", "\\frac{}{}", "x^", "_y",
            "\\begin{matrix}", "\\undefinedcommand{z}", "\\left( a \\right",
            "a_{b_{c_{d_{e}}}}", "100%", "\\text{caf\\'e}", "\\\\\\\\",
        ]
    }

    #[test]
    fn render_png_never_panics_on_hostile_latex() {
        for src in fuzz_cases() {
            let (png, err) = render_equation_png(src, 2.0);
            assert!(
                png.is_some() || err.is_some(),
                "render_equation_png({src:?}) returned neither output nor error"
            );
            if let Some(bytes) = png {
                assert!(
                    bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n",
                    "output for {src:?} is not a real PNG"
                );
            }
        }
    }

    #[test]
    fn render_svg_never_panics_on_hostile_latex() {
        for src in fuzz_cases() {
            if let Some(svg) = render_equation_svg(src) {
                assert!(svg.contains("<svg"), "render_equation_svg({src:?}) is not SVG");
            }
        }
    }

    #[test]
    fn render_png_valid_equation_is_a_real_png() {
        let (png, err) = render_equation_png("E = mc^2", 2.0);
        assert!(png.is_some(), "valid equation failed to render: {err:?}");
        let bytes = png.unwrap();
        assert!(bytes.len() > 100 && &bytes[..8] == b"\x89PNG\r\n\x1a\n", "PNG signature/size wrong");
    }

    #[test]
    fn render_png_extreme_scale_does_not_panic() {
        // Scale is user-influenced; 0 and large values must not blow up.
        let _ = render_equation_png("x", 0.0);
        let _ = render_equation_png("x", 50.0);
    }
}
