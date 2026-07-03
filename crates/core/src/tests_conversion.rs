//! Conversion safety harness - S1.
//!
//! Goal: PROVE that conversions never destroy or corrupt the user's content.
//! Each test is focused so `cargo test` output pinpoints exactly which of the
//! 13 export formats and the import path are genuinely safe vs. which are theater.
//!
//! What "safe" means here:
//!   - An export never panics and produces a NON-EMPTY, STRUCTURALLY VALID file.
//!   - Office formats (DOCX/ODT/EPUB) are valid ZIP archives with required entries.
//!   - The DOCX lossless round-trip recovers the EXACT original Markdown (headline claim).
//!   - Edge inputs (empty, unicode, huge, malformed) never panic.
//!
//! These run inside the bin crate so they can reach `pub(crate)` items.

#![cfg(test)]

use crate::export::{self, PdfMetadata};
use crate::export_formats;
use crate::source_embed;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

// ── Helpers ────────────────────────────────────────────────────────────────

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique temp path for a test artifact (no Date/random needed - pid + counter).
fn tmp(name: &str, ext: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("mdall_{}_{}_{}.{}", std::process::id(), name, n, ext))
}

fn meta() -> PdfMetadata {
    let mut m = PdfMetadata::default();
    m.title = "Document de Test".into();
    m.author = "MD -> ALL test harness".into();
    m
}

/// Rich, equation-FREE sample - exercises the structural breadth a researcher uses.
/// (Equation-free so the bulk validity tests stay fast and don't invoke Typst.)
fn sample() -> &'static str {
    r#"# Titre Principal

Paragraphe avec **gras**, *italique*, `code inline`, et un [lien](https://example.com).

## Sous-section

- item un
- item deux avec **gras**

1. ordonne A
2. ordonne B

> Citation importante du document.

```rust
fn main() { println!("hello"); }
```

| Colonne A | Colonne B |
|-----------|-----------|
| valeur 1  | valeur 2  |

Fin du document de test.
"#
}

/// Sample WITH equations - used only for equation-preservation tests.
fn sample_eq() -> &'static str {
    "# Equations\n\nInline $E = mc^2$ dans le texte.\n\n$$\n\\frac{1}{2} \\sum_{i=0}^{n} x_i\n$$\n\nFin.\n"
}

/// Assert a file exists and is non-empty; return its bytes.
fn read_nonempty(path: &PathBuf, ctx: &str) -> Vec<u8> {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: file not created ({})", ctx, e));
    assert!(!bytes.is_empty(), "{}: file is EMPTY - user would get a blank document", ctx);
    bytes
}

/// Assert the file at `path` is a valid ZIP archive containing all `required` entries.
fn assert_valid_zip(path: &PathBuf, required: &[&str], ctx: &str) {
    let file = fs::File::open(path).unwrap_or_else(|e| panic!("{}: open failed ({})", ctx, e));
    let mut zip = zip::ZipArchive::new(file)
        .unwrap_or_else(|e| panic!("{}: NOT a valid ZIP - file is corrupt ({})", ctx, e));
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    for req in required {
        assert!(
            names.iter().any(|n| n == req || n.ends_with(req)),
            "{}: missing required ZIP entry '{}' - archive incomplete. Present: {:?}",
            ctx, req, names
        );
    }
}

fn cleanup(path: &PathBuf) {
    let _ = fs::remove_file(path);
}

// ── Office formats: valid ZIP structure (won't hand the user a corrupt file) ──

#[test]
fn export_docx_is_valid_office_zip() {
    let out = tmp("docx_valid", "docx");
    export_formats::export_docx(sample(), &out, &meta(), None).expect("export_docx returned Err");
    read_nonempty(&out, "DOCX");
    assert_valid_zip(&out, &["[Content_Types].xml", "word/document.xml"], "DOCX");
    cleanup(&out);
}

#[test]
fn export_odt_is_valid_office_zip() {
    let out = tmp("odt_valid", "odt");
    export_formats::export_odt(sample(), &out, &meta(), None).expect("export_odt returned Err");
    read_nonempty(&out, "ODT");
    assert_valid_zip(&out, &["content.xml"], "ODT");
    cleanup(&out);
}

#[test]
fn export_epub_is_valid_zip() {
    let out = tmp("epub_valid", "epub");
    export_formats::export_epub(sample(), &out, &meta(), None).expect("export_epub returned Err");
    read_nonempty(&out, "EPUB");
    // EPUB OCF: must contain mimetype + META-INF/container.xml
    assert_valid_zip(&out, &["mimetype", "META-INF/container.xml"], "EPUB");
    cleanup(&out);
}

// ── Text-based formats: non-empty + contain the actual content ────────────────

fn assert_text_contains(bytes: &[u8], needles: &[&str], ctx: &str) {
    let text = String::from_utf8_lossy(bytes);
    for n in needles {
        assert!(
            text.contains(n),
            "{}: output is missing expected content '{}' - silent content loss", ctx, n
        );
    }
}

#[test]
fn export_txt_keeps_content() {
    let out = tmp("txt", "txt");
    export_formats::export_txt(sample(), &out).expect("export_txt returned Err");
    let b = read_nonempty(&out, "TXT");
    assert_text_contains(&b, &["Titre Principal", "item un", "Citation"], "TXT");
    cleanup(&out);
}

#[test]
fn export_tex_keeps_content() {
    let out = tmp("tex", "tex");
    export_formats::export_tex(sample(), &out, &meta()).expect("export_tex returned Err");
    let b = read_nonempty(&out, "TeX");
    assert_text_contains(&b, &["documentclass", "Titre Principal"], "TeX");
    cleanup(&out);
}

#[test]
fn export_rtf_keeps_content() {
    let out = tmp("rtf", "rtf");
    export_formats::export_rtf(sample(), &out, &meta(), None).expect("export_rtf returned Err");
    let b = read_nonempty(&out, "RTF");
    assert_text_contains(&b, &["\\rtf1", "Titre"], "RTF");
    cleanup(&out);
}

#[test]
fn export_org_keeps_content() {
    let out = tmp("org", "org");
    export_formats::export_org(sample(), &out, &meta()).expect("export_org returned Err");
    let b = read_nonempty(&out, "Org");
    assert_text_contains(&b, &["Titre Principal"], "Org");
    cleanup(&out);
}

#[test]
fn export_rst_keeps_content() {
    let out = tmp("rst", "rst");
    export_formats::export_rst(sample(), &out, &meta()).expect("export_rst returned Err");
    let b = read_nonempty(&out, "RST");
    assert_text_contains(&b, &["Titre Principal"], "RST");
    cleanup(&out);
}

