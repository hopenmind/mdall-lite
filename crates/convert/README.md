<div align="center">
  <img src="../../assets/Logo.png" alt="MD -> ALL" width="200"/>
</div>

<br/>

# mdall-convert

Standalone CLI for the MD -> ALL conversion channel. Converts any supported
document to any supported format, fully offline, using the exact same
`mdall-core` engine as the editor: no pandoc, no LibreOffice, no browser.

## Build

```sh
cargo build --release -p mdall-convert
# -> target/release/mdall-convert
```

## Usage

```sh
mdall-convert <input> <output>     # formats inferred from extensions
mdall-convert --list-formats       # list supported import/export formats
mdall-convert --help
```

### Examples

```sh
mdall-convert paper.docx paper.pdf
mdall-convert thesis.tex  thesis.docx
mdall-convert notes.md    notes.html
```

## Reversible DOCX

A DOCX export embeds the full Markdown source (`md-to-all-source.xml`) and each
equation's LaTeX (PNG `tEXt` + SVG metadata). Re-importing that DOCX recovers the
original editable source losslessly:

```sh
mdall-convert paper.md   paper.docx   # export
mdall-convert paper.docx recovered.md # recover -> byte-identical to paper.md
```

This is the core MD -> ALL differentiator (see the project spec), shared verbatim
with the editor via `mdall_core::convert`.

## Relationship to the editor

`mdall-convert` and the `mdall` editor are two surfaces over one core. The
editor adds the WYSIWYG GUI; this binary is the headless surface and the seed of
the planned conversion MCP. Both call `mdall_core::convert::{import_to_md,
export_md, convert_file}`, so conversion behavior and reversibility are identical.
