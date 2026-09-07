// wysiwyg.rs - WYSIWYG rich-text layout for the preview inline editor.
//
// Converts raw markdown block text into an egui LayoutJob that renders
// formatting (bold, italic, code, inline math, etc.) directly.
// Markdown syntax delimiters (**,  *, #, `, >, -) are rendered as very faint
// tiny text so the user edits markdown but sees formatted output.
//
// Usage:
//   let job = wysiwyg::build_layout_job(text, wrap_width, font_size, ui.visuals(), tag);
//   let galley = ui.fonts(|f| f.layout_job(job));

use eframe::egui;
use egui::{
    text::LayoutJob,
    Align, Color32, FontFamily, FontId, Stroke, TextFormat,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

// ── Block context tag ─────────────────────────────────────────────────────────

/// Which kind of block is being rendered - determines layout strategy.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WysiwygTag {
    Paragraph,
    Heading(u8),
    BulletList,
    OrderedList,
    BlockQuote,
    FencedCode,
    Other,
}

impl WysiwygTag {
    pub fn from_block_kind(kind: &mdall_core::editor::BlockKind) -> Self {
        match kind {
            mdall_core::editor::BlockKind::Heading(n)       => Self::Heading(*n),
            mdall_core::editor::BlockKind::Paragraph         => Self::Paragraph,
            mdall_core::editor::BlockKind::BulletList        => Self::BulletList,
            mdall_core::editor::BlockKind::OrderedList       => Self::OrderedList,
            mdall_core::editor::BlockKind::BlockQuote        => Self::BlockQuote,
            mdall_core::editor::BlockKind::FencedCode { .. } => Self::FencedCode,
            _                                           => Self::Other,
        }
    }
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Build a LayoutJob for displaying a markdown block in the WYSIWYG inline editor.
///
/// Syntax markers are rendered in faint text (barely visible), formatted
/// content is rendered with appropriate styling:
///   - bold → strong text color
///   - italic → italic
///   - `code` → monospace + code background
///   - $math$ → purple monospace
///   - # Heading → large text with faint # prefix
///   - - item → colored bullet, inline-formatted content
///   - > quote → indented, colored italic
pub fn build_layout_job(
    text: &str,
    wrap_width: f32,
    base_size: f32,
    visuals: &egui::Visuals,
    tag: WysiwygTag,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.wrap.break_anywhere = false;

    if text.is_empty() {
        return job;
    }

    match tag {
        WysiwygTag::Heading(level) => heading_job(text, base_size, visuals, level, &mut job),
        WysiwygTag::BulletList     => list_job(text, wrap_width, base_size, visuals, false, &mut job),
        WysiwygTag::OrderedList    => list_job(text, wrap_width, base_size, visuals, true, &mut job),
        WysiwygTag::BlockQuote     => blockquote_job(text, wrap_width, base_size, visuals, &mut job),
        WysiwygTag::FencedCode     => fenced_code_job(text, base_size, visuals, &mut job),
        WysiwygTag::Paragraph | WysiwygTag::Other => {
            inline_job(text, wrap_width, base_size, visuals, &mut job);
        }
    }

    // Safety net: never return an empty job for non-empty text
    if job.text.is_empty() && !text.is_empty() {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: FontId::new(base_size, FontFamily::Proportional),
                color: visuals.text_color(),
                ..Default::default()
            },
        );
    }

    job
}

// ── Shared formatting helpers ─────────────────────────────────────────────────

/// Append a styled text run to the job.
#[inline]
fn push(
    job: &mut LayoutJob,
    text: &str,
    size: f32,
    color: Color32,
    italic: bool,
    mono: bool,
    strikethrough: Stroke,
    background: Color32,
) {
    if text.is_empty() { return; }
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: FontId::new(size, if mono { FontFamily::Monospace } else { FontFamily::Proportional }),
            color,
            italics: italic,
            strikethrough,
            background,
            ..Default::default()
        },
    );
}