#[test]
fn export_adoc_keeps_content() {
    let out = tmp("adoc", "adoc");
    export_formats::export_adoc(sample(), &out, &meta()).expect("export_adoc returned Err");
    let b = read_nonempty(&out, "AsciiDoc");
    assert_text_contains(&b, &["Titre Principal"], "AsciiDoc");
    cleanup(&out);
}

#[test]
fn export_typst_keeps_content() {
    let out = tmp("typ", "typ");
    export_formats::export_typst_src(sample(), &out, &meta()).expect("export_typst_src returned Err");
    let b = read_nonempty(&out, "Typst");
    assert_text_contains(&b, &["Titre Principal"], "Typst");
    cleanup(&out);
}

#[test]
fn export_ipynb_is_valid_json() {
    let out = tmp("ipynb", "ipynb");
    export_formats::export_ipynb(sample(), &out, &meta()).expect("export_ipynb returned Err");
    let b = read_nonempty(&out, "Jupyter");
    let v: serde_json::Value = serde_json::from_slice(&b)
        .expect("Jupyter: output is NOT valid JSON - notebook would not open");
    assert!(v["cells"].is_array(), "Jupyter: 'cells' is not an array - invalid notebook");
    assert!(v["nbformat"].is_number(), "Jupyter: missing nbformat");
    cleanup(&out);
}

#[test]
fn export_html_is_self_contained() {
    let out = tmp("html", "html");
    export::export_html(sample(), &out, &meta(), None).expect("export_html returned Err");
    let b = read_nonempty(&out, "HTML");
    assert_text_contains(&b, &["<html", "Titre Principal", "</html>"], "HTML");
    cleanup(&out);
}

// ── THE HEADLINE: DOCX lossless round-trip (the core differentiator) ───────────

#[test]
fn docx_roundtrip_is_lossless() {
    let original = sample_eq(); // includes equations - must survive too
    let out = tmp("roundtrip", "docx");
    export_formats::export_docx(original, &out, &meta(), None).expect("export_docx failed");

    let recovered = source_embed::import_docx_source(&out)
        .expect("import_docx_source failed - the embedded source entry is unreadable");

    assert_eq!(
        recovered.trim(),
        original.trim(),
        "LOSSLESS ROUND-TRIP BROKEN - recovered Markdown differs from original. \
         This is the core promise of the tool."
    );
    cleanup(&out);
}

#[test]
fn docx_embeds_source_entry() {
    // Confirm the md-to-all-source.xml ZIP entry is actually present in the DOCX.
    let out = tmp("embed_check", "docx");
    export_formats::export_docx(sample(), &out, &meta(), None).expect("export_docx failed");
    let file = fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut found = false;
    for i in 0..zip.len() {
        if zip.by_index(i).unwrap().name() == source_embed::DOCX_SOURCE_ENTRY {
            found = true;
            break;
        }
    }
    assert!(found, "DOCX missing '{}' - reversibility layer 1 absent", source_embed::DOCX_SOURCE_ENTRY);
    cleanup(&out);
}

#[test]
fn docx_media_carry_recoverable_latex() {
    // Reversibility layers 2 & 3 through the REAL export pipeline: the equation
    // images embedded in the DOCX must carry the original LaTeX (PNG tEXt chunk +
    // SVG <metadata>), so recovery still works after Word strips the primary
    // md-to-all-source.xml entry. This guards the "hidden data survives export"
    // contract end-to-end - the existing round-trip tests only exercise layer 1.
    use std::io::Read;
    let md = "# Title\n\n$$E = mc^2$$\n";
    let out = tmp("media_latex", "docx");
    export_formats::export_docx(md, &out, &meta(), None).expect("export_docx failed");

    let file = fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut png_latex: Option<String> = None;
    let mut svg_latex: Option<String> = None;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).unwrap();
        let name = e.name().to_string();
        if !name.starts_with("word/media/") {
            continue;
        }
        if name.ends_with(".png") {
            let mut buf = Vec::new();
            e.read_to_end(&mut buf).unwrap();
            if let Some(l) = source_embed::extract_latex_from_png(&buf) {
                png_latex = Some(l);
            }
        } else if name.ends_with(".svg") {
            let mut s = String::new();
            e.read_to_string(&mut s).unwrap();
            if let Some(l) = source_embed::extract_latex_from_svg(&s) {
                svg_latex = Some(l);
            }
        }
    }

    // Layer 2 (PNG tEXt) is the reliable secondary recovery layer - require it.
    let png = png_latex
        .expect("no PNG in word/media carried a recoverable LaTeX tEXt chunk (layer 2 broken)");
    assert!(png.contains("mc^2"), "PNG tEXt LaTeX '{png}' does not carry the equation");
    // Layer 3 (SVG metadata) is for older Word / LibreOffice; assert it when the
    // exporter embedded an SVG, but do not fail the test on its absence.
    if let Some(svg) = svg_latex {
        assert!(svg.contains("mc^2"), "SVG metadata LaTeX '{svg}' does not carry the equation");
    }
    cleanup(&out);
}

#[test]
fn docx_recovers_after_source_entry_stripped() {
    // The "Word Save-As rebuilt the ZIP and dropped md-to-all-source.xml" scenario -
    // the real failure mode the product promises to survive. Recovery must fall back
    // to layers 2/3 (equation images / MD-TO-ALL comments), still return the LaTeX,
    // and mark the result partial (full=false). Only a synthetic DOCX tested this
    // before; this drives it through a genuine export.
    let md = "# Title\n\nText before.\n\n$$E = mc^2$$\n\nText after.\n";
    let full_docx = tmp("strip_full", "docx");
    export_formats::export_docx(md, &full_docx, &meta(), None).expect("export_docx failed");

    // Rebuild the DOCX without the primary source entry (raw-copy every other entry).
    let stripped = tmp("strip_partial", "docx");
    {
        let rf = fs::File::open(&full_docx).unwrap();
        let mut zin = zip::ZipArchive::new(rf).unwrap();
        let wf = fs::File::create(&stripped).unwrap();
        let mut zout = zip::ZipWriter::new(wf);
        for i in 0..zin.len() {
            let file = zin.by_index(i).unwrap();
            if file.name() == source_embed::DOCX_SOURCE_ENTRY {
                continue;
            }
            zout.raw_copy_file(file).unwrap();
        }
        zout.finish().unwrap();
    }

    // Sanity: the primary entry is really gone.
    {
        let rf = fs::File::open(&stripped).unwrap();
        let zin = zip::ZipArchive::new(rf).unwrap();
        assert!(
            !zin.file_names().any(|n| n == source_embed::DOCX_SOURCE_ENTRY),
            "the source entry should have been stripped"
        );
    }

    let (recovered, full) = source_embed::import_docx_source_detailed(&stripped)
        .expect("recovery must degrade gracefully, not error, on a stripped DOCX");
    assert!(!full, "recovery must be marked partial once the primary entry is gone");
    assert!(
        recovered.contains("mc^2"),
        "the equation LaTeX must be recovered from layers 2/3, got: {recovered}"
    );

    cleanup(&full_docx);
    cleanup(&stripped);
}

