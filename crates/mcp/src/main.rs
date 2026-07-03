//! mdall-mcp - a headless Model Context Protocol server over the MD -> ALL
//! conversion core. It exposes, as MCP tools, document conversion across 40+
//! formats, the lossless DOCX -> Markdown LaTeX recovery (the reversibility
//! differentiator), native LaTeX equation rendering (PNG/SVG), LaTeX -> Typst /
//! Unicode conversion, and structured document analysis. Fully offline,
//! dependency-light, no MCP SDK and no async runtime.
//!
//! Transport: newline-delimited JSON-RPC 2.0 on stdin/stdout (the MCP stdio
//! transport). One JSON object per line in, one per line out. Conversion is
//! synchronous, so there is no async runtime - just the core engine plus
//! `serde_json` for framing and `base64` for inline image results.

use base64::Engine as _;
use mdall_core::editor::{self, BlockKind};
use mdall_core::export::PdfMetadata;
use mdall_core::{convert, equation_renderer, export_typst, inline_math, render, source_embed, stats};
use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Write};
use std::path::Path;

/// MCP protocol revision this server speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server usage guidance returned to the client on `initialize`. An agent reads
/// this to understand the offline conversion + reversibility workflow.
const SERVER_INSTRUCTIONS: &str = "\
MD -> ALL conversion server. Pure Rust, fully offline, no external tools.

What it does:
- Convert documents across 40+ formats (convert_file, convert_batch); the target
  is chosen by the output file extension.
- Recover the ORIGINAL editable Markdown + LaTeX from a DOCX that this tool
  produced, even after a reviewer annotated it in Word (recover_source). Use
  inspect_docx first to check whether a DOCX is recoverable.
- Render a LaTeX equation to a PNG or SVG image (render_equation); the LaTeX is
  re-embedded in the image so the result stays recoverable.
- Convert a LaTeX equation to Typst or Unicode (convert_latex).
- Inspect content without converting: analyze_document (counts, outline, reading
  time) and extract_equations (every display + inline equation).

Typical scientific round-trip: convert_file a Markdown paper to .docx, the
supervisor annotates it in Word, then recover_source rebuilds the exact source.

All file paths must be absolute.";

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                // Non-empty but unparseable: reply with a JSON-RPC parse error.
                let _ = writeln!(out, "{}", err(Value::Null, -32700, "parse error"));
                let _ = out.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        if let Some(resp) = handle(method, req.get("params"), id) {
            let _ = writeln!(out, "{}", resp);
            let _ = out.flush();
        }
    }
}

/// Dispatch a JSON-RPC request. Returns `None` for notifications (no reply).
fn handle(method: &str, params: Option<&Value>, id: Option<Value>) -> Option<Value> {
    match method {
        "initialize" => Some(ok(
            id?,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {}, "prompts": {} },
                "serverInfo": { "name": "mdall-mcp", "version": env!("CARGO_PKG_VERSION") },
                "instructions": SERVER_INSTRUCTIONS
            }),
        )),
        // Lifecycle notifications carry no id and expect no response.
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(ok(id?, json!({}))),
        "tools/list" => Some(ok(id?, json!({ "tools": tool_defs() }))),
        "tools/call" => Some(handle_call(params, id?)),
        "prompts/list" => Some(ok(id?, json!({ "prompts": prompt_defs() }))),
        "prompts/get" => Some(handle_prompt_get(params, id?)),
        _ => id.map(|id| err(id, -32601, "method not found")),
    }
}

// ── Tool definitions ─────────────────────────────────────────────────────────