/// Very faint variant of a color - used for hidden markdown syntax markers.
#[inline]
fn faint(c: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), 48)
}

/// Bold font family (Cambria Bold loaded at startup).
#[inline]
fn bold_font() -> FontFamily {
    FontFamily::Name("CambriaBold".into())
}

/// Italic font family (Cambria Italic loaded at startup).
#[inline]
fn italic_font() -> FontFamily {
    FontFamily::Name("CambriaItalic".into())
}

/// Fill the byte range `[from..to]` in `text` with faint small text (markdown delimiters).
fn fill_gap(job: &mut LayoutJob, text: &str, from: usize, to: usize, size: f32, color: Color32) {
    if from >= to { return; }
    let from = from.min(text.len());
    let to   = to.min(text.len());
    if from >= to { return; }
    let gap = &text[from..to];
    if gap.is_empty() { return; }
    job.append(
        gap,
        0.0,
        TextFormat {
            font_id: FontId::new(size * 0.6, FontFamily::Proportional),
            color,
            ..Default::default()
        },
    );
}

// ── Inline pulldown-cmark parser ──────────────────────────────────────────────

/// Parse inline markdown with pulldown-cmark offset iterator.
/// Delimiters fill the gaps between Text events as faint text.
/// Inline HTML formatting state, toggled by `<span>/<u>/<mark>/<sup>/<sub>` tags.
/// Applied to the visible text so the EDITOR renders the effect (colored text,
/// underline, highlight, super/subscript) instead of showing raw HTML code.
#[derive(Default, Clone, Copy)]
struct InlineHtmlState {
    color: Option<Color32>,
    bg: Option<Color32>,
    underline: bool,
    vshift: i8, // +1 = superscript, -1 = subscript, 0 = none
    font_size: Option<f32>, // <span style="font-size:Npt"> per-selection size
}

/// Parse a `#rgb` or `#rrggbb` hex color appearing after `key` in a style attr.
fn style_color(tag: &str, key: &str) -> Option<Color32> {
    let low = tag.to_ascii_lowercase();
    let kpos = low.find(key)?;
    let after = &tag[kpos + key.len()..];
    let hpos = after.find('#')?;
    let hex: String = after[hpos + 1..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .take(6)
        .collect();
    let parse2 = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(parse2(&hex[0..2])?, parse2(&hex[2..4])?, parse2(&hex[4..6])?)),
        3 => {
            let dup = |c: char| u8::from_str_radix(&format!("{c}{c}"), 16).ok();
            let b = hex.as_bytes();
            Some(Color32::from_rgb(dup(b[0] as char)?, dup(b[1] as char)?, dup(b[2] as char)?))
        }
        _ => None,
    }
}