#[test]
fn docx_comment_latex_roundtrips_through_md_to_all_author() {
    // Layer-2-via-comments recovery binds two hardcoded literals: the exporter
    // writes w:author="MD-TO-ALL" comments carrying the LaTeX, and source_embed
    // mines exactly that author. A silent drift on either side kills comment-based
    // recovery (the project's reversibility spec marks this literal load-bearing).
    // Round-trips both sides through the real export + the real extractor.
    use std::io::Read;
    let md = "# Title\n\n$$E = mc^2$$\n";
    let out = tmp("comment_rt", "docx");
    export_formats::export_docx(md, &out, &meta(), None).expect("export_docx failed");

    let file = fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut comments = String::new();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).unwrap();
        if !e.name().ends_with(".xml") {
            continue;
        }
        let mut s = String::new();
        if e.read_to_string(&mut s).is_err() {
            continue;
        }
        if s.contains("w:author=\"MD-TO-ALL\"") {
            comments = s;
            break;
        }
    }
    assert!(
        !comments.is_empty(),
        "export wrote no MD-TO-ALL-authored comment for the equation (write-side literal drifted?)"
    );

    // Read side: the extractor recovers the LaTeX from exactly that author's comments.
    let map = source_embed::extract_latex_from_comments_xml(&comments);
    assert!(
        map.values().any(|v| v.contains("mc^2")),
        "extract_latex_from_comments_xml did not recover the equation from the MD-TO-ALL comment: {map:?}"
    );
    // A foreign author must be ignored - we only mine our own embedded data.
    let foreign = comments.replace("MD-TO-ALL", "SomeReviewer");
    assert!(
        source_embed::extract_latex_from_comments_xml(&foreign).is_empty(),
        "comments from a foreign author must not be mined"
    );
    cleanup(&out);
}

#[test]
fn extract_equations_edge_cases() {
    // extract_equations is the primitive behind the MCP extract_equations tool and
    // analyze_document. Pin its three tricky behaviors.
    use crate::render::extract_equations;

    // 1. $$ inside a fenced code block is NOT an equation.
    let fenced = "text\n\n```\n$$ not_an_equation $$\n```\n\nreal:\n\n$$a+b$$\n";
    let eqs = extract_equations(fenced);
    assert_eq!(
        eqs,
        vec!["a+b".to_string()],
        "must ignore $$ inside a code fence; got {eqs:?}"
    );

    // 2. A multi-line $$ block is captured whole, as one equation.
    let multi = "$$\n\\int_0^1 x\\,dx\n= \\tfrac12\n$$\n";
    let eqs = extract_equations(multi);
    assert_eq!(eqs.len(), 1, "a multi-line $$ block is one equation; got {eqs:?}");
    assert!(
        eqs[0].contains("int_0^1") && eqs[0].contains("tfrac12"),
        "the multi-line body must be captured whole; got {eqs:?}"
    );

    // 3. An unterminated $$ must not hang, and must not swallow the following
    //    heading/text into a phantom equation.
    let unclosed = "$$\na+b\n\n# Heading\n\nmore text\n";
    let eqs = extract_equations(unclosed);
    assert!(
        !eqs.iter().any(|e| e.contains("Heading")),
        "an unclosed $$ must not capture the following heading; got {eqs:?}"
    );
}

#[test]
fn txt_export_renders_display_math_as_unicode() {
    // latex_to_unicode (via sub_eq_unicode) is a documented never-remove item: the
    // TXT export turns $$...$$ display blocks into a unicode approximation, not raw
    // LaTeX. A silent switch to raw passthrough would ship "\alpha" to a .txt reader.
    let md = "# T\n\n$$\\alpha + \\beta$$\n";
    let out = tmp("txt_unicode", "txt");
    export_formats::export_txt(md, &out).expect("export_txt failed");
    let txt = fs::read_to_string(&out).expect("read txt");
    assert!(
        txt.contains('\u{03b1}'),
        "TXT export must render $$\\alpha$$ as unicode alpha; got:\n{txt}"
    );
    assert!(
        !txt.contains("\\alpha"),
        "TXT export must not leave raw LaTeX \\alpha; got:\n{txt}"
    );
    cleanup(&out);
}

#[test]
fn advertised_export_formats_are_all_dispatched() {
    // list_formats / the MCP advertise supported_export_exts(); each must actually
    // be handled by export_md's dispatch, not fall through to "not supported".
    // Some formats may Err for runtime reasons; only an "is not supported" error
    // means the advertised list lies about a format it cannot produce.
    let md = "# T\n\ntext and\n\n$$a+b$$\n";
    for ext in crate::convert::supported_export_exts() {
        let out = tmp("parity", ext);
        if let Err(e) = crate::convert::export_md(md, &out, &meta(), None) {
            assert!(
                !e.contains("is not supported"),
                "advertised export .{ext} is not dispatched by export_md: {e}"
            );
        }
        cleanup(&out);
    }
}

