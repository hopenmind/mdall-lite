//! Markdown syntax highlighting for the Source code editor.
//!
//! Produces an egui `LayoutJob` (monospace, syntax-colored) for the raw
//! Markdown buffer. Unlike the WYSIWYG layouter, which fades the syntax to
//! preview the rendered look, this keeps every marker visible and colors it
//! like a code editor. The tokenizer is intentionally lightweight and
//! forgiving: an unterminated marker just falls back to plain text rather than
//! breaking the rest of the line.

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

struct Palette {
    text: Color32,
    marker: Color32,
    heading: Color32,
    code: Color32,
    link: Color32,
    url: Color32,
    math: Color32,
    fence: Color32,
    quote: Color32,
    list: Color32,
    hr: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            text: Color32::from_rgb(0xD6, 0xCB, 0xB8),
            marker: Color32::from_rgb(0x7E, 0x72, 0x5C),
            heading: Color32::from_rgb(0xE6, 0xB4, 0x5C),
            code: Color32::from_rgb(0xD8, 0x8C, 0x8C),
            link: Color32::from_rgb(0x6F, 0xB0, 0xE0),
            url: Color32::from_rgb(0x7E, 0x9A, 0xB8),
            math: Color32::from_rgb(0xB9, 0x8C, 0xE0),
            fence: Color32::from_rgb(0x82, 0xB8, 0x82),
            quote: Color32::from_rgb(0x9A, 0xB0, 0x9A),
            list: Color32::from_rgb(0xC9, 0x92, 0x0A),
            hr: Color32::from_rgb(0x6A, 0x60, 0x50),
        }
    } else {
        Palette {
            text: Color32::from_rgb(0x2A, 0x1F, 0x0F),
            marker: Color32::from_rgb(0xA8, 0x9A, 0x82),
            heading: Color32::from_rgb(0x8A, 0x52, 0x00),
            code: Color32::from_rgb(0xA0, 0x30, 0x30),
            link: Color32::from_rgb(0x1A, 0x6F, 0xB0),
            url: Color32::from_rgb(0x6B, 0x8C, 0xA8),
            math: Color32::from_rgb(0x7A, 0x4F, 0xB0),
            fence: Color32::from_rgb(0x2F, 0x7A, 0x2F),
            quote: Color32::from_rgb(0x5C, 0x7A, 0x5C),
            list: Color32::from_rgb(0xC9, 0x92, 0x0A),
            hr: Color32::from_rgb(0x9B, 0x88, 0x78),
        }
    }
}

fn fmt(font: &FontId, color: Color32, italics: bool, bold_bg: Option<Color32>) -> TextFormat {
    TextFormat {
        font_id: font.clone(),
        color,
        italics,
        background: bold_bg.unwrap_or(Color32::TRANSPARENT),
        ..Default::default()
    }
}

/// Build a syntax-highlighted, monospace `LayoutJob` for Markdown source.
pub fn highlight_markdown(text: &str, font_size: f32, dark: bool) -> LayoutJob {
    let p = palette(dark);
    let mono = FontId::monospace(font_size);
    let mut job = LayoutJob::default();
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let nl = if line.ends_with('\n') { "\n" } else { "" };
        let lead = body.len() - body.trim_start().len();
        let stripped = &body[lead..];

        // Fenced code block delimiters toggle code mode.
        if stripped.starts_with("```") || stripped.starts_with("~~~") {
            job.append(line, 0.0, fmt(&mono, p.fence, false, None));
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            job.append(line, 0.0, fmt(&mono, p.code, false, None));
            continue;
        }
        // Heading: leading #'s.
        if let Some(level) = heading_level(stripped) {
            job.append(&body[..lead], 0.0, fmt(&mono, p.text, false, None));
            job.append(&stripped[..level], 0.0, fmt(&mono, p.marker, false, None));
            job.append(&stripped[level..], 0.0, fmt(&mono, p.heading, false, None));
            job.append(nl, 0.0, fmt(&mono, p.text, false, None));
            continue;
        }
        // Blockquote.
        if stripped.starts_with('>') {
            job.append(line, 0.0, fmt(&mono, p.quote, false, None));
            continue;
        }
        // Horizontal rule.
        if is_hr(stripped) {
            job.append(line, 0.0, fmt(&mono, p.hr, false, None));
            continue;
        }
        // List marker (-, *, +, or "N.").
        if let Some(mlen) = list_marker_len(stripped) {
            job.append(&body[..lead], 0.0, fmt(&mono, p.text, false, None));
            job.append(&stripped[..mlen], 0.0, fmt(&mono, p.list, false, None));
            append_inline(&mut job, &stripped[mlen..], &p, &mono);
            job.append(nl, 0.0, fmt(&mono, p.text, false, None));
            continue;
        }
        // Table row.
        if stripped.starts_with('|') {
            job.append(line, 0.0, fmt(&mono, p.list, false, None));
            continue;
        }
        // Default paragraph line: inline highlighting.
        if lead > 0 {
            job.append(&body[..lead], 0.0, fmt(&mono, p.text, false, None));
        }
        append_inline(&mut job, stripped, &p, &mono);
        job.append(nl, 0.0, fmt(&mono, p.text, false, None));
    }
    job
}