/// Parse a `font-size:Npt` / `Npx` value (returns the number in points/pixels).
fn style_font_size(tag: &str) -> Option<f32> {
    let low = tag.to_ascii_lowercase();
    let kpos = low.find("font-size")?;
    let after = &low[kpos + "font-size".len()..];
    let cpos = after.find(':')?;
    let num: String = after[cpos + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num.parse::<f32>().ok().filter(|n| *n > 0.0 && *n < 400.0)
}

/// Mutate the inline-HTML state from a single tag token (opening or closing).
fn apply_inline_html(tag: &str, st: &mut InlineHtmlState) {
    let t = tag.trim();
    let low = t.to_ascii_lowercase();
    if low.starts_with("</") {
        if low.starts_with("</span") {
            st.color = None;
            st.font_size = None;
        } else if low.starts_with("</mark") {
            st.bg = None;
        } else if low.starts_with("</u>") {
            st.underline = false;
        } else if low.starts_with("</sup") || low.starts_with("</sub") {
            st.vshift = 0;
        }
        return;
    }
    if low.starts_with("<span") {
        if let Some(c) = style_color(t, "color") {
            st.color = Some(c);
        }
        if let Some(sz) = style_font_size(t) {
            st.font_size = Some(sz);
        }
    } else if low.starts_with("<mark") {
        st.bg = style_color(t, "background").or(Some(Color32::from_rgb(255, 245, 130)));
    } else if low == "<u>" || low.starts_with("<u ") {
        st.underline = true;
    } else if low.starts_with("<sup") {
        st.vshift = 1;
    } else if low.starts_with("<sub") {
        st.vshift = -1;
    }
}

fn inline_job(
    text: &str,
    _wrap_width: f32,
    base_size: f32,
    visuals: &egui::Visuals,
    job: &mut LayoutJob,
) {
    let normal   = visuals.text_color();
    let strong   = visuals.strong_text_color();
    let dim      = faint(normal);
    let code_bg  = visuals.code_bg_color;
    // Inline math color: light purple
    let math_fg  = Color32::from_rgb(130, 80, 210);
    let math_bg  = Color32::from_rgba_unmultiplied(180, 160, 255, 25);

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_MATH);

    let mut bold   = false;
    let mut italic = false;
    let mut strike = false;
    let mut html   = InlineHtmlState::default();
    let mut last_end: usize = 0;

    for (event, range) in Parser::new_ext(text, opts).into_offset_iter() {
        let ts = range.start.min(text.len());
        let te = range.end.min(text.len());

        match event {
            // ── Visible content events ──────────────────────────────────────
            Event::Text(_) => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                if ts < te {
                    // Base color from markdown weight, overridden by an inline <span color>.
                    let color = html.color.unwrap_or(if bold { strong } else { normal });
                    let strike_st = if strike { Stroke::new(1.0, color) } else { Stroke::NONE };
                    let underline_st = if html.underline { Stroke::new(1.0, color) } else { Stroke::NONE };
                    // Use dedicated font faces for bold/italic so weight is physically correct
                    let family = match (bold, italic) {
                        (true,  _)     => bold_font(),
                        (false, true)  => italic_font(),
                        (false, false) => FontFamily::Proportional,
                    };
                    // Per-selection font-size if present, else heading/base size;
                    // super/subscript shrinks whichever applies.
                    let size = {
                        let base = html.font_size.unwrap_or(base_size);
                        if html.vshift != 0 { base * 0.75 } else { base }
                    };
                    let valign = if html.vshift > 0 { Align::TOP } else { Align::BOTTOM };
                    job.append(&text[ts..te], 0.0, TextFormat {
                        font_id: FontId::new(size, family),
                        color,
                        italics: italic && !bold, // let font handle italic weight; egui slant for plain italic
                        underline: underline_st,
                        strikethrough: strike_st,
                        background: html.bg.unwrap_or(Color32::TRANSPARENT),
                        valign,
                        ..Default::default()
                    });
                }
                last_end = te;
            }
            Event::Code(_) => {
                // Inline `code` - include backticks in the styled span
                fill_gap(job, text, last_end, ts, base_size, dim);
                if ts < te {
                    push(job, &text[ts..te], base_size * 0.88, normal, false, true, Stroke::NONE, code_bg);
                }
                last_end = te;
            }
            Event::InlineMath(_) => {
                // $...$ inline equation - show with purple tint
                fill_gap(job, text, last_end, ts, base_size, dim);
                if ts < te {
                    push(job, &text[ts..te], base_size * 0.88, math_fg, false, true, Stroke::NONE, math_bg);
                }
                last_end = te;
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                if ts < te {
                    // Update formatting state from the tag, then render the tag
                    // itself very faint and small so it recedes - the EFFECT is
                    // applied to the text, the raw HTML no longer dominates.
                    // (Chars are kept, not dropped, so the editor cursor stays aligned.)
                    apply_inline_html(&text[ts..te], &mut html);
                    push(job, &text[ts..te], base_size * 0.62, faint(dim), false, false, Stroke::NONE, Color32::TRANSPARENT);
                }
                last_end = te;
            }
            Event::SoftBreak => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                push(job, "\n", base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
                last_end = te;
            }
            Event::HardBreak => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                push(job, "\n\n", base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
                last_end = te;
            }

            // ── Formatting state changes - render closing delimiter as faint ─
            Event::Start(Tag::Strong)        => { bold   = true; }
            Event::End(TagEnd::Strong)       => {
                fill_gap(job, text, last_end, te, base_size, dim);
                bold = false; last_end = te;
            }
            Event::Start(Tag::Emphasis)      => { italic = true; }
            Event::End(TagEnd::Emphasis)     => {
                fill_gap(job, text, last_end, te, base_size, dim);
                italic = false; last_end = te;
            }
            Event::Start(Tag::Strikethrough) => { strike = true; }
            Event::End(TagEnd::Strikethrough)=> {
                fill_gap(job, text, last_end, te, base_size, dim);
                strike = false; last_end = te;
            }

            _ => {}
        }
    }

    // Trailing bytes not consumed by events (e.g. trailing newline, stray chars)
    if last_end < text.len() {
        push(job, &text[last_end..], base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
    }
}