#[test]
fn docx_recovers_from_svg_media_alone() {
    // Reversibility layer 3: even when the source entry, the MD-TO-ALL comments,
    // AND the PNG media are all gone (the older-Word / LibreOffice case that keeps
    // only the SVG), recovery must still reconstruct the equation from the SVG
    // <metadata>. Both image formats are embedded precisely so one surviving suffices.
    let md = "# Title\n\n$$E = mc^2$$\n";
    let full_docx = tmp("svg_full", "docx");
    export_formats::export_docx(md, &full_docx, &meta(), None).expect("export_docx failed");

    // Rebuild keeping only the SVG media: drop the source entry, the comments part,
    // and every PNG, so only the SVG can carry the LaTeX.
    let svg_only = tmp("svg_only", "docx");
    {
        let rf = fs::File::open(&full_docx).unwrap();
        let mut zin = zip::ZipArchive::new(rf).unwrap();
        let wf = fs::File::create(&svg_only).unwrap();
        let mut zout = zip::ZipWriter::new(wf);
        let mut kept_svg = false;
        for i in 0..zin.len() {
            let file = zin.by_index(i).unwrap();
            let name = file.name().to_string();
            if name == source_embed::DOCX_SOURCE_ENTRY
                || name.contains("comments")
                || name.to_lowercase().ends_with(".png")
            {
                continue;
            }
            if name.to_lowercase().ends_with(".svg") {
                kept_svg = true;
            }
            zout.raw_copy_file(file).unwrap();
        }
        zout.finish().unwrap();
        assert!(kept_svg, "the export embedded no SVG media - layer 3 cannot exist");
    }

    let (recovered, full) = source_embed::import_docx_source_detailed(&svg_only)
        .expect("recovery must not error on an SVG-only DOCX");
    assert!(!full, "recovery must be partial once the primary entry is gone");
    assert!(
        recovered.contains("mc^2"),
        "the equation LaTeX must be recovered from the SVG metadata alone, got: {recovered}"
    );

    cleanup(&full_docx);
    cleanup(&svg_only);
}

#[test]
fn recovery_on_non_mdall_or_corrupt_docx_never_panics() {
    // The MCP inspect_docx / recover_source tools gate on this: a plain Word doc
    // (no mdall markers) must degrade to partial recovery (full=false) without
    // panicking, and a corrupt file must return a clean Err, not abort the process.

    // (1) A foreign-looking DOCX: a real export, no equations, source entry stripped.
    let plain = tmp("foreign_full", "docx");
    export_formats::export_docx("# Title\n\nJust plain prose.\n", &plain, &meta(), None)
        .expect("export_docx failed");
    let stripped = tmp("foreign_stripped", "docx");
    {
        let rf = fs::File::open(&plain).unwrap();
        let mut zin = zip::ZipArchive::new(rf).unwrap();
        let wf = fs::File::create(&stripped).unwrap();
        let mut zout = zip::ZipWriter::new(wf);
        for i in 0..zin.len() {
            let file = zin.by_index(i).unwrap();
            if file.name() == source_embed::DOCX_SOURCE_ENTRY {
                continue;
            }
            zout.raw_copy_file(file).unwrap();
        }
        zout.finish().unwrap();
    }
    let (_md, full) = source_embed::import_docx_source_detailed(&stripped)
        .expect("a plain DOCX must degrade to partial recovery, not error");
    assert!(!full, "a DOCX with no source entry must not report lossless recovery");

    // (2) A corrupt (non-zip) file must Err cleanly, never panic.
    let junk = tmp("corrupt", "docx");
    fs::write(&junk, b"not a zip file at all").unwrap();
    assert!(
        source_embed::import_docx_source_detailed(&junk).is_err(),
        "a corrupt DOCX must return Err, not panic"
    );

    cleanup(&plain);
    cleanup(&stripped);
    cleanup(&junk);
}

// ── Edge cases: conversions must NEVER panic on hostile input ──────────────────

fn run_all_text_exports(md: &str, tag: &str) {
    let m = meta();
    // Each call must return (Ok or Err) - a panic here fails the test.
    let _ = export_formats::export_txt(md, &tmp(&format!("{}_txt", tag), "txt"));
    let _ = export_formats::export_tex(md, &tmp(&format!("{}_tex", tag), "tex"), &m);
    let _ = export_formats::export_rtf(md, &tmp(&format!("{}_rtf", tag), "rtf"), &m, None);
    let _ = export_formats::export_org(md, &tmp(&format!("{}_org", tag), "org"), &m);
    let _ = export_formats::export_rst(md, &tmp(&format!("{}_rst", tag), "rst"), &m);
    let _ = export_formats::export_adoc(md, &tmp(&format!("{}_adoc", tag), "adoc"), &m);
    let _ = export_formats::export_ipynb(md, &tmp(&format!("{}_ipynb", tag), "ipynb"), &m);
    let _ = export_formats::export_typst_src(md, &tmp(&format!("{}_typ", tag), "typ"), &m);
    let _ = export::export_html(md, &tmp(&format!("{}_html", tag), "html"), &m, None);
}

#[test]
fn edge_empty_never_panics() {
    run_all_text_exports("", "empty");
}

#[test]
fn edge_unicode_never_panics() {
    run_all_text_exports("# Ünïcödé 日本語 العربية 🔬\n\n**émojis** $\\alpha\\beta$ ç à é", "unicode");
}

#[test]
fn edge_malformed_never_panics() {
    // Unbalanced markup, dangling delimiters, broken table, unterminated code fence.
    let bad = "# Titre **gras sans fin\n\n| a | b\n| broken\n\n```rust\nno close fence\n\n$$ unterminated";
    run_all_text_exports(bad, "malformed");
}

#[test]
fn edge_large_never_panics() {
    // 50k lines - verify no quadratic blowup / stack overflow on a big document.
    let mut big = String::with_capacity(2_000_000);
    for i in 0..50_000 {
        big.push_str(&format!("Ligne {} avec **gras** et `code`.\n", i));
    }
    let m = meta();
    let _ = export_formats::export_txt(&big, &tmp("large_txt", "txt"));
    let _ = export::export_html(&big, &tmp("large_html", "html"), &m, None);
}

// ── Import: must not silently empty the user's document ────────────────────────

#[test]
fn import_html_preserves_content() {
    let html = "<html><body><h1>Mon Titre</h1><p>Un <strong>paragraphe</strong> important.</p>\
                <ul><li>alpha</li><li>beta</li></ul></body></html>";
    let md = crate::import::html_to_md(html).expect("html_to_md failed");
    assert!(!md.trim().is_empty(), "HTML import produced EMPTY markdown - total content loss");
    for needle in ["Mon Titre", "paragraphe", "alpha", "beta"] {
        assert!(md.contains(needle), "HTML import lost content: '{}'. Got: {}", needle, md);
    }
}

#[test]
fn import_docx_generic_does_not_panic_on_real_docx() {
    // Export a DOCX, then import it via the GENERIC path (not the lossless layer).
    // Proves generic Word import extracts real text instead of returning blank.
    let out = tmp("generic_import", "docx");
    export_formats::export_docx(sample(), &out, &meta(), None).expect("export_docx failed");
    let md = crate::import::docx_generic_to_md(&out).expect("docx_generic_to_md failed");
    assert!(!md.trim().is_empty(), "Generic DOCX import returned EMPTY - would blank the user's doc");
    assert!(md.contains("Titre"), "Generic DOCX import lost the title. Got: {}", md);
    cleanup(&out);
}

