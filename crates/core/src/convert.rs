//! Headless conversion hub: import any supported file to Markdown, and export
//! Markdown to any supported format. This is the single source of truth for the
//! conversion channel, shared by the egui editor and the standalone CLI/MCP.
//!
//! Reversibility invariant (project spec §3): a DOCX export embeds the full Markdown
//! in `md-to-all-source.xml` and the LaTeX of each equation in PNG/SVG metadata,
//! so `import_to_md` on a re-opened DOCX recovers the original editable LaTeX.

use std::path::Path;
use crate::export::PdfMetadata;
use crate::{export, export_formats, import, source_embed, text_encoding, latex_macros};

/// Apply scholarly export pre-passes to `markdown`: cross-reference numbering and
/// resolution (`\ref`/`\eqref`/`\cref`/`\label`, `{#id}`) and citation resolution
/// (`[@key]`/`\cite{}` against a `references.bib` next to the document, appending a
/// numbered reference list). A no-op for documents that use neither, so plain
/// exports are byte-for-byte unaffected. The editor SOURCE is never mutated - this
/// runs only on the markdown handed to the exporters.
pub fn apply_scholarly_passes(markdown: &str, source_dir: Option<&Path>) -> String {
    let md = crate::crossref::resolve_crossrefs(markdown);
    match source_dir.and_then(|d| std::fs::read_to_string(d.join("references.bib")).ok()) {
        Some(bib_src) => {
            let db = crate::bibliography::parse_bibtex(&bib_src);
            crate::bibliography::process_citations(&md, &db)
        }
        None => md,
    }
}

/// Import a file of any supported format into the internal Markdown representation.
/// For DOCX it first tries the lossless `md-to-all-source.xml` recovery, then
/// falls back to generic Word XML extraction.
pub fn import_to_md(path: &Path) -> Result<String, String> {
    // Foreign documents can be malformed or truncated; a hand-rolled parser must
    // never crash the whole app. Catch any unwind and surface it as an error so
    // the editor degrades gracefully instead of aborting.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| import_to_md_inner(path)))
        .unwrap_or_else(|_| {
            Err(format!(
                "failed to import {}: the file appears malformed or corrupted",
                path.display()
            ))
        })
}

fn import_to_md_inner(path: &Path) -> Result<String, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let read = |p: &Path| text_encoding::read_text(p);
    match ext.as_str() {
        "md" | "markdown" | "txt" | "rmd" | "qmd" | "rmarkdown" => read(path),
        "docx" => {
            // Lossless recovery first (embedded source + equation LaTeX), then fallback.
            if let Ok(md) = source_embed::import_docx_source(path) {
                return Ok(md);
            }
            import::docx_generic_to_md(path)
        }
        "html" | "htm" => import::html_to_md(&read(path)?),
        "epub" => import::epub_to_md(path),
        "odt" => import::odt_to_md(path),
        "rtf" => import::rtf_to_md(path),
        "tex" | "latex" => import::tex_to_md(&read(path)?),
        "org" => import::org_to_md(&read(path)?),
        "rst" => import::rst_to_md(&read(path)?),
        "wiki" | "mediawiki" => import::wiki_to_md(&read(path)?),
        "adoc" | "asciidoc" | "asc" => import::adoc_to_md(&read(path)?),
        "typ" => import::typ_to_md(&read(path)?),
        "ipynb" => import::ipynb_to_md(&read(path)?),
        "bib" => import::bib_to_md(&read(path)?),
        "fb2" => import::fb2_to_md(path),
        "pptx" => import::pptx_to_md(path),
        "eml" => import::eml_to_md(&read(path)?),
        "csv" | "tsv" => import::csv_to_md(&read(path)?),
        "py" => import::code_to_md(&read(path)?, "python"),
        "js" => import::code_to_md(&read(path)?, "javascript"),
        "ts" => import::code_to_md(&read(path)?, "typescript"),
        "rs" => import::code_to_md(&read(path)?, "rust"),
        "c" => import::code_to_md(&read(path)?, "c"),
        "cpp" | "cxx" | "cc" => import::code_to_md(&read(path)?, "cpp"),
        "java" => import::code_to_md(&read(path)?, "java"),
        "go" => import::code_to_md(&read(path)?, "go"),
        "rb" => import::code_to_md(&read(path)?, "ruby"),
        "php" => import::code_to_md(&read(path)?, "php"),
        "sh" | "bash" | "zsh" => import::code_to_md(&read(path)?, "bash"),
        "r" => import::code_to_md(&read(path)?, "r"),
        other => Err(format!("import of .{other} is not supported")),
    }
}