// ── Block-level layout functions ──────────────────────────────────────────────

/// Heading block: "## Title\n" → faint "## " at half-size + large title text.
fn heading_job(text: &str, base_size: f32, visuals: &egui::Visuals, level: u8, job: &mut LayoutJob) {
    let normal = visuals.text_color();
    let h_size = match level {
        1 => base_size * 2.0,
        2 => base_size * 1.6,
        3 => base_size * 1.3,
        4 => base_size * 1.15,
        _ => base_size,
    };

    for line in text.lines() {
        // Find where the # markers end
        let marker_end = line
            .char_indices()
            .find(|&(_, c)| c != '#' && c != ' ')
            .map(|(i, _)| i)
            .unwrap_or(line.len());

        let markers = &line[..marker_end];
        let content  = &line[marker_end..];

        if !markers.is_empty() {
            job.append(
                markers,
                0.0,
                TextFormat {
                    font_id: FontId::new(h_size * 0.48, FontFamily::Proportional),
                    color: faint(normal),
                    ..Default::default()
                },
            );
        }
        if !content.is_empty() {
            // Use inline_job so **bold** / *italic* inside headings renders correctly.
            // We temporarily scale the base_size to h_size for this block.
            inline_job(content, f32::INFINITY, h_size, visuals, job);
        }
        job.append("\n", 0.0, TextFormat {
            font_id: FontId::new(h_size, FontFamily::Proportional),
            color: normal,
            ..Default::default()
        });
    }
}

