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

use mdall_core::{convert, purify};

/// Centralized user-facing strings (English base, i18n-ready: swap this table).
mod msg {
    pub const USAGE: &str = "\
mdall-convert - offline document converter (MD -> ALL engine)

USAGE:
    mdall-convert <input> <output>
    mdall-convert --clean[=audit|sanitize|decontaminate] <input> [output]
    mdall-convert --list-formats
    mdall-convert --help

The input and output formats are inferred from the file extensions.

EXAMPLES:
    mdall-convert paper.docx paper.pdf
    mdall-convert thesis.tex thesis.docx
    mdall-convert notes.md notes.html

DOCX exports embed the original Markdown + equation LaTeX, so re-importing a
DOCX into MD -> ALL (or this tool) recovers the original editable source.

--clean strips LLM watermarks and encoding artifacts (hidden Unicode, homoglyphs,
unicode dashes, CRLF). Modes:
    audit          report only, never writes
    sanitize       strip watermarks + normalize encoding (default; writes in place
                   or to [output])
    decontaminate  also remove LLM tics + French typography (prose zones only)
Frozen zones (code, YAML/JSON, front matter) and MATH zones ($...$, $$...$$) are
always preserved.";

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

/// Parse the clean mode from a `--clean` or `--clean=MODE` flag (default sanitize).
fn parse_clean_mode(flag: &str) -> Result<purify::PurifyMode, String> {
    let mode = flag
        .strip_prefix("--clean")
        .and_then(|s| s.strip_prefix('='))
        .unwrap_or("sanitize");
    match mode {
        "" | "sanitize" => Ok(purify::PurifyMode::Sanitize),
        "audit" => Ok(purify::PurifyMode::Audit),
        "decontaminate" => Ok(purify::PurifyMode::Decontaminate),
        other => Err(format!(
            "error: unknown clean mode '{}' (use audit|sanitize|decontaminate)",
            other
        )),
    }
}

/// Handle `mdall-convert --clean[=MODE] <input> [output]`: purify the file,
/// print the JSON report; non-audit modes write the result (in place, or to
/// [output]). Math zones and code are preserved by the purify core.
fn run_clean(flag: &str, rest: &[String]) -> Result<(), String> {
    let mode = parse_clean_mode(flag)?;
    let input_str = match rest.first() {
        Some(s) => s.as_str(),
        None => {
            return Err("error: --clean needs an <input> file (optionally an <output>)".to_string())
        }
    };
    let input = Path::new(input_str);
    if !input.is_file() {
        return Err(format!("{} {}", msg::INPUT_NOT_FOUND, input.display()));
    }
    let bytes = std::fs::read(input)
        .map_err(|e| format!("error: cannot read {}: {}", input.display(), e))?;
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let opts = purify::PurifyOptions { mode, ..Default::default() };
    let outcome = purify::purify_str(&raw, Some(input_str), &opts);
    println!("{}", purify::report_json(&outcome.report));
    if mode != purify::PurifyMode::Audit {
        let out = rest.get(1).map(Path::new).unwrap_or(input);
        std::fs::write(out, outcome.text.as_bytes())
            .map_err(|e| format!("error: cannot write {}: {}", out.display(), e))?;
        println!("cleaned: {} -> {}", input.display(), out.display());
    }
    Ok(())
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
        Some(a) if a == "--clean" || a.starts_with("--clean=") => {
            return run_clean(a, &args[1..]);
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