/// Export Markdown to the format implied by `output`'s extension.
/// Installs the document's custom LaTeX macros so equation renderers expand them.
pub fn export_md(markdown: &str, output: &Path, meta: &PdfMetadata, source_dir: Option<&Path>) -> Result<(), String> {
    // Make this document's \newcommand macros active for the equation renderers.
    latex_macros::install_from_source(markdown);
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let result = match ext.as_str() {
        "md" | "markdown" => std::fs::write(output, markdown).map_err(|e| e.to_string()),
        "pdf" => export::export_pdf(markdown, output, meta, source_dir),
        "html" | "htm" => export::export_html(markdown, output, meta, source_dir),
        "docx" => export_formats::export_docx(markdown, output, meta, source_dir),
        "txt" => export_formats::export_txt(markdown, output),
        "tex" | "latex" => export_formats::export_tex(markdown, output, meta),
        "rtf" => export_formats::export_rtf(markdown, output, meta, source_dir),
        "odt" => export_formats::export_odt(markdown, output, meta, source_dir),
        "epub" => export_formats::export_epub(markdown, output, meta, source_dir),
        "org" => export_formats::export_org(markdown, output, meta),
        "rst" => export_formats::export_rst(markdown, output, meta),
        "adoc" | "asciidoc" => export_formats::export_adoc(markdown, output, meta),
        "ipynb" => export_formats::export_ipynb(markdown, output, meta),
        "typ" => export_formats::export_typst_src(markdown, output, meta),
        other => Err(format!("export to .{other} is not supported")),
    };

    // Reference formats (tex/typ/rst/org/adoc/md) emit an image reference rather
    // than embedding the binary, so the figure file must sit next to the output
    // for the result to compile/render. Copy referenced images alongside it.
    if result.is_ok()
        && matches!(ext.as_str(), "tex" | "latex" | "typ" | "rst" | "org" | "adoc" | "asciidoc" | "md" | "markdown")
    {
        copy_referenced_assets(markdown, source_dir, output);
    }
    result
}

/// Copy each author figure referenced by the markdown next to `output`, so the
/// emitted reference (`\includegraphics{f.png}`, `#image("f.png")`, ...) resolves
/// when the user compiles/opens the exported file in its own folder.
///
/// Only relative, non-escaping reference paths are copied (no `..`, no absolute,
/// no remote); the destination mirrors the path exactly as referenced.
fn copy_referenced_assets(markdown: &str, source_dir: Option<&Path>, output: &Path) {
    use pulldown_cmark::{Event, Parser, Tag};
    let out_dir = match output.parent() {
        Some(d) => d,
        None => return,
    };
    for event in Parser::new(markdown) {
        if let Event::Start(Tag::Image { dest_url, .. }) = event {
            let src = dest_url.trim();
            if src.is_empty() || src.contains("..") || Path::new(src).is_absolute() {
                continue;
            }
            let lower = src.to_ascii_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
                continue;
            }
            let Some(resolved) = crate::figure_embed::resolve_local_image(src, source_dir) else {
                continue;
            };
            let dest = out_dir.join(src);
            if dest == resolved {
                continue; // already in place
            }
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&resolved, &dest);
        }
    }
}

/// Convert `input` to `output`, inferring both formats from their extensions.
/// The document title defaults to the input file stem.
pub fn convert_file(input: &Path, output: &Path) -> Result<(), String> {
    let markdown = import_to_md(input)?;
    let title = input.file_stem().and_then(|s| s.to_str()).unwrap_or("document").to_string();
    let meta = PdfMetadata { title, ..Default::default() };
    export_md(&markdown, output, &meta, input.parent())
}

/// Extensions accepted by [`import_to_md`].
pub fn supported_import_exts() -> &'static [&'static str] {
    &[
        "md", "markdown", "txt", "docx", "html", "htm", "epub", "odt", "rtf",
        "tex", "latex", "org", "rst", "wiki", "mediawiki", "adoc", "asciidoc",
        "asc", "typ", "ipynb", "bib", "fb2", "pptx", "eml", "csv", "tsv",
        "rmd", "qmd", "rmarkdown", "py", "js", "ts", "rs", "c", "cpp", "cxx",
        "cc", "java", "go", "rb", "php", "sh", "bash", "zsh", "r",
    ]
}

/// Extensions accepted by [`export_md`].
pub fn supported_export_exts() -> &'static [&'static str] {
    &[
        "md", "markdown", "pdf", "html", "htm", "docx", "txt", "tex", "latex",
        "rtf", "odt", "epub", "org", "rst", "adoc", "asciidoc", "ipynb", "typ",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_md_to_docx_and_back_recovers_latex() {
        // End-to-end through the shared hub: the reversibility invariant must hold.
        let dir = std::env::temp_dir();
        let md_in = dir.join("mdall_convert_in.md");
        let docx = dir.join("mdall_convert_out.docx");
        let src = "Title\n\nInline $a^2 + b^2 = c^2$ and display:\n\n$$\\sum_{i=1}^n i$$\n\nEnd.\n";
        std::fs::write(&md_in, src).unwrap();

        convert_file(&md_in, &docx).expect("md -> docx");
        let recovered = import_to_md(&docx).expect("docx -> md");

        let _ = std::fs::remove_file(&md_in);
        let _ = std::fs::remove_file(&docx);
        assert_eq!(recovered, src, "round-trip via convert hub not lossless: {recovered:?}");
    }
}