/// List block: "- item\n* item2\n" → colored bullets + inline-formatted content.
fn list_job(
    text: &str,
    wrap_width: f32,
    base_size: f32,
    visuals: &egui::Visuals,
    ordered: bool,
    job: &mut LayoutJob,
) {
    let normal = visuals.text_color();
    let accent = Color32::from_rgb(55, 115, 220);
    let mut item_num = 1u32;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            push(job, "\n", base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
            continue;
        }

        // Detect unordered markers: "- ", "* ", "+ "
        let unordered_content = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "));

        // Detect ordered markers: "1. ", "12. ", ...
        let ordered_content = if unordered_content.is_none() {
            trimmed.find(". ").and_then(|dot| {
                if dot > 0 && trimmed[..dot].chars().all(|c| c.is_ascii_digit()) {
                    Some(&trimmed[dot + 2..])
                } else {
                    None
                }
            })
        } else {
            None
        };

        if let Some(content) = unordered_content {
            let bullet = if ordered { format!("{}. ", item_num) } else { "• ".to_string() };
            push(job, &bullet, base_size, accent, false, false, Stroke::NONE, Color32::TRANSPARENT);
            inline_job(content, wrap_width, base_size, visuals, job);
            item_num += 1;
        } else if let Some(content) = ordered_content {
            let bullet = format!("{}. ", item_num);
            push(job, &bullet, base_size, accent, false, false, Stroke::NONE, Color32::TRANSPARENT);
            inline_job(content, wrap_width, base_size, visuals, job);
            item_num += 1;
        } else {
            // Continuation line or nested indent - just inline format it
            let indent_len = line.len() - trimmed.len();
            if indent_len > 0 {
                push(job, &line[..indent_len], base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
            }
            inline_job(trimmed, wrap_width, base_size, visuals, job);
        }

        push(job, "\n", base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
    }
}

/// Blockquote block: "> text\n" → faint "> " marker + colored italic content.
fn blockquote_job(
    text: &str,
    wrap_width: f32,
    base_size: f32,
    visuals: &egui::Visuals,
    job: &mut LayoutJob,
) {
    let normal  = visuals.text_color();
    let dim     = faint(normal);
    let q_color = Color32::from_rgb(70, 100, 200);

    for line in text.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("> ") {
            // Faint marker
            job.append("> ", 0.0, TextFormat {
                font_id: FontId::new(base_size * 0.65, FontFamily::Proportional),
                color: dim,
                ..Default::default()
            });
            // Content: italic, colored
            // We inline-parse but override color - simplest: push directly
            let normal_save = visuals.text_color();
            let _ = normal_save; // visuals is read-only; use italic directly
            inline_job_colored(rest, wrap_width, base_size, visuals, q_color, true, job);
        } else if trimmed == ">" {
            job.append(">", 0.0, TextFormat {
                font_id: FontId::new(base_size * 0.65, FontFamily::Proportional),
                color: dim,
                ..Default::default()
            });
        } else if !trimmed.is_empty() {
            inline_job_colored(trimmed, wrap_width, base_size, visuals, q_color, true, job);
        }

        push(job, "\n", base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
    }
}

/// Variant of inline_job with an overridden base color and italic flag.
fn inline_job_colored(
    text: &str,
    _wrap_width: f32,
    base_size: f32,
    visuals: &egui::Visuals,
    base_color: Color32,
    force_italic: bool,
    job: &mut LayoutJob,
) {
    let dim = faint(base_color);
    let strong = visuals.strong_text_color();
    let code_bg = visuals.code_bg_color;

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_MATH);

    let mut bold   = false;
    let mut italic = force_italic;
    let mut strike = false;
    let mut last_end: usize = 0;

    for (event, range) in Parser::new_ext(text, opts).into_offset_iter() {
        let ts = range.start.min(text.len());
        let te = range.end.min(text.len());

        match event {
            Event::Text(_) => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                if ts < te {
                    let color = if bold { strong } else { base_color };
                    let st = if strike { Stroke::new(1.0, color) } else { Stroke::NONE };
                    let family = match (bold, italic) {
                        (true,  _)     => bold_font(),
                        (false, true)  => italic_font(),
                        (false, false) => FontFamily::Proportional,
                    };
                    job.append(&text[ts..te], 0.0, TextFormat {
                        font_id: FontId::new(base_size, family),
                        color,
                        italics: italic && !bold,
                        strikethrough: st,
                        background: Color32::TRANSPARENT,
                        ..Default::default()
                    });
                }
                last_end = te;
            }
            Event::Code(_) => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                if ts < te {
                    push(job, &text[ts..te], base_size * 0.88, base_color, false, true, Stroke::NONE, code_bg);
                }
                last_end = te;
            }
            Event::SoftBreak | Event::HardBreak => {
                fill_gap(job, text, last_end, ts, base_size, dim);
                push(job, "\n", base_size, base_color, false, false, Stroke::NONE, Color32::TRANSPARENT);
                last_end = te;
            }
            Event::Start(Tag::Strong)        => { bold = true; }
            Event::End(TagEnd::Strong)       => {
                fill_gap(job, text, last_end, te, base_size, dim);
                bold = false; last_end = te;
            }
            Event::Start(Tag::Emphasis)      => { italic = true; }
            Event::End(TagEnd::Emphasis)     => {
                fill_gap(job, text, last_end, te, base_size, dim);
                italic = force_italic; last_end = te;
            }
            Event::Start(Tag::Strikethrough) => { strike = true; }
            Event::End(TagEnd::Strikethrough)=> {
                fill_gap(job, text, last_end, te, base_size, dim);
                strike = false; last_end = te;
            }
            _ => {}
        }
    }

    if last_end < text.len() {
        push(job, &text[last_end..], base_size, base_color, italic, false, Stroke::NONE, Color32::TRANSPARENT);
    }
}