fn tool_defs() -> Value {
    json!([
        {
            "name": "list_formats",
            "description": "List every import and export format the conversion engine supports, as structured JSON.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "convert_file",
            "description": "Convert a document from one format to another, inferring both formats from the file extensions. Fully offline. DOCX export is reversible: the original Markdown + LaTeX is embedded for lossless recovery.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input":  { "type": "string", "description": "Absolute path to the source file (.md/.docx/.html/.tex/...)." },
                    "output": { "type": "string", "description": "Absolute path to write; the extension selects the target format (.pdf/.docx/.html/.typ/...)." }
                },
                "required": ["input", "output"],
                "additionalProperties": false
            }
        },
        {
            "name": "convert_batch",
            "description": "Convert many documents in one call. Each job is converted independently; one failure does not abort the rest. Returns a per-job result summary.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "jobs": {
                        "type": "array",
                        "description": "List of conversions to run.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "input":  { "type": "string", "description": "Absolute path to the source file." },
                                "output": { "type": "string", "description": "Absolute output path; the extension selects the format." }
                            },
                            "required": ["input", "output"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["jobs"],
                "additionalProperties": false
            }
        },
        {
            "name": "import_to_md",
            "description": "Import any supported document and return its Markdown representation (LaTeX equations preserved as $...$). Does not write a file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Absolute path to the source file." }
                },
                "required": ["input"],
                "additionalProperties": false
            }
        },
        {
            "name": "export_md",
            "description": "Export inline Markdown to a file in the format implied by the output extension. Optional title/author metadata. Referenced figures are resolved relative to base_dir (default: the output folder).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "markdown": { "type": "string", "description": "Markdown source to export." },
                    "output":   { "type": "string", "description": "Absolute output path; extension selects the format." },
                    "title":    { "type": "string", "description": "Optional document title." },
                    "author":   { "type": "string", "description": "Optional document author." },
                    "base_dir": { "type": "string", "description": "Optional folder to resolve relative image paths against (default: output folder)." }
                },
                "required": ["markdown", "output"],
                "additionalProperties": false
            }
        },
        {
            "name": "recover_source",
            "description": "Recover the ORIGINAL editable Markdown + LaTeX from a DOCX previously exported by MD -> ALL. This is the reversibility differentiator: a supervisor can annotate the DOCX in Word and the author recovers their exact source. Returns the recovered Markdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Absolute path to a .docx produced by MD -> ALL." }
                },
                "required": ["input"],
                "additionalProperties": false
            }
        },
        {
            "name": "inspect_docx",
            "description": "Check whether a DOCX is recoverable by MD -> ALL without performing a full conversion. Reports reversibility, the recovered size, and a short preview.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "Absolute path to a .docx file." }
                },
                "required": ["input"],
                "additionalProperties": false
            }
        },
        {
            "name": "render_equation",
            "description": "Render a LaTeX equation to a PNG or SVG image using the embedded Typst engine. The LaTeX is re-embedded in the image metadata so the result stays recoverable. With an output path, writes the file; without one, returns the image inline (PNG) or the SVG source.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "latex":  { "type": "string", "description": "LaTeX math, e.g. 'E = mc^2' or '\\int_0^1 x\\,dx'." },
                    "format": { "type": "string", "enum": ["png", "svg"], "description": "Output format (default png)." },
                    "output": { "type": "string", "description": "Optional absolute path to write the image to. If omitted, the result is returned inline." },
                    "scale":  { "type": "number", "description": "PNG scale factor (default 2.0)." }
                },
                "required": ["latex"],
                "additionalProperties": false
            }
        },
        {
            "name": "convert_latex",
            "description": "Convert a LaTeX equation to Typst markup and/or a Unicode approximation. Pure and instant; writes no file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "latex": { "type": "string", "description": "LaTeX math to convert." },
                    "to":    { "type": "string", "enum": ["typst", "unicode", "all"], "description": "Target representation (default all)." }
                },
                "required": ["latex"],
                "additionalProperties": false
            }
        },
        {
            "name": "extract_equations",
            "description": "Extract every equation (display and inline) from a document or Markdown string, returned as structured JSON. Provide either 'input' (a file path) or 'markdown'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input":    { "type": "string", "description": "Absolute path to a document to read first." },
                    "markdown": { "type": "string", "description": "Markdown source (used instead of 'input')." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "analyze_document",
            "description": "Analyze a document or Markdown string: word/character counts, heading outline, equation counts (display + inline), code blocks, lists, tables, images, links, and reading time. Provide either 'input' (a file path) or 'markdown'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input":    { "type": "string", "description": "Absolute path to a document to read first." },
                    "markdown": { "type": "string", "description": "Markdown source (used instead of 'input')." }
                },
                "additionalProperties": false
            }
        }
    ])
}