#[test]
fn import_odt_survives_zip_entry_cap() {
    // Round-trip an ODT through export + import. Exercises the size-capped
    // content.xml read (MAX_ZIP_ENTRY_BYTES zip-bomb guard): a legitimate
    // document must come back whole, never truncated by the cap.
    let out = tmp("odt_cap", "odt");
    export_formats::export_odt(sample(), &out, &meta(), None).expect("export_odt failed");
    let md = crate::import::odt_to_md(&out).expect("odt_to_md failed");
    assert!(!md.trim().is_empty(), "ODT import returned EMPTY - content loss");
    assert!(md.contains("Titre"), "ODT import lost the title. Got: {}", md);
    cleanup(&out);
}

// ── S6: generic DOCX import fidelity (bold / italic / headings) ────────────────

#[test]
fn docx_import_preserves_bold_and_italic() {
    // A run with <w:b/> → **bold**, a run with <w:i/> → *italic*.
    let xml = r#"<w:document><w:body>
        <w:p>
          <w:r><w:t xml:space="preserve">plain </w:t></w:r>
          <w:r><w:rPr><w:b/></w:rPr><w:t>strong</w:t></w:r>
          <w:r><w:t xml:space="preserve"> and </w:t></w:r>
          <w:r><w:rPr><w:i/></w:rPr><w:t>emph</w:t></w:r>
        </w:p>
        </w:body></w:document>"#;
    let md = crate::import::docx_xml_to_md(xml).expect("docx_xml_to_md failed");
    assert!(md.contains("**strong**"), "bold run not converted to **...**. Got: {}", md);
    assert!(md.contains("*emph*"), "italic run not converted to *...*. Got: {}", md);
    assert!(md.contains("plain"), "plain text lost. Got: {}", md);
}

#[test]
fn docx_import_emphasis_markers_avoid_adjacent_spaces() {
    // Trailing space inside a bold run must end up OUTSIDE the markers, else the
    // emphasis would not render ("** bold **" is not bold in CommonMark).
    let xml = r#"<w:document><w:body>
        <w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">bold </w:t></w:r><w:r><w:t>tail</w:t></w:r></w:p>
        </w:body></w:document>"#;
    let md = crate::import::docx_xml_to_md(xml).expect("docx_xml_to_md failed");
    assert!(md.contains("**bold**"), "marker should hug the word, space outside. Got: {}", md);
    // No space adjacent INSIDE the emphasis (CommonMark would not render those).
    assert!(!md.contains("** bold"), "no space after opening marker. Got: {}", md);
    assert!(!md.contains("bold **"), "no space before closing marker. Got: {}", md);
}

#[test]
fn docx_import_val_false_does_not_bold() {
    // <w:b w:val="false"/> turns bold OFF - must not wrap in **.
    let xml = r#"<w:document><w:body>
        <w:p><w:r><w:rPr><w:b w:val="false"/></w:rPr><w:t>normal</w:t></w:r></w:p>
        </w:body></w:document>"#;
    let md = crate::import::docx_xml_to_md(xml).expect("docx_xml_to_md failed");
    assert!(md.contains("normal"), "text lost. Got: {}", md);
    assert!(!md.contains("**"), "w:val=false should NOT produce bold. Got: {}", md);
}

#[test]
fn docx_import_coalesces_same_format_runs() {
    // Two adjacent bold runs must merge into one **...** span, not **a****b**.
    let xml = r#"<w:document><w:body>
        <w:p>
          <w:r><w:rPr><w:b/></w:rPr><w:t>foo</w:t></w:r>
          <w:r><w:rPr><w:b/></w:rPr><w:t>bar</w:t></w:r>
        </w:p>
        </w:body></w:document>"#;
    let md = crate::import::docx_xml_to_md(xml).expect("docx_xml_to_md failed");
    assert!(md.contains("**foobar**"), "adjacent bold runs should coalesce. Got: {}", md);
    assert!(!md.contains("****"), "no empty marker run from concatenation. Got: {}", md);
}

// ── Import panic-safety: a foreign or malformed file must never crash the app ──
//    (Exercises the catch_unwind guard in convert::import_to_md across the
//     importer surface. Constraint: never panic on user input.)

fn import_via_convert(content: &str, ext: &str) {
    let out = tmp("imp", ext);
    fs::write(&out, content).unwrap();
    let _ = crate::convert::import_to_md(&out); // guarded: must return, never panic
    let _ = fs::remove_file(&out);
}

#[test]
fn import_malformed_files_never_panic() {
    let exts = ["html", "tex", "org", "rst", "adoc", "wiki", "typ", "ipynb", "csv", "rtf", "md"];
    let inputs = [
        "",
        "<h1>open <b>no close <ul><li>x",
        "\\section{open \\textbf{no close $x = ",
        "{ \"cells\": [ { \"cell_type\":",
        "a,b,c\n\"unterminated,quote,here",
        "#+BEGIN_SRC\nno end",
        "=",
        "==",
        "======",
        "# Unicode test 日本語 $\\alpha\\beta$ cafe",
    ];
    for ext in exts {
        for inp in inputs {
            import_via_convert(inp, ext);
        }
    }
}

#[test]
fn wiki_bare_equals_lines_no_longer_panic() {
    // Regression: a MediaWiki line made only of `=` used to slice out of range
    // (t[level..t.len()-level]). The direct parser must now be safe by itself.
    for src in ["=", "==", "===", "======", "=======", "==x==", "= ="] {
        let _ = crate::import::wiki_to_md(src);
    }
    let md = crate::import::wiki_to_md("== Heading ==").unwrap_or_default();
    assert!(md.contains("Heading"), "valid wiki heading lost: {md:?}");
}

// ── <div> blocks span blank lines (so styled boxes render as one box, not raw
//    fragments separated by gaps). ────────────────────────────────────────────

#[test]
fn div_with_blank_lines_is_one_html_block() {
    let md = "<div style=\"text-align:center\">\n\nHello world.\n\n</div>";
    let blocks = crate::editor::parse_document(md);
    assert_eq!(blocks.len(), 1, "the whole div must be ONE block, got {:?}",
        blocks.iter().map(|b| format!("{:?}", b.kind)).collect::<Vec<_>>());
    assert!(matches!(blocks[0].kind, crate::editor::BlockKind::HtmlBlock));
    assert!(md[blocks[0].source_range.clone()].contains("</div>"), "block must include the close");
}