// The old single-galley full-document layouter (build_document_layout_job and
// its equation_block_job / horizontal_rule_job / equation_block_at_offset
// helpers) was superseded by the per-block segmented WYSIWYG editor and removed.

/// Fenced code block: render fences as faint, code body as monospace.
fn fenced_code_job(text: &str, base_size: f32, visuals: &egui::Visuals, job: &mut LayoutJob) {
    let normal = visuals.text_color();
    let dim    = faint(Color32::GRAY);
    let mut in_body = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            // Fence line - faint small mono
            push(job, line, base_size * 0.7, dim, false, true, Stroke::NONE, Color32::TRANSPARENT);
            in_body = !in_body;
        } else {
            // Code body or non-code continuation
            let size = if in_body { base_size * 0.9 } else { base_size };
            push(job, line, size, normal, false, in_body, Stroke::NONE, Color32::TRANSPARENT);
        }
        push(job, "\n", base_size, normal, false, false, Stroke::NONE, Color32::TRANSPARENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> LayoutJob {
        let v = egui::Visuals::light();
        let mut job = LayoutJob::default();
        inline_job(src, f32::INFINITY, 14.0, &v, &mut job);
        job
    }

    /// A <span color> must color the TEXT, not show the raw HTML in full color.
    #[test]
    fn span_color_applies_to_inner_text() {
        let job = run("a<span style=\"color:#ff0000\">red</span>b");
        let red = Color32::from_rgb(255, 0, 0);
        let mut inner_is_red = false;
        for s in &job.sections {
            let seg = &job.text[s.byte_range.clone()];
            if seg == "red" {
                assert_eq!(s.format.color, red, "inner text should be red");
                inner_is_red = true;
            }
            if seg.contains("span") {
                assert_ne!(s.format.color, red, "the raw tag must not be colorized");
            }
        }
        assert!(inner_is_red, "no section held the inner text 'red'");
    }

    /// A <span font-size> must size the inner TEXT; surrounding text keeps base.
    #[test]
    fn span_font_size_applies_to_inner_text() {
        let job = run("a<span style=\"font-size:24pt\">big</span>b");
        let big = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "big").unwrap();
        assert!((big.format.font_id.size - 24.0).abs() < 0.01, "inner text should be 24pt, got {}", big.format.font_id.size);
        let a = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "a").unwrap();
        assert!((a.format.font_id.size - 14.0).abs() < 0.01, "outside text keeps base size");
    }

    #[test]
    fn underline_and_mark_and_sup() {
        let job = run("<u>x</u>");
        let x = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "x").unwrap();
        assert!(x.format.underline.width > 0.0, "x should be underlined");

        let job = run("<mark>y</mark>");
        let y = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "y").unwrap();
        assert_ne!(y.format.background, Color32::TRANSPARENT, "y should be highlighted");

        let job = run("E<sup>2</sup>");
        let sup = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "2").unwrap();
        assert!(sup.format.font_id.size < 14.0, "superscript should be smaller");
        assert_eq!(sup.format.valign, Align::TOP, "superscript should align to top");
    }

    #[test]
    fn span_color_short_hex() {
        let job = run("<span style=\"color:#0f0\">g</span>");
        let g = job.sections.iter().find(|s| &job.text[s.byte_range.clone()] == "g").unwrap();
        assert_eq!(g.format.color, Color32::from_rgb(0, 255, 0));
    }

    #[test]
    fn plain_text_unchanged() {
        let job = run("hello");
        let h = job.sections.iter().find(|s| job.text[s.byte_range.clone()].contains("hello")).unwrap();
        assert_eq!(h.format.color, egui::Visuals::light().text_color());
    }
}