// ── Tool dispatch ─────────────────────────────────────────────────────────────

fn handle_call(params: Option<&Value>, id: Value) -> Value {
    let params = match params {
        Some(p) => p,
        None => return err(id, -32602, "missing params"),
    };
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    let result: Result<Vec<Value>, String> = match name {
        "list_formats" => call_list_formats(),
        "convert_file" => call_convert_file(&args),
        "convert_batch" => call_convert_batch(&args),
        "import_to_md" => call_import_to_md(&args),
        "export_md" => call_export_md(&args),
        "recover_source" => call_recover_source(&args),
        "inspect_docx" => call_inspect_docx(&args),
        "render_equation" => call_render_equation(&args),
        "convert_latex" => call_convert_latex(&args),
        "extract_equations" => call_extract_equations(&args),
        "analyze_document" => call_analyze_document(&args),
        other => Err(format!("unknown tool '{}'", other)),
    };

    match result {
        Ok(blocks) => ok(id, json!({ "content": blocks, "isError": false })),
        Err(msg) => ok(id, json!({ "content": [ text_block(&msg) ], "isError": true })),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or non-string argument '{}'", key))
}

/// Resolve a tool's Markdown input from either an inline `markdown` string or an
/// `input` file path (imported to Markdown first).
fn resolve_markdown(args: &Value) -> Result<String, String> {
    if let Some(md) = args.get("markdown").and_then(Value::as_str) {
        return Ok(md.to_string());
    }
    if let Some(input) = args.get("input").and_then(Value::as_str) {
        return convert::import_to_md(Path::new(input));
    }
    Err("provide either 'markdown' or 'input'".to_string())
}

fn call_list_formats() -> Result<Vec<Value>, String> {
    let imports = convert::supported_import_exts();
    let exports = convert::supported_export_exts();
    let v = json!({
        "import": { "count": imports.len(), "extensions": imports },
        "export": { "count": exports.len(), "extensions": exports },
        "reversible_export": "docx",
        "note": "DOCX export embeds the original Markdown + LaTeX; recover it with recover_source."
    });
    Ok(vec![text_block(&pretty(&v))])
}

fn call_convert_file(args: &Value) -> Result<Vec<Value>, String> {
    let input = str_arg(args, "input")?;
    let output = str_arg(args, "output")?;
    convert::convert_file(Path::new(input), Path::new(output))?;
    let size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    Ok(vec![text_block(&format!("converted: {} -> {} ({} bytes)", input, output, size))])
}

fn call_convert_batch(args: &Value) -> Result<Vec<Value>, String> {
    let jobs = args
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing 'jobs' array of {input, output}".to_string())?;
    let mut results = Vec::with_capacity(jobs.len());
    let (mut converted, mut failed) = (0usize, 0usize);
    for job in jobs {
        let input = job.get("input").and_then(Value::as_str).unwrap_or("");
        let output = job.get("output").and_then(Value::as_str).unwrap_or("");
        if input.is_empty() || output.is_empty() {
            failed += 1;
            results.push(json!({ "input": input, "output": output, "ok": false, "error": "missing input/output" }));
            continue;
        }
        match convert::convert_file(Path::new(input), Path::new(output)) {
            Ok(()) => {
                converted += 1;
                let size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
                results.push(json!({ "input": input, "output": output, "ok": true, "bytes": size }));
            }
            Err(e) => {
                failed += 1;
                results.push(json!({ "input": input, "output": output, "ok": false, "error": e }));
            }
        }
    }
    let summary = json!({ "total": jobs.len(), "converted": converted, "failed": failed, "results": results });
    Ok(vec![text_block(&pretty(&summary))])
}

fn call_import_to_md(args: &Value) -> Result<Vec<Value>, String> {
    let input = str_arg(args, "input")?;
    let md = convert::import_to_md(Path::new(input))?;
    Ok(vec![text_block(&md)])
}

fn call_export_md(args: &Value) -> Result<Vec<Value>, String> {
    let markdown = str_arg(args, "markdown")?;
    let output = str_arg(args, "output")?;
    let out_path = Path::new(output);
    let mut meta = PdfMetadata::default();
    if let Some(t) = args.get("title").and_then(Value::as_str) {
        meta.title = t.to_string();
    }
    if let Some(a) = args.get("author").and_then(Value::as_str) {
        meta.author = a.to_string();
    }
    // Resolve relative figures against base_dir, else the output's own folder.
    let base = args
        .get("base_dir")
        .and_then(Value::as_str)
        .map(|s| Path::new(s).to_path_buf())
        .or_else(|| out_path.parent().map(Path::to_path_buf));
    convert::export_md(markdown, out_path, &meta, base.as_deref())?;
    let size = std::fs::metadata(out_path).map(|m| m.len()).unwrap_or(0);
    Ok(vec![text_block(&format!("exported markdown -> {} ({} bytes)", output, size))])
}

fn call_recover_source(args: &Value) -> Result<Vec<Value>, String> {
    let input = str_arg(args, "input")?;
    let md = source_embed::import_docx_source(Path::new(input))?;
    Ok(vec![text_block(&md)])
}

fn call_inspect_docx(args: &Value) -> Result<Vec<Value>, String> {
    let input = str_arg(args, "input")?;
    let v = match source_embed::import_docx_source_detailed(Path::new(input)) {
        Ok((md, full)) => {
            let preview: String = md.chars().take(200).collect();
            json!({
                "input": input,
                "reversible": true,
                "fidelity": if full { "lossless" } else { "partial" },
                "recovered_chars": md.chars().count(),
                "recovered_bytes": md.len(),
                "preview": preview
            })
        }
        Err(e) => json!({ "input": input, "reversible": false, "reason": e }),
    };
    Ok(vec![text_block(&pretty(&v))])
}

fn call_render_equation(args: &Value) -> Result<Vec<Value>, String> {
    let latex = str_arg(args, "latex")?;
    let format = args.get("format").and_then(Value::as_str).unwrap_or("png");
    let output = args.get("output").and_then(Value::as_str);
    match format {
        "svg" => {
            let svg = equation_renderer::render_equation_svg(latex)
                .ok_or_else(|| "SVG rendering failed for this LaTeX".to_string())?;
            let svg = source_embed::embed_latex_in_svg(&svg, latex);
            if let Some(out) = output {
                std::fs::write(out, &svg).map_err(|e| format!("write failed: {}", e))?;
                Ok(vec![text_block(&format!(
                    "wrote {} ({} bytes), LaTeX embedded in <metadata>",
                    out,
                    svg.len()
                ))])
            } else {
                Ok(vec![text_block(&svg)])
            }
        }
        "png" => {
            let scale = args.get("scale").and_then(Value::as_f64).unwrap_or(2.0) as f32;
            let (png_opt, err_opt) = equation_renderer::render_equation_png(latex, scale);
            let png = png_opt
                .ok_or_else(|| err_opt.unwrap_or_else(|| "PNG rendering failed".to_string()))?;
            let png = source_embed::embed_latex_in_png(&png, latex);
            if let Some(out) = output {
                std::fs::write(out, &png).map_err(|e| format!("write failed: {}", e))?;
                Ok(vec![text_block(&format!(
                    "wrote {} ({} bytes), LaTeX embedded in PNG tEXt chunk",
                    out,
                    png.len()
                ))])
            } else {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
                Ok(vec![
                    image_block(&b64, "image/png"),
                    text_block(&format!("rendered LaTeX -> PNG ({} bytes, scale {})", png.len(), scale)),
                ])
            }
        }
        other => Err(format!("unknown format '{}': use 'png' or 'svg'", other)),
    }
}

fn call_convert_latex(args: &Value) -> Result<Vec<Value>, String> {
    let latex = str_arg(args, "latex")?;
    let to = args.get("to").and_then(Value::as_str).unwrap_or("all");
    let mut v = Map::new();
    if to == "typst" || to == "all" {
        v.insert("typst".to_string(), json!(export_typst::latex_to_typst_math(latex)));
    }
    if to == "unicode" || to == "all" {
        v.insert("unicode".to_string(), json!(render::latex_to_unicode(latex)));
    }
    if v.is_empty() {
        return Err(format!("unknown target '{}': use 'typst', 'unicode', or 'all'", to));
    }
    Ok(vec![text_block(&pretty(&Value::Object(v)))])
}

fn call_extract_equations(args: &Value) -> Result<Vec<Value>, String> {
    let md = resolve_markdown(args)?;
    let display = render::extract_equations(&md);
    let inline = inline_equations(&md);
    let v = json!({
        "display": display,
        "inline": inline,
        "counts": { "display": display.len(), "inline": inline.len(), "total": display.len() + inline.len() }
    });
    Ok(vec![text_block(&pretty(&v))])
}

fn call_analyze_document(args: &Value) -> Result<Vec<Value>, String> {
    let md = resolve_markdown(args)?;
    Ok(vec![text_block(&pretty(&analyze_markdown(&md)))])
}

// ── Analysis helpers ───────────────────────────────────────────────────────────

/// Collect every inline equation (`$...$` / `\(...\)`) in text-bearing blocks,
/// excluding display-equation and code blocks.
fn inline_equations(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    for b in editor::parse_document(md) {
        if matches!(b.kind, BlockKind::DisplayEquation { .. } | BlockKind::FencedCode { .. }) {
            continue;
        }
        let raw = b.raw_source(md);
        if inline_math::needs_reparse(raw) {
            for run in inline_math::split_inline(raw) {
                if let inline_math::InlineRun::Equation { latex, .. } = run {
                    out.push(latex);
                }
            }
        }
    }
    out
}

/// Build a structured analysis of a Markdown document from the authoritative
/// block parser plus the word counter.
fn analyze_markdown(md: &str) -> Value {
    let blocks = editor::parse_document(md);
    let mut headings: Vec<Value> = Vec::new();
    let mut code_langs: Vec<String> = Vec::new();
    let (mut paragraphs, mut display_eq, mut code_blocks) = (0usize, 0usize, 0usize);
    let (mut bullet, mut ordered, mut quotes) = (0usize, 0usize, 0usize);
    let (mut tables, mut rules, mut html) = (0usize, 0usize, 0usize);
    let mut inline_eq = 0usize;

    for b in &blocks {
        let raw = b.raw_source(md);
        match &b.kind {
            BlockKind::Heading(level) => {
                let text = raw.trim().trim_start_matches('#').trim().to_string();
                headings.push(json!({ "level": level, "text": text }));
            }
            BlockKind::Paragraph => paragraphs += 1,
            BlockKind::DisplayEquation { .. } => display_eq += 1,
            BlockKind::FencedCode { lang } => {
                code_blocks += 1;
                if !lang.is_empty() {
                    code_langs.push(lang.clone());
                }
            }
            BlockKind::BulletList => bullet += 1,
            BlockKind::OrderedList => ordered += 1,
            BlockKind::BlockQuote => quotes += 1,
            BlockKind::HorizontalRule => rules += 1,
            BlockKind::Table => tables += 1,
            BlockKind::HtmlBlock => html += 1,
        }
        if !matches!(b.kind, BlockKind::DisplayEquation { .. } | BlockKind::FencedCode { .. })
            && inline_math::needs_reparse(raw)
        {
            for run in inline_math::split_inline(raw) {
                if matches!(run, inline_math::InlineRun::Equation { .. }) {
                    inline_eq += 1;
                }
            }
        }
    }

    let words = stats::word_count(md);
    let images = md.matches("![").count();
    let links = md.matches("](").count().saturating_sub(images);
    let reading_min = ((words as f64) / 200.0).ceil() as usize;

    json!({
        "words": words,
        "characters": md.chars().count(),
        "lines": md.lines().count(),
        "reading_time_min": reading_min,
        "headings": { "count": headings.len(), "outline": headings },
        "equations": { "display": display_eq, "inline": inline_eq, "total": display_eq + inline_eq },
        "code_blocks": { "count": code_blocks, "languages": code_langs },
        "lists": { "bullet": bullet, "ordered": ordered },
        "blockquotes": quotes,
        "tables": tables,
        "horizontal_rules": rules,
        "html_blocks": html,
        "paragraphs": paragraphs,
        "images": images,
        "links": links
    })
}

// ── Prompts ────────────────────────────────────────────────────────────────────

fn prompt_defs() -> Value {
    json!([
        {
            "name": "scientific_roundtrip",
            "description": "Guide the lossless MD -> DOCX -> review -> recover workflow for a paper.",
            "arguments": [ { "name": "paper_path", "description": "Absolute path to the source .md paper.", "required": true } ]
        },
        {
            "name": "extract_math",
            "description": "Pull every equation (display + inline) out of a document.",
            "arguments": [ { "name": "document_path", "description": "Absolute path to the document.", "required": true } ]
        }
    ])
}

fn handle_prompt_get(params: Option<&Value>, id: Value) -> Value {
    let params = match params {
        Some(p) => p,
        None => return err(id, -32602, "missing params"),
    };
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arg = |k: &str| {
        params
            .get("arguments")
            .and_then(|a| a.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let text = match name {
        "scientific_roundtrip" => {
            let p = arg("paper_path");
            format!(
                "Run the lossless review round-trip for the paper at {p}.\n\
                 1. inspect the source, then convert_file {{ input: \"{p}\", output: \"<paper>.docx\" }}.\n\
                 2. The supervisor annotates the .docx in Word and returns it.\n\
                 3. recover_source {{ input: \"<reviewed>.docx\" }} -> the exact original Markdown + LaTeX.\n\
                 Use inspect_docx on the returned file first to confirm it is recoverable."
            )
        }
        "extract_math" => {
            let p = arg("document_path");
            format!(
                "Extract every equation from {p}.\n\
                 Call extract_equations {{ input: \"{p}\" }} to get display and inline LaTeX,\n\
                 then optionally render_equation each one to PNG/SVG or convert_latex to Typst/Unicode."
            )
        }
        _ => return err(id, -32602, "unknown prompt"),
    };
    ok(
        id,
        json!({ "messages": [ { "role": "user", "content": { "type": "text", "text": text } } ] }),
    )
}

// ── JSON-RPC + content helpers ─────────────────────────────────────────────────

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A single MCP text content block.
fn text_block(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

/// A single MCP image content block (base64 data + MIME type).
fn image_block(data_b64: &str, mime: &str) -> Value {
    json!({ "type": "image", "data": data_b64, "mimeType": mime })
}

/// Pretty-print a JSON value for a text result, falling back to compact form.
fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> Value {
        handle_call(Some(&json!({ "name": name, "arguments": args })), json!(1))
    }

    fn first_text(v: &Value) -> String {
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    }

    #[test]
    fn initialize_advertises_tools_and_prompts_and_instructions() {
        let init = handle("initialize", Some(&json!({})), Some(json!(1))).unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], "mdall-mcp");
        assert!(init["result"]["capabilities"]["prompts"].is_object());
        assert!(init["result"]["instructions"].as_str().unwrap().contains("recover"));
    }

    #[test]
    fn tools_list_has_eleven_tools() {
        let list = handle("tools/list", None, Some(json!(2))).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 11);
    }

    #[test]
    fn prompts_list_and_get_work() {
        let list = handle("prompts/list", None, Some(json!(3))).unwrap();
        assert_eq!(list["result"]["prompts"].as_array().unwrap().len(), 2);
        let got = handle(
            "prompts/get",
            Some(&json!({ "name": "extract_math", "arguments": { "document_path": "/x.md" } })),
            Some(json!(4)),
        )
        .unwrap();
        assert!(got["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("/x.md"));
    }

    #[test]
    fn initialized_notification_gets_no_reply() {
        assert!(handle("notifications/initialized", None, None).is_none());
    }

    #[test]
    fn unparseable_input_is_not_fatal_here() {
        // The main loop replies with a parse error; handle() itself rejects unknown methods.
        let r = handle("garbage/method", None, Some(json!(7)));
        assert_eq!(r.unwrap()["error"]["code"], -32601);
    }

    #[test]
    fn convert_then_recover_round_trips_latex_via_tools() {
        // The reversibility differentiator, exercised through the MCP tool layer.
        let dir = std::env::temp_dir().join(format!("mdall_mcp_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("d.md");
        std::fs::write(&md, "# T\n\nInline $E=mc^2$ and\n\n$$\\int_0^1 x\\,dx = \\frac{1}{2}$$\n").unwrap();
        let docx = dir.join("d.docx");
        let conv = call("convert_file", json!({ "input": md.to_str().unwrap(), "output": docx.to_str().unwrap() }));
        assert_eq!(conv["result"]["isError"], false);
        let rec = call("recover_source", json!({ "input": docx.to_str().unwrap() }));
        let recovered = first_text(&rec);
        assert!(recovered.contains("$E=mc^2$"), "inline LaTeX not recovered: {recovered:?}");
        assert!(recovered.contains("\\frac{1}{2}"), "display LaTeX not recovered: {recovered:?}");

        // inspect_docx reports the same DOCX as recoverable, and lossless since
        // it was just exported (the full source entry is present).
        let ins = first_text(&call("inspect_docx", json!({ "input": docx.to_str().unwrap() })));
        assert!(ins.contains("\"reversible\": true"));
        assert!(ins.contains("\"fidelity\": \"lossless\""), "fresh export should be lossless: {ins}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_tool_is_an_error_result_not_a_crash() {
        let r = call("nope", json!({}));
        assert_eq!(r["result"]["isError"], true);
    }

    #[test]
    fn list_formats_reports_import_and_export() {
        let t = first_text(&call("list_formats", json!({})));
        assert!(t.contains("\"import\"") && t.contains("\"export\""));
        assert!(t.contains("docx") && t.contains("pdf"));
    }

    #[test]
    fn convert_latex_returns_typst_and_unicode() {
        let t = first_text(&call("convert_latex", json!({ "latex": "\\alpha + \\beta" })));
        assert!(t.contains("typst"));
        assert!(t.contains("unicode"));
    }

    #[test]
    fn analyze_document_counts_structure() {
        let md = "# Title\n\nA paragraph with inline $x^2$.\n\n$$E=mc^2$$\n\n- a\n- b\n";
        let t = first_text(&call("analyze_document", json!({ "markdown": md })));
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["headings"]["count"], 1);
        assert_eq!(v["equations"]["display"], 1);
        assert_eq!(v["equations"]["inline"], 1);
    }

    #[test]
    fn extract_equations_separates_display_and_inline() {
        let md = "Inline $a+b$ here.\n\n$$\\int x\\,dx$$\n";
        let t = first_text(&call("extract_equations", json!({ "markdown": md })));
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["counts"]["inline"], 1);
        assert_eq!(v["counts"]["display"], 1);
    }

    #[test]
    fn convert_batch_reports_per_job_results() {
        let dir = std::env::temp_dir().join(format!("mdall_mcp_batch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("a.md");
        std::fs::write(&md, "# A\n\ntext\n").unwrap();
        let html = dir.join("a.html");
        let t = first_text(&call(
            "convert_batch",
            json!({ "jobs": [
                { "input": md.to_str().unwrap(), "output": html.to_str().unwrap() },
                { "input": "/nonexistent/zzz.md", "output": "/tmp/zzz.html" }
            ] }),
        ));
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["converted"], 1);
        assert_eq!(v["failed"], 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