#[test]
fn nested_divs_balance_into_one_block() {
    let md = "<div style=\"a\">\n\n<div style=\"b\">\n\nx\n\n</div>\n\n</div>";
    let blocks = crate::editor::parse_document(md);
    assert_eq!(blocks.len(), 1, "nested divs are one outer block");
    assert!(md[blocks[0].source_range.clone()].trim_end().ends_with("</div>"));
}

#[test]
fn unclosed_div_does_not_swallow_following_blocks() {
    let md = "<div style=\"x\">\n\ninside\n\n# Heading after";
    let blocks = crate::editor::parse_document(md);
    assert!(blocks.iter().any(|b| matches!(b.kind, crate::editor::BlockKind::Heading(1))),
        "a heading after an unclosed div must still parse");
}

// ── Inline style tags (<span style=color>, <mark style=background>) are inline
//    runs, NOT HTML blocks. Regression: applying colour/highlight wrote a tag with
//    attributes that the block parser captured as a block and leaked raw. ────────

#[test]
fn inline_style_tags_are_paragraphs_not_html_blocks() {
    for md in [
        "<span style=\"color:#e74c3c\">red text</span>",
        "<mark style=\"background:#e67e22\">highlighted</mark>",
        "<u>underlined</u>",
        "<sup>2</sup> is squared",
    ] {
        let blocks = crate::editor::parse_document(md);
        assert_eq!(blocks.len(), 1, "{md:?} should be one block");
        assert!(matches!(blocks[0].kind, crate::editor::BlockKind::Paragraph),
            "{md:?} must be an inline Paragraph, got {:?}", blocks[0].kind);
    }
    // Block containers are unaffected.
    let d = crate::editor::parse_document("<div style=\"text-align:center\">\n\nx\n\n</div>");
    assert!(matches!(d[0].kind, crate::editor::BlockKind::HtmlBlock), "<div> stays a block");
    // <ul> must NOT be mistaken for the inline <u> tag.
    let u = crate::editor::parse_document("<ul><li>a</li></ul>");
    assert!(matches!(u[0].kind, crate::editor::BlockKind::HtmlBlock), "<ul> stays a block");
}

#[test]
fn multiline_inline_span_is_one_paragraph() {
    // The colour command can wrap a selection that crosses a soft line break.
    let md = "<span style=\"color:#e74c3c\">t\ntest</span>";
    let blocks = crate::editor::parse_document(md);
    assert_eq!(blocks.len(), 1, "a span across a soft break stays one paragraph");
    assert!(matches!(blocks[0].kind, crate::editor::BlockKind::Paragraph));
}

// ── An unclosed `$$` display equation must NOT swallow the rest of the document.
//    Regression: a single stray `$$` ate headings + tables into one giant equation
//    that then broke the math renderer (Typst choked on the `#` of `###`). ───────

#[test]
fn unclosed_display_math_does_not_swallow_heading() {
    let md = "$$\\text{Non-Markovian} \\iff s\n\n### 3.3 Classification\n\nbody text";
    let blocks = crate::editor::parse_document(md);
    assert!(
        blocks.iter().any(|b| matches!(b.kind, crate::editor::BlockKind::Heading(3))),
        "a heading after an unclosed $$ must still parse, got {:?}",
        blocks.iter().map(|b| format!("{:?}", b.kind)).collect::<Vec<_>>()
    );
    assert!(matches!(blocks[0].kind, crate::editor::BlockKind::Paragraph),
        "the unclosed $$ opening line is a bounded paragraph, not a swallowing equation");
}

#[test]
fn unclosed_display_math_bounded_by_blank_line() {
    let md = "$$ E = mc^2 -\n\nNext paragraph.";
    let blocks = crate::editor::parse_document(md);
    assert!(matches!(blocks[0].kind, crate::editor::BlockKind::Paragraph));
    assert!(
        blocks.iter().any(|b| matches!(b.kind, crate::editor::BlockKind::Paragraph)
            && md[b.source_range.clone()].contains("Next paragraph")),
        "content after an unclosed $$ must still parse"
    );
}

#[test]
fn well_formed_display_math_still_parses() {
    let a = crate::editor::parse_document("$$E = mc^2$$");
    assert!(matches!(a[0].kind, crate::editor::BlockKind::DisplayEquation { .. }),
        "single-line $$...$$ is still an equation");
    let b = crate::editor::parse_document("$$\n\\frac{a}{b}\n= c\n$$");
    assert_eq!(b.len(), 1, "a properly closed multi-line $$ is one block");
    assert!(matches!(b[0].kind, crate::editor::BlockKind::DisplayEquation { .. }),
        "closed multi-line $$ is still an equation");
}

// ── S6: quick-xml DOCX parser (tables, hyperlinks) ────────────────────────────

#[test]
fn docx_qx_renders_table_as_gfm() {
    let xml = r#"<w:document><w:body>
      <w:tbl>
        <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
              <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
        <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc>
              <w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc></w:tr>
      </w:tbl>
      </w:body></w:document>"#;
    let rels = std::collections::HashMap::new();
    let md = crate::import_xml::docx_document_to_md(xml, &rels).expect("docx qx failed");
    assert!(md.contains("| A | B |"), "table header missing. Got: {}", md);
    assert!(md.contains("| --- | --- |"), "table separator missing. Got: {}", md);
    assert!(md.contains("| 1 | 2 |"), "table body row missing. Got: {}", md);
}

#[test]
fn docx_qx_resolves_hyperlink_via_rels() {
    let xml = r#"<w:document><w:body>
      <w:p><w:hyperlink r:id="rId1"><w:r><w:t>site</w:t></w:r></w:hyperlink></w:p>
      </w:body></w:document>"#;
    let mut rels = std::collections::HashMap::new();
    rels.insert("rId1".to_string(), "https://example.com".to_string());
    let md = crate::import_xml::docx_document_to_md(xml, &rels).expect("docx qx failed");
    assert!(md.contains("[site](https://example.com)"), "hyperlink not resolved. Got: {}", md);
}

#[test]
fn docx_qx_bold_run() {
    let xml = r#"<w:document><w:body>
      <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>strong</w:t></w:r></w:p>
      </w:body></w:document>"#;
    let rels = std::collections::HashMap::new();
    let md = crate::import_xml::docx_document_to_md(xml, &rels).expect("docx qx failed");
    assert!(md.contains("**strong**"), "bold not detected by qx. Got: {}", md);
}

// ── S6: quick-xml ODT parser (style-resolved emphasis, tables) ─────────────────

