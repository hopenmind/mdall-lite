//! `mdall-convert` - standalone CLI for the MD -> ALL conversion channel.
//!
//! Converts any supported document to any supported format, fully offline, using
//! the exact same `mdall-core` engine as the editor - including the lossless DOCX
//! LaTeX round-trip (export embeds the source; re-import recovers editable LaTeX).
//!
//! Usage:
//!   mdall-convert <input> <output>      convert by file extensions
//!   mdall-convert --list-formats        list supported import/export formats
//!   mdall-convert --help                show usage

use std::path::Path;
use std::process::ExitCode;

use mdall_core::convert;

/// Centralized user-facing strings (English base, i18n-ready: swap this table).
mod msg {
    pub const USAGE: &str = "\
mdall-convert - offline document converter (MD -> ALL engine)

USAGE:
    mdall-convert <input> <output>
    mdall-convert --list-formats
    mdall-convert --help

The input and output formats are inferred from the file extensions.

EXAMPLES:
    mdall-convert paper.docx paper.pdf
    mdall-convert thesis.tex thesis.docx
    mdall-convert notes.md notes.html

DOCX exports embed the original Markdown + equation LaTeX, so re-importing a
DOCX into MD -> ALL (or this tool) recovers the original editable source.";

    pub const NEED_TWO_ARGS: &str = "error: expected <input> <output> (see --help)";
    pub const INPUT_NOT_FOUND: &str = "error: input file not found:";
    pub const OK_PREFIX: &str = "converted:";
    pub const FAIL_PREFIX: &str = "error:";
}

fn list_formats() {
    println!("Import formats ({}):", convert::supported_import_exts().len());
    println!("  {}", convert::supported_import_exts().join(" "));
    println!("Export formats ({}):", convert::supported_export_exts().len());
    println!("  {}", convert::supported_export_exts().join(" "));
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") | None => {
            println!("{}", msg::USAGE);
            return Ok(());
        }
        Some("--list-formats") | Some("-l") => {
            list_formats();
            return Ok(());
        }
        _ => {}
    }

    if args.len() != 2 {
        return Err(msg::NEED_TWO_ARGS.to_string());
    }
    let input = Path::new(&args[0]);
    let output = Path::new(&args[1]);

    if !input.is_file() {
        return Err(format!("{} {}", msg::INPUT_NOT_FOUND, input.display()));
    }

    convert::convert_file(input, output)?;
    println!("{} {} -> {}", msg::OK_PREFIX, input.display(), output.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let e = if e.starts_with(msg::FAIL_PREFIX) || e.starts_with("error") {
                e
            } else {
                format!("{} {}", msg::FAIL_PREFIX, e)
            };
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
