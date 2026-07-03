//! `mdall-core` - the core of MD -> ALL.
//!
//! Holds all the document, conversion and equation-rendering logic. A PURE core
//! with no egui: the boundary is enforced by the compiler, since this crate
//! declares no UI dependency. The egui binary (`mdall`) consumes this core, so
//! the logic stays testable in isolation and reusable.
//!
//! Self-contained: no external resource at runtime. The Typst fonts and the
//! KaTeX assets are embedded via `include_bytes!` / `include_str!`.

pub mod bibliography;
pub mod convert;
pub mod crossref;
pub mod docx_review;
pub mod editor;
pub mod equation_renderer;
pub mod export;
pub mod export_engine;
pub mod export_formats;
pub mod figure_embed;
pub mod fonts;
pub mod export_typst;
pub mod import;
pub(crate) mod import_xml;
pub mod inline_math;
pub mod latex_macros;
pub mod render;
pub mod source_embed;
pub mod spell;
pub mod stats;
pub mod text_encoding;
// NOTE: `wysiwyg` is UI (egui LayoutJob building) and lives in the binary, not here.

#[cfg(test)]
mod tests_conversion;