#[test]
fn odt_qx_bold_via_style_map() {
    let xml = r#"<office:document-content>
      <office:automatic-styles>
        <style:style style:name="T1" style:family="text">
          <style:text-properties fo:font-weight="bold"/>
        </style:style>
        <style:style style:name="T2" style:family="text">
          <style:text-properties fo:font-style="italic"/>
        </style:style>
      </office:automatic-styles>
      <office:body><office:text>
        <text:p>plain <text:span text:style-name="T1">strong</text:span> and <text:span text:style-name="T2">emph</text:span></text:p>
      </office:text></office:body>
      </office:document-content>"#;
    let md = crate::import_xml::odt_content_to_md(xml).expect("odt qx failed");
    assert!(md.contains("**strong**"), "ODT bold via style map failed. Got: {}", md);
    assert!(md.contains("*emph*"), "ODT italic via style map failed. Got: {}", md);
    assert!(md.contains("plain"), "ODT plain text lost. Got: {}", md);
}

#[test]
fn odt_qx_renders_table_as_gfm() {
    let xml = r#"<office:document-content>
      <office:body><office:text>
        <table:table>
          <table:table-row><table:table-cell><text:p>A</text:p></table:table-cell>
                           <table:table-cell><text:p>B</text:p></table:table-cell></table:table-row>
          <table:table-row><table:table-cell><text:p>1</text:p></table:table-cell>
                           <table:table-cell><text:p>2</text:p></table:table-cell></table:table-row>
        </table:table>
      </office:text></office:body></office:document-content>"#;
    let md = crate::import_xml::odt_content_to_md(xml).expect("odt qx failed");
    assert!(md.contains("| A | B |"), "ODT table header missing. Got: {}", md);
    assert!(md.contains("| 1 | 2 |"), "ODT table body missing. Got: {}", md);
}

// ── S6: EPUB import is verified end-to-end (rides on the HTML converter) ───────

#[test]
fn epub_roundtrip_import_preserves_content() {
    let out = tmp("epub_rt", "epub");
    export_formats::export_epub(sample(), &out, &meta(), None).expect("export_epub failed");
    let md = crate::import::epub_to_md(&out).expect("epub_to_md failed");
    assert!(!md.trim().is_empty(), "EPUB import returned EMPTY - content loss");
    assert!(md.contains("Titre Principal"), "EPUB import lost the title. Got: {}", md);
    cleanup(&out);
}

// ── OMML → LaTeX import fidelity (renderable equations from DOCX) ──────────────

#[test]
fn omml_accented_text_becomes_text_command() {
    // Accented prose in math must be wrapped in \text{} (raw 'é' breaks Typst).
    let xml = "<m:r><m:t>café</m:t></m:r>";
    assert_eq!(crate::import::omml_to_latex(xml), "\\text{café}");
}

#[test]
fn omml_plain_text_run_wrapped() {
    let xml = "<m:r><m:rPr><m:nor/></m:rPr><m:t>vitesse</m:t></m:r>";
    assert_eq!(crate::import::omml_to_latex(xml), "\\text{vitesse}");
}

#[test]
fn omml_greek_maps_to_command() {
    let xml = "<m:r><m:t>α</m:t></m:r>";
    assert_eq!(crate::import::omml_to_latex(xml).trim(), "\\alpha");
}

#[test]
fn omml_fraction_structure() {
    let xml = "<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>\
               <m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>";
    assert_eq!(crate::import::omml_to_latex(xml), "\\frac{a}{b}");
}

#[test]
fn omml_multi_part_run_keeps_all_text() {
    // A run with two <m:t> leaves must keep both (ASCII math stays as-is).
    let xml = "<m:r><m:t>ab</m:t><m:t>cd</m:t></m:r>";
    assert_eq!(crate::import::omml_to_latex(xml), "abcd");
}

// ── Figure coverage: a figure must survive into EVERY output format ──────────

/// Assert `bytes` is well-formed XML (parses to EOF with no error). Guards the
/// class of bug where a ZIP is structurally valid but a part is malformed XML
/// (e.g. a missing space between attributes) so the office app refuses to open it.
fn assert_well_formed_xml(bytes: &[u8], ctx: &str) {
    let s = std::str::from_utf8(bytes).unwrap_or_else(|_| panic!("{ctx}: part is not UTF-8"));
    let mut reader = quick_xml::Reader::from_str(s);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => panic!("{ctx}: MALFORMED XML - office app would refuse to open it: {e}"),
            _ => {}
        }
    }
}

fn zip_part(path: &PathBuf, name_suffix: &str) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).ok()?;
        if f.name().ends_with(name_suffix) {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).ok()?;
            return Some(buf);
        }
    }
    None
}

/// Every ODT part must be well-formed XML. Regression guard for the malformed
/// styles.xml (a missing attribute space) that made LibreOffice reject the file
/// while the ZIP-validity test still passed.
#[test]
fn odt_parts_are_well_formed_xml() {
    let out = tmp("odt_xml", "odt");
    export_formats::export_odt(sample(), &out, &meta(), None).expect("export_odt failed");
    for part in ["styles.xml", "content.xml", "META-INF/manifest.xml"] {
        let bytes = zip_part(&out, part).unwrap_or_else(|| panic!("ODT missing part {part}"));
        assert_well_formed_xml(&bytes, &format!("ODT {part}"));
    }
    cleanup(&out);
}