fn heading_level(s: &str) -> Option<usize> {
    let hashes = s.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && s[hashes..].starts_with(' ') {
        Some(hashes)
    } else {
        None
    }
}

fn is_hr(s: &str) -> bool {
    let t = s.trim();
    (t.len() >= 3) && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// Length (in bytes) of a leading list marker including its trailing space,
/// e.g. "- " -> 2, "12. " -> 4. Returns None if the line is not a list item.
fn list_marker_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if (b.first() == Some(&b'-') || b.first() == Some(&b'*') || b.first() == Some(&b'+'))
        && b.get(1) == Some(&b' ')
    {
        return Some(2);
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && s[digits..].starts_with(". ") {
        return Some(digits + 2);
    }
    None
}

/// Append a line of paragraph text with inline markup colored.
fn append_inline(job: &mut LayoutJob, s: &str, p: &Palette, mono: &FontId) {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut plain = String::new();

    let flush = |job: &mut LayoutJob, plain: &mut String| {
        if !plain.is_empty() {
            job.append(plain, 0.0, fmt(mono, p.text, false, None));
            plain.clear();
        }
    };

    while i < chars.len() {
        let c = chars[i];
        // Inline code: `...`
        if c == '`' {
            if let Some(end) = find_from(&chars, i + 1, '`') {
                flush(job, &mut plain);
                let seg: String = chars[i..=end].iter().collect();
                job.append(&seg, 0.0, fmt(mono, p.code, false, None));
                i = end + 1;
                continue;
            }
        }
        // Math: $...$
        if c == '$' {
            if let Some(end) = find_from(&chars, i + 1, '$') {
                flush(job, &mut plain);
                let seg: String = chars[i..=end].iter().collect();
                job.append(&seg, 0.0, fmt(mono, p.math, false, None));
                i = end + 1;
                continue;
            }
        }
        // Bold: **...**
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find_pair(&chars, i + 2, '*') {
                flush(job, &mut plain);
                let seg: String = chars[i..end + 2].iter().collect();
                job.append(&seg, 0.0, fmt(mono, p.text, false, None));
                i = end + 2;
                continue;
            }
        }
        // Italic: *...* or _..._
        if (c == '*' || c == '_') && chars.get(i + 1) != Some(&c) {
            if let Some(end) = find_from(&chars, i + 1, c) {
                flush(job, &mut plain);
                let seg: String = chars[i..=end].iter().collect();
                job.append(&seg, 0.0, fmt(mono, p.text, true, None));
                i = end + 1;
                continue;
            }
        }
        // Link / image: [text](url) or ![alt](url)
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            let br = if c == '!' { i + 1 } else { i };
            if let Some(close) = find_from(&chars, br + 1, ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find_from(&chars, close + 2, ')') {
                        flush(job, &mut plain);
                        let label: String = chars[i..=close].iter().collect();
                        let url: String = chars[close + 1..=paren].iter().collect();
                        job.append(&label, 0.0, fmt(mono, p.link, false, None));
                        job.append(&url, 0.0, fmt(mono, p.url, false, None));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        plain.push(c);
        i += 1;
    }
    flush(job, &mut plain);
}

fn find_from(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len()).find(|&k| chars[k] == target)
}

/// Find the second of a doubled delimiter (e.g. closing `**`): returns the index
/// of the first of the closing pair.
fn find_pair(chars: &[char], start: usize, target: char) -> Option<usize> {
    let mut k = start;
    while k + 1 < chars.len() {
        if chars[k] == target && chars[k + 1] == target {
            return Some(k);
        }
        k += 1;
    }
    None
}