/// A document figure (`![](pic.png)`) must survive into every output format -
/// embedded for containers/self-contained targets, referenced for source formats.
#[test]
fn figure_survives_into_every_format() {
    let dir = std::env::temp_dir().join(format!("mdall_figcov_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    image::RgbaImage::from_pixel(8, 6, image::Rgba([40, 90, 200, 255]))
        .save(dir.join("pic.png"))
        .expect("write test png");
    let md = "# Doc\n\nBefore.\n\n![A caption](pic.png)\n\nAfter.\n";
    let sd = Some(dir.as_path());
    let p = |ext: &str| dir.join(format!("doc.{ext}"));

    // Containers / self-contained: the binary travels inside the file.
    export_formats::export_docx(md, &p("docx"), &meta(), sd).unwrap();
    assert!(zip_part(&p("docx"), "fig_0.png").is_some(), "DOCX dropped figure binary");
    export_formats::export_odt(md, &p("odt"), &meta(), sd).unwrap();
    assert!(zip_part(&p("odt"), "fig_0.png").is_some(), "ODT dropped figure binary");
    export_formats::export_epub(md, &p("epub"), &meta(), sd).unwrap();
    assert!(zip_part(&p("epub"), "fig_0.png").is_some(), "EPUB dropped figure binary");
    export::export_html(md, &p("html"), &meta(), sd).unwrap();
    assert!(fs::read_to_string(p("html")).unwrap().contains("data:image"), "HTML did not inline figure");
    export_formats::export_rtf(md, &p("rtf"), &meta(), sd).unwrap();
    assert!(fs::read_to_string(p("rtf")).unwrap().contains("pngblip"), "RTF dropped figure");

    // Source formats: a correct reference in the format's own idiom.
    export_formats::export_tex(md, &p("tex"), &meta()).unwrap();
    assert!(fs::read_to_string(p("tex")).unwrap().contains("includegraphics"), "TeX dropped figure");
    export_formats::export_typst_src(md, &p("typ"), &meta()).unwrap();
    assert!(fs::read_to_string(p("typ")).unwrap().contains("image("), "Typst dropped figure");
    export_formats::export_rst(md, &p("rst"), &meta()).unwrap();
    assert!(fs::read_to_string(p("rst")).unwrap().contains(".. image::"), "RST dropped figure");
    export_formats::export_org(md, &p("org"), &meta()).unwrap();
    assert!(fs::read_to_string(p("org")).unwrap().contains("[[file:"), "Org dropped figure");
    export_formats::export_adoc(md, &p("adoc"), &meta()).unwrap();
    assert!(fs::read_to_string(p("adoc")).unwrap().contains("image::"), "AsciiDoc dropped figure");
    export_formats::export_ipynb(md, &p("ipynb"), &meta()).unwrap();
    assert!(fs::read_to_string(p("ipynb")).unwrap().contains("!["), "ipynb dropped figure");

    let _ = fs::remove_dir_all(&dir);
}

// ── Tables must render natively (not concatenate cells into one run) ──────────

#[test]
fn tables_render_natively_in_every_format() {
    let md = "Intro.\n\n| Symbol | Meaning |\n|--------|---------|\n| pi | ratio |\n| e | Euler |\n\nOutro.\n";
    let dir = std::env::temp_dir().join(format!("mdall_tbl_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let p = |ext: &str| dir.join(format!("t.{ext}"));

    // Container formats: a native table element exists in the XML.
    export_formats::export_docx(md, &p("docx"), &meta(), None).unwrap();
    let docx = String::from_utf8_lossy(&zip_part(&p("docx"), "word/document.xml").unwrap()).into_owned();
    assert!(docx.contains("<w:tbl>"), "DOCX: markdown table not rendered as a Word table");
    export_formats::export_odt(md, &p("odt"), &meta(), None).unwrap();
    let odt = String::from_utf8_lossy(&zip_part(&p("odt"), "content.xml").unwrap()).into_owned();
    assert!(odt.contains("<table:table"), "ODT: markdown table not rendered as an ODF table");

    // Source formats: the format's own table idiom appears.
    let check = |ext: &str, needle: &str, run: &dyn Fn(&PathBuf)| {
        run(&p(ext));
        let t = fs::read_to_string(p(ext)).unwrap();
        assert!(t.contains(needle), "{ext}: table idiom '{needle}' missing - cells were dropped/garbled");
        // The body cells must survive (not just the header).
        assert!(t.contains("Euler"), "{ext}: table body row lost");
    };
    check("tex", r"\begin{tabular}", &|f| export_formats::export_tex(md, f, &meta()).unwrap());
    check("typ", "#table(", &|f| export_formats::export_typst_src(md, f, &meta()).unwrap());
    check("rst", ".. list-table::", &|f| export_formats::export_rst(md, f, &meta()).unwrap());
    check("org", "| Symbol", &|f| export_formats::export_org(md, f, &meta()).unwrap());
    check("adoc", "|===", &|f| export_formats::export_adoc(md, f, &meta()).unwrap());
    check("rtf", r"\trowd", &|f| export_formats::export_rtf(md, f, &meta(), None).unwrap());

    let _ = fs::remove_dir_all(&dir);
}

/// Inline `$x$` must stay inline in the sentence, not be hoisted into its own
/// centered display block (which used to fragment paragraphs and shred tables).
#[test]
fn inline_math_stays_inline() {
    let dir = std::env::temp_dir().join(format!("mdall_inl_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let md = "A sentence with $x^2$ in the middle of it.\n";

    let tex = dir.join("a.tex");
    export_formats::export_tex(md, &tex, &meta()).unwrap();
    let t = fs::read_to_string(&tex).unwrap();
    assert!(t.contains("$x^2$"), "TeX: inline math delimiters lost");
    assert!(!t.contains(r"\begin{equation}"), "TeX: inline math wrongly hoisted to a display equation");

    let typ = dir.join("a.typ");
    export_formats::export_typst_src(md, &typ, &meta()).unwrap();
    let y = fs::read_to_string(&typ).unwrap();
    // The sentence stays together (the word after the math is on the same line).
    assert!(y.contains("middle of it"), "Typst: sentence fragmented around inline math");

    let _ = fs::remove_dir_all(&dir);
}

// ── Reference formats: assets copied next to output + RST escaping ────────────

#[test]
fn reference_export_copies_assets_next_to_output() {
    let src_dir = std::env::temp_dir().join(format!("mdall_assrc_{}", std::process::id()));
    let out_dir = std::env::temp_dir().join(format!("mdall_assout_{}", std::process::id()));
    let _ = fs::create_dir_all(&src_dir);
    let _ = fs::create_dir_all(&out_dir);
    image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
        .save(src_dir.join("pic.png")).unwrap();
    let md_path = src_dir.join("doc.md");
    fs::write(&md_path, "# T\n\n![cap](pic.png)\n").unwrap();

    // Export to a DIFFERENT directory; the image must be copied alongside the .typ.
    crate::convert::convert_file(&md_path, &out_dir.join("doc.typ")).unwrap();
    assert!(out_dir.join("pic.png").is_file(), "referenced asset not copied next to .typ output");

    let _ = fs::remove_dir_all(&src_dir);
    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn rst_escapes_literal_pipe_so_docutils_does_not_see_a_substitution() {
    // |x| in prose must not become an RST substitution reference (a hard error).
    let out = tmp("rst_pipe", "rst");
    export_formats::export_rst("V = |det of the map| here.\n", &out, &meta()).unwrap();
    let t = fs::read_to_string(&out).unwrap();
    assert!(t.contains(r"\|det of the map\|"), "literal pipes not escaped in RST: {t}");
    cleanup(&out);
}
