//! Visible↔source index map - keystone of the true-WYSIWYG editor (ADR-002, step 1).
#![allow(dead_code)] // foundation module; wired by later ADR-002 steps
//!
//! Given a block's Markdown source, produce the VISIBLE text (markup removed)
//! plus a span map so a caret placed in the rendered view can be mapped back to
//! a byte offset in the source for editing. This is what lets the editor show
//! only the rendered document while every edit still goes through the Markdown.
//!
//! Pure logic (pulldown-cmark only), unit-tested. The live editor is untouched
//! until later steps build on top of this.

use eframe::egui;
use egui::{text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Inline style flags carried by a visible text run.
///
/// `color`/`bg` are `Color32` (Copy), so the whole struct stays `Copy`. Link
/// targets need owned strings and live on [`VisSpan::link`] instead.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub underline: bool,
    pub sup: bool,
    pub sub: bool,
    /// `<mark>` highlight (background tint). `bg` overrides the default tint.
    pub mark: bool,
    /// Text colour from `<span style="color:#...">`.
    pub color: Option<Color32>,
    /// Background colour from `<mark style="background:#...">`.
    pub bg: Option<Color32>,
}

/// A hyperlink target carried by a visible text run (`[text](url "title")`).
/// Owned strings, so this hangs off [`VisSpan`] (which is `Clone`, not `Copy`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LinkMeta {
    pub url: String,
    pub title: String,
}

/// What a visible span represents.
#[derive(Clone, PartialEq, Debug)]
pub enum VisKind {
    /// Editable styled text. Within the span, visible and source advance 1:1.
    Text(RunStyle),
    /// Atomic inline equation (`$...$`). The caret treats it as one step; clicking
    /// it opens the equation editor. Visible/source are NOT 1:1.
    Equation,
}

/// One contiguous visible span and the source bytes it came from.
#[derive(Clone, Debug)]
pub struct VisSpan {
    /// Byte range inside the VISIBLE string.
    pub vis: std::ops::Range<usize>,
    /// Byte range inside the block SOURCE (includes any hidden markup it covers).
    pub src: std::ops::Range<usize>,
    pub kind: VisKind,
    /// True when offsets map linearly (text). False for atomic/non-1:1 spans
    /// (code spans keep their backticks in `src`; equations are atomic).
    pub linear: bool,
    /// Set when this run is the visible text of a Markdown link.
    pub link: Option<LinkMeta>,
}

/// A block's source mapped to its markup-free visible form.
#[derive(Clone, Debug, Default)]
pub struct MappedBlock {
    pub visible: String,
    pub spans: Vec<VisSpan>,
}

impl MappedBlock {
    /// Map a byte offset in the VISIBLE string to a byte offset in the SOURCE.
    /// Visible spans are contiguous, so `vis_off` lands in exactly one span
    /// (or at the very end → end of the last span's source).
    pub fn source_offset(&self, vis_off: usize) -> usize {
        for s in &self.spans {
            if vis_off < s.vis.end {
                let local = vis_off.saturating_sub(s.vis.start);
                return if s.linear {
                    s.src.start + local
                } else if local == 0 {
                    s.src.start
                } else {
                    s.src.end
                };
            }
        }
        self.spans.last().map(|s| s.src.end).unwrap_or(0)
    }

    /// Map a SOURCE byte offset to a VISIBLE byte offset (best effort).
    pub fn visible_offset(&self, src_off: usize) -> usize {
        for s in &self.spans {
            if src_off >= s.src.start && src_off <= s.src.end {
                return if s.linear {
                    let span_len = s.vis.end - s.vis.start;
                    s.vis.start + (src_off - s.src.start).min(span_len)
                } else if src_off >= s.src.end {
                    s.vis.end
                } else {
                    s.vis.start
                };
            }
        }
        self.visible.len()
    }

    /// Like `source_offset` but for the END of an edited range: maps to the
    /// CONTENT end of the span ending at `vis_off`, so a replace at a styled
    /// run's end does not swallow the following hidden markup (e.g. closing `**`).
    pub fn source_offset_end(&self, vis_off: usize) -> usize {
        for s in &self.spans {
            if vis_off > s.vis.start && vis_off <= s.vis.end {
                let local = vis_off - s.vis.start;
                return if s.linear { s.src.start + local } else { s.src.end };
            }
        }
        self.spans.first().map(|s| s.src.start).unwrap_or(0)
    }

    /// The visible style at a visible offset (for caret-aware toolbar state).
    pub fn style_at(&self, vis_off: usize) -> RunStyle {
        for s in &self.spans {
            if vis_off < s.vis.end {
                if let VisKind::Text(st) = s.kind {
                    return st;
                }
                return RunStyle::default();
            }
        }
        RunStyle::default()
    }

    /// The link target at a visible offset, if the run is a hyperlink (for
    /// hover-tooltip and click-to-open in the editor region).
    pub fn link_at(&self, vis_off: usize) -> Option<&LinkMeta> {
        for s in &self.spans {
            if vis_off < s.vis.end {
                return s.link.as_ref();
            }
        }
        None
    }
}

/// Parse a `#rrggbb` (or `rrggbb`) hex colour. Returns None on malformed input.
fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// Extract a `key:#rrggbb` colour from an inline-style attribute (e.g. `color:`
/// or `background:`) inside an HTML tag string. Tolerant of spaces and quoting.
fn style_hex(tag_lower: &str, key: &str) -> Option<Color32> {
    let i = tag_lower.find(key)?;
    let rest = &tag_lower[i + key.len()..];
    let hex: String = rest.chars().skip_while(|c| *c != '#').take(7).collect();
    parse_hex(&hex)
}

/// Apply one inline-HTML tag to the running style flags. The editor only emits a
/// small, known set (`<u>`, `<sup>`, `<sub>`, `<mark[ style=background]>`,
/// `<span style=color>`); unknown tags are ignored (and stay hidden).
fn apply_inline_html(
    tag: &str,
    underline: &mut bool,
    sup: &mut bool,
    sub: &mut bool,
    mark: &mut bool,
    color: &mut Option<Color32>,
    bg: &mut Option<Color32>,
) {
    let lower = tag.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("</") {
        if rest.starts_with('u') { *underline = false; }
        else if rest.starts_with("sup") { *sup = false; }
        else if rest.starts_with("sub") { *sub = false; }
        else if rest.starts_with("mark") { *mark = false; *bg = None; }
        else if rest.starts_with("span") { *color = None; }
    } else if lower.starts_with("<u>") || lower.starts_with("<u ") {
        *underline = true;
    } else if lower.starts_with("<sup") {
        *sup = true;
    } else if lower.starts_with("<sub") {
        *sub = true;
    } else if lower.starts_with("<mark") {
        *mark = true;
        if let Some(c) = style_hex(&lower, "background:") { *bg = Some(c); }
    } else if lower.starts_with("<span") {
        if let Some(c) = style_hex(&lower, "color:") { *color = Some(c); }
    }
}

/// Build the visible↔source map for a block's Markdown source.
///
/// Handles inline emphasis (bold/italic/strikethrough), inline code, and inline
/// equations; the markup delimiters (`**`, `*`, `~~`, `#`, ...) are removed from
/// the visible text but their bytes stay inside the spanning source range so the
/// caret maps correctly across them.
///
/// Known limitation (step 1): pulldown-cmark decodes HTML entities in text, so a
/// source using literal entities would make visible bytes differ from source
/// bytes inside that run. Our content does not use entities; later steps add a
/// guard.
pub fn map_block(source: &str) -> MappedBlock {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_MATH);

    let mut mb = MappedBlock::default();
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let mut underline = false;
    let mut sup = false;
    let mut sub = false;
    let mut mark = false;
    let mut color: Option<Color32> = None;
    let mut bg: Option<Color32> = None;
    let mut link: Option<LinkMeta> = None;

    for (event, range) in Parser::new_ext(source, opts).into_offset_iter() {
        let ts = range.start.min(source.len());
        let te = range.end.min(source.len());
        match event {
            Event::Text(t) => {
                let vstart = mb.visible.len();
                mb.visible.push_str(&t);
                let vend = mb.visible.len();
                // Linear (1:1 offset mapping) only if the visible bytes equal the
                // source bytes. pulldown decodes HTML entities, so an entity run
                // (visible "&" from "&amp;") is NOT 1:1 → treat it as atomic so
                // offsets clamp to boundaries instead of computing bogus arithmetic.
                let linear = source.get(ts..te).map_or(false, |seg| seg == &*t);
                mb.spans.push(VisSpan {
                    vis: vstart..vend,
                    src: ts..te,
                    kind: VisKind::Text(RunStyle {
                        bold, italic, strike, underline, sup, sub, mark, color, bg,
                        ..Default::default()
                    }),
                    linear,
                    link: link.clone(),
                });
            }
            Event::Code(c) => {
                // Visible = inner code (no backticks); source range includes them.
                let vstart = mb.visible.len();
                mb.visible.push_str(&c);
                let vend = mb.visible.len();
                mb.spans.push(VisSpan {
                    vis: vstart..vend,
                    src: ts..te,
                    kind: VisKind::Text(RunStyle { code: true, ..Default::default() }),
                    linear: false,
                    link: None,
                });
            }
            Event::InlineMath(m) => {
                // Inline math is rendered as its Unicode approximation INSIDE the
                // editable galley so the paragraph stays one wrapping region (the
                // galley text must equal the buffer; a sub/superscript layout would
                // change the characters and break caret mapping). This is the one
                // deliberate use of latex_to_unicode for on-screen rendering, chosen
                // by the user over keeping inline equations as non-wrapping images.
                // Display equations ($$...$$) still use the Typst image path.
                let unicode = mdall_core::render::latex_to_unicode(&m);
                let shown = if unicode.is_empty() { m.to_string() } else { unicode };
                let vstart = mb.visible.len();
                mb.visible.push_str(&shown);
                let vend = mb.visible.len();
                mb.spans.push(VisSpan {
                    vis: vstart..vend,
                    src: ts..te,
                    kind: VisKind::Equation,
                    linear: false,
                    link: None,
                });
            }
            Event::SoftBreak | Event::HardBreak => {
                let vstart = mb.visible.len();
                mb.visible.push('\n');
                mb.spans.push(VisSpan {
                    vis: vstart..mb.visible.len(),
                    src: ts..te,
                    kind: VisKind::Text(RunStyle::default()),
                    linear: false,
                    link: None,
                });
            }
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Strikethrough) => strike = true,
            Event::End(TagEnd::Strikethrough) => strike = false,
            Event::Start(Tag::Link { dest_url, title, .. }) => {
                link = Some(LinkMeta { url: dest_url.to_string(), title: title.to_string() });
            }
            Event::End(TagEnd::Link) => link = None,
            // Inline HTML the editor emits for options Markdown has no syntax for
            // (underline, super/subscript, highlight, text colour). The tag bytes
            // are NOT appended to `visible` (kept hidden); they only flip style.
            Event::InlineHtml(h) | Event::Html(h) => {
                apply_inline_html(&h, &mut underline, &mut sup, &mut sub, &mut mark, &mut color, &mut bg);
            }
            _ => {}
        }
    }
    mb
}

/// Step 2 - render a mapped block to a styled galley of the VISIBLE text only
/// (markup removed). This is the read-only render half of the engine: the
/// document reads clean, with no `**`, `#`, `<span>` ever shown. Equations are a
/// purple placeholder here; step 6 replaces them with in-place widgets.
pub fn render_visible_job(mb: &MappedBlock, base_size: f32, visuals: &egui::Visuals) -> LayoutJob {
    render_buffer_job(&mb.visible, mb, base_size, visuals)
}

/// Floor `i` to a char boundary of `s` (never panics on slicing).
fn floor_char(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Hyperlink text colour (blue, reads as a web link regardless of theme accent).
const LINK_COLOR: Color32 = Color32::from_rgb(38, 109, 211);
/// Default `<mark>` highlight tint when no explicit background colour is given.
const MARK_BG: Color32 = Color32::from_rgb(255, 240, 130);

fn append_run(
    job: &mut LayoutJob,
    text: &str,
    kind: &VisKind,
    base_size: f32,
    visuals: &egui::Visuals,
    link: Option<&LinkMeta>,
) {
    if text.is_empty() {
        return;
    }
    let normal = visuals.text_color();
    let strong = visuals.strong_text_color();
    let code_bg = visuals.code_bg_color;
    match kind {
        VisKind::Text(st) => {
            let is_link = link.is_some();
            // Explicit text colour wins; else link blue; else bold/normal body colour.
            let color = st.color.unwrap_or(if is_link {
                LINK_COLOR
            } else if st.bold {
                strong
            } else {
                normal
            });
            let family = if st.code {
                FontFamily::Monospace
            } else if st.bold {
                FontFamily::Name("CambriaBold".into())
            } else if st.italic {
                FontFamily::Name("CambriaItalic".into())
            } else {
                FontFamily::Proportional
            };
            let mut size = if st.code { base_size * 0.9 } else { base_size };
            if st.sup || st.sub {
                size *= 0.75;
            }
            let background = if st.code {
                code_bg
            } else if let Some(b) = st.bg {
                b
            } else if st.mark {
                MARK_BG
            } else {
                Color32::TRANSPARENT
            };
            let underline = if st.underline || is_link {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            };
            let mut fmt = TextFormat {
                font_id: FontId::new(size, family),
                color,
                italics: st.italic && !st.bold,
                underline,
                strikethrough: if st.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
                background,
                ..Default::default()
            };
            // Approximate super/subscript with vertical alignment within the line.
            if st.sup {
                fmt.valign = egui::Align::TOP;
            } else if st.sub {
                fmt.valign = egui::Align::BOTTOM;
            }
            job.append(text, 0.0, fmt);
        }
        VisKind::Equation => {
            // Inline math: render as normal-weight italic text (math feel), in the
            // body text colour - not a loud purple. Clicking it opens the LaTeX
            // editor (handled by the region).
            job.append(text, 0.0, TextFormat {
                font_id: FontId::new(base_size, FontFamily::Proportional),
                color: visuals.text_color(),
                italics: true,
                ..Default::default()
            });
        }
    }
}

/// Style the LIVE edit `buffer` (which may differ from `mb.visible` by one
/// in-flight keystroke) using `mb`'s spans positionally. Guarantees the galley
/// text equals `buffer` (egui's invariant for caret mapping), styling
/// best-effort - at most the just-typed char is unstyled for a single frame,
/// re-styled next frame after `sync_edit` updates the source. No markup shown.
pub fn render_buffer_job(buffer: &str, mb: &MappedBlock, base_size: f32, visuals: &egui::Visuals) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut covered = 0usize;
    for span in &mb.spans {
        if span.vis.start >= buffer.len() {
            break;
        }
        let start = floor_char(buffer, span.vis.start);
        let end = floor_char(buffer, span.vis.end);
        if start > covered {
            let kind = VisKind::Text(RunStyle::default());
            append_run(&mut job, &buffer[covered..start], &kind, base_size, visuals, None);
        }
        if end > start {
            append_run(&mut job, &buffer[start..end], &span.kind, base_size, visuals, span.link.as_ref());
        }
        covered = end.max(covered);
        if covered >= buffer.len() {
            break;
        }
    }
    if covered < buffer.len() {
        let kind = VisKind::Text(RunStyle::default());
        append_run(&mut job, &buffer[covered..], &kind, base_size, visuals, None);
    }
    // Load-bearing invariant: egui requires the galley text to equal the buffer
    // it laid out, or the caret desyncs / egui can panic. Trip in dev if broken.
    debug_assert_eq!(job.text, buffer, "render_buffer_job: galley text must equal buffer");
    job
}

// ── Step 3/4 - visible-edit → source-edit synchronization ────────────────────

/// Longest common prefix of `a` and `b`, snapped to a char boundary.
fn common_prefix(a: &str, b: &str) -> usize {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let max = ab.len().min(bb.len());
    let mut i = 0;
    while i < max && ab[i] == bb[i] {
        i += 1;
    }
    while i > 0 && (!a.is_char_boundary(i) || !b.is_char_boundary(i)) {
        i -= 1;
    }
    i
}

/// Longest common suffix of `a` and `b` (not crossing `floor`), char-snapped.
fn common_suffix(a: &str, b: &str, floor: usize) -> usize {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let max = a.len().saturating_sub(floor).min(b.len().saturating_sub(floor));
    let mut i = 0;
    while i < max && ab[a.len() - 1 - i] == bb[b.len() - 1 - i] {
        i += 1;
    }
    while i > 0 && (!a.is_char_boundary(a.len() - i) || !b.is_char_boundary(b.len() - i)) {
        i -= 1;
    }
    i
}

/// Apply an edit made to the VISIBLE text back to the block SOURCE.
///
/// The user types into the rendered (markup-free) view; `new_visible` is the
/// resulting visible string. We diff it against `map.visible` to find the single
/// contiguous changed range (covers typing, backspace, paste, selection-replace),
/// map that range to source bytes via the index map, and splice the inserted
/// (plain) text in - leaving surrounding markup intact. Returns the new source.
pub fn sync_edit(source: &str, map: &MappedBlock, new_visible: &str) -> String {
    let old = &map.visible;
    if new_visible == old {
        return source.to_string();
    }
    let p = common_prefix(old, new_visible);
    let s = common_suffix(old, new_visible, p);
    let old_start = p;
    let old_end = old.len() - s;

    // Guard: refuse edits whose changed range overlaps an ATOMIC (non-linear)
    // span - inline code, equations, breaks. A partial interior splice would
    // mangle their markup; they are edited via their own UI, not inline.
    if map.spans.iter().any(|sp| !sp.linear && old_start < sp.vis.end && old_end > sp.vis.start) {
        return source.to_string();
    }

    let inserted = &new_visible[p..new_visible.len() - s];

    // Map to source bytes, then floor to char boundaries so slicing never panics
    // on multi-byte text (accents) and never lands inside a multi-byte char.
    let raw_start = map.source_offset(old_start);
    let raw_end = if old_end == old_start { raw_start } else { map.source_offset_end(old_end).max(raw_start) };
    let src_start = floor_char(source, raw_start.min(source.len()));
    let src_end = floor_char(source, raw_end.max(raw_start).min(source.len()));

    let mut out = String::with_capacity(source.len() + inserted.len());
    out.push_str(&source[..src_start]);
    out.push_str(inserted);
    out.push_str(&source[src_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_identity() {
        let mb = map_block("hello world");
        assert_eq!(mb.visible, "hello world");
        assert_eq!(mb.source_offset(0), 0);
        assert_eq!(mb.source_offset(6), 6);
        assert_eq!(mb.source_offset(11), 11);
    }

    #[test]
    fn bold_markup_is_hidden_and_mapped() {
        // "a**b**c": markup at bytes 1,2 and 4,5 is hidden; 'b' is at source 3.
        let mb = map_block("a**b**c");
        assert_eq!(mb.visible, "abc", "markup must not appear in visible text");
        assert!(!mb.visible.contains('*'));
        assert_eq!(mb.source_offset(0), 0); // 'a'
        assert_eq!(mb.source_offset(1), 3); // start of 'b' (past '**')
        assert_eq!(mb.source_offset(2), 6); // 'c' (past closing '**')
        // 'b' span carries bold
        assert!(mb.style_at(1).bold);
        assert!(!mb.style_at(0).bold);
    }

    #[test]
    fn italic_and_strike_styles() {
        let mb = map_block("*x* ~~y~~");
        assert_eq!(mb.visible, "x y");
        assert!(mb.style_at(0).italic);
        assert!(mb.style_at(2).strike);
    }

    #[test]
    fn heading_markup_hidden() {
        let mb = map_block("## **Title**");
        assert_eq!(mb.visible, "Title");
        assert!(!mb.visible.contains('#'));
        assert!(!mb.visible.contains('*'));
    }

    #[test]
    fn headings_levels_1_to_6_markup_free() {
        // Every heading level must hide its leading hashes in the editor view.
        for n in 1..=6 {
            let hashes = "#".repeat(n);
            let mb = map_block(&format!("{} Title", hashes));
            assert_eq!(mb.visible, "Title", "H{} must hide its hashes", n);
            assert!(!mb.visible.contains('#'));
        }
        // Contract: a heading prefix WITHOUT the trailing space is not a heading -
        // the hashes leak as raw text. This is exactly why the H5/H6 toolbar styles
        // must emit "##### "/"###### " (with the space), like H1-H4.
        let leak = map_block("#####Title");
        assert!(leak.visible.contains('#'), "no-space '#####' is plain text, hashes shown");
    }

    #[test]
    fn inline_code_visible_without_backticks() {
        let mb = map_block("run `cargo build` now");
        assert_eq!(mb.visible, "run cargo build now");
        assert!(!mb.visible.contains('`'));
        // code span is atomic for caret mapping
        let i = mb.visible.find("cargo").unwrap();
        assert!(mb.style_at(i).code);
    }

    #[test]
    fn inline_equation_is_atomic() {
        let mb = map_block("a $x^2$ b");
        assert!(mb.visible.starts_with("a "));
        assert!(mb.visible.ends_with(" b"));
        let eq = mb.spans.iter().find(|s| s.kind == VisKind::Equation).expect("equation span");
        // source range covers the $...$ delimiters
        assert_eq!(&"a $x^2$ b"[eq.src.clone()], "$x^2$");
    }

    #[test]
    fn inline_math_renders_as_unicode_not_dollars() {
        let mb = map_block("mass $E=mc^2$ here");
        assert!(!mb.visible.contains('$'), "delimiters must not be shown");
        assert!(mb.visible.starts_with("mass "));
        assert!(mb.visible.ends_with(" here"));
        let eq = mb.spans.iter().find(|s| s.kind == VisKind::Equation).expect("equation span");
        let shown = &mb.visible[eq.vis.clone()];
        assert!(!shown.is_empty() && !shown.contains('$'), "inline math shown as unicode text");
    }

    #[test]
    fn render_job_is_markup_free_and_styled() {
        let v = egui::Visuals::light();
        let mb = map_block("plain **bold** and *it*");
        let job = render_visible_job(&mb, 14.0, &v);
        // The rendered galley text equals the visible (markup-free) text.
        assert_eq!(job.text, mb.visible);
        assert_eq!(job.text, "plain bold and it");
        assert!(!job.text.contains('*'));
        // The "bold" section uses the bold font family.
        let bold_sec = job.sections.iter()
            .find(|s| &job.text[s.byte_range.clone()] == "bold")
            .expect("bold section");
        assert_eq!(bold_sec.format.font_id.family, FontFamily::Name("CambriaBold".into()));
        // The "it" section is italic.
        let it_sec = job.sections.iter()
            .find(|s| &job.text[s.byte_range.clone()] == "it")
            .expect("italic section");
        assert!(it_sec.format.italics || it_sec.format.font_id.family == FontFamily::Name("CambriaItalic".into()));
    }

    #[test]
    fn buffer_job_text_equals_buffer_even_mid_edit() {
        let v = egui::Visuals::light();
        let mb = map_block("a**b**c"); // visible "abc"
        // at rest: galley == visible
        let j0 = render_buffer_job(&mb.visible, &mb, 14.0, &v);
        assert_eq!(j0.text, "abc");
        // mid-edit: a char was just typed before sync re-derives the map
        let edited = "abXc";
        let j1 = render_buffer_job(edited, &mb, 14.0, &v);
        assert_eq!(j1.text, edited, "galley text MUST equal the live buffer (egui invariant)");
        // shorter buffer (a delete) also safe
        let j2 = render_buffer_job("ab", &mb, 14.0, &v);
        assert_eq!(j2.text, "ab");
    }

    #[test]
    fn sync_plain_insert_and_delete() {
        let mb = map_block("hello");
        assert_eq!(sync_edit("hello", &mb, "heLLo"), "heLLo");
        assert_eq!(sync_edit("hello", &mb, "hllo"), "hllo"); // deleted 'e'
        assert_eq!(sync_edit("hello", &mb, "hellox"), "hellox"); // append
        assert_eq!(sync_edit("hello", &mb, "hello"), "hello"); // no-op
    }

    #[test]
    fn sync_preserves_bold_markup_on_inner_edit() {
        let src = "a**bold**c";
        let mb = map_block(src);
        assert_eq!(mb.visible, "aboldc");
        // Rename the bold word in the rendered view → markup stays around it.
        assert_eq!(sync_edit(src, &mb, "aBOLDc"), "a**BOLD**c");
    }

    #[test]
    fn sync_insert_at_bold_boundary_is_plain() {
        let src = "a**b**c";
        let mb = map_block(src); // visible "abc"
        assert_eq!(sync_edit(src, &mb, "abXc"), "a**b**Xc");
    }

    #[test]
    fn sync_roundtrip_through_map_is_stable() {
        // One contiguous edit per call (matches real per-frame keystroke editing):
        // "Hello" -> "Hi", keeping the bold "world" intact.
        let src = "Hello **world**";
        let mb = map_block(src);
        let new_src = sync_edit(src, &mb, "Hi world");
        assert_eq!(new_src, "Hi **world**");
        assert_eq!(map_block(&new_src).visible, "Hi world");
    }

    #[test]
    fn sync_accented_text_no_panic_and_correct() {
        let src = "café **gras** déjà";
        let mb = map_block(src);
        assert_eq!(mb.visible, "café gras déjà");
        // Edit the bold word (accents around it).
        assert_eq!(sync_edit(src, &mb, "café GRAS déjà"), "café **GRAS** déjà");
        // Edit accented plain text ('é' -> 'e') - visible has NO markup; must not
        // panic on the multi-byte boundary, must map right.
        assert_eq!(sync_edit(src, &mb, "cafe gras déjà"), "cafe **gras** déjà");
    }

    #[test]
    fn sync_inside_inline_code_is_noop() {
        let src = "run `cargo build` now";
        let mb = map_block(src);
        assert_eq!(mb.visible, "run cargo build now");
        // Editing INSIDE the atomic code span must not corrupt the source.
        assert_eq!(sync_edit(src, &mb, "run cargo built now"), src);
    }

    #[test]
    fn sync_full_delete_multibyte_no_panic() {
        let src = "**é**";
        let mb = map_block(src);
        assert_eq!(mb.visible, "é");
        let out = sync_edit(src, &mb, ""); // must not panic on multi-byte delete
        assert!(map_block(&out).visible.is_empty());
    }

    #[test]
    fn sync_empty_block_append() {
        let mb = map_block("");
        assert_eq!(sync_edit("", &mb, "x"), "x");
    }

    #[test]
    fn round_trip_offsets() {
        let mb = map_block("a**bb**c");
        // visible "abbc"; mapping back and forth lands consistently
        for v in 0..=mb.visible.len() {
            let s = mb.source_offset(v);
            let v2 = mb.visible_offset(s);
            assert!(v2 <= mb.visible.len());
        }
        assert_eq!(mb.visible, "abbc");
        assert_eq!(mb.source_offset(1), 3); // first 'b' past '**'
    }
}

/// Functional pipeline: one case per editor inline option. Each asserts the
/// shared invariant - the rendered (visible) text hides the markup AND carries
/// the option's style/metadata, so the editor renders it while the markup lives
/// only in the source. The exact source strings mirror what `src/ui/toolbar.rs`
/// emits for every button. Run with `--nocapture` to see the per-option table.
#[cfg(test)]
mod option_pipeline {
    use super::*;

    struct Case {
        name: &'static str,
        /// Inline Markdown/HTML the editor option writes into the source.
        src: &'static str,
        /// Expected markup-free visible text (None ⇒ equation: just assert no `$`).
        visible: Option<&'static str>,
        /// Style/metadata predicate at the content offset - the "rendered" half.
        check: fn(&MappedBlock) -> bool,
    }

    /// Offset of the styled content inside the visible string.
    fn content_off(mb: &MappedBlock) -> usize {
        // Content is "X" in every case; fall back to 0 for equations.
        mb.visible.find('X').unwrap_or(0)
    }

    fn cases() -> Vec<Case> {
        vec![
            Case { name: "bold (**X**)",            src: "**X**",                                  visible: Some("X"), check: |m| m.style_at(content_off(m)).bold },
            Case { name: "italic (*X*)",            src: "*X*",                                    visible: Some("X"), check: |m| m.style_at(content_off(m)).italic },
            Case { name: "strikethrough (~~X~~)",   src: "~~X~~",                                  visible: Some("X"), check: |m| m.style_at(content_off(m)).strike },
            Case { name: "inline code (`X`)",       src: "`X`",                                    visible: Some("X"), check: |m| m.style_at(content_off(m)).code },
            Case { name: "underline (<u>)",         src: "<u>X</u>",                               visible: Some("X"), check: |m| m.style_at(content_off(m)).underline },
            Case { name: "superscript (<sup>)",     src: "<sup>X</sup>",                           visible: Some("X"), check: |m| m.style_at(content_off(m)).sup },
            Case { name: "subscript (<sub>)",       src: "<sub>X</sub>",                           visible: Some("X"), check: |m| m.style_at(content_off(m)).sub },
            Case { name: "highlight (<mark>)",      src: "<mark>X</mark>",                         visible: Some("X"), check: |m| m.style_at(content_off(m)).mark },
            Case { name: "highlight color",         src: "<mark style=\"background:#ffff00\">X</mark>", visible: Some("X"), check: |m| m.style_at(content_off(m)).bg.is_some() },
            Case { name: "text color (<span>)",     src: "<span style=\"color:#ff0000\">X</span>", visible: Some("X"), check: |m| m.style_at(content_off(m)).color.is_some() },
            Case { name: "link [X](url)",           src: "[X](https://e.com)",                     visible: Some("X"), check: |m| m.link_at(content_off(m)).is_some() },
            Case { name: "link + title (tooltip)",  src: "[X](https://e.com \"tip\")",             visible: Some("X"), check: |m| m.link_at(content_off(m)).map_or(false, |l| l.title == "tip") },
            Case { name: "inline equation ($X$)",   src: "$X$",                                    visible: None,      check: |m| m.spans.iter().any(|s| s.kind == VisKind::Equation) },
        ]
    }

    /// Markup characters that must NEVER leak into the rendered view.
    fn has_raw_markup(visible: &str) -> bool {
        visible.contains('*') || visible.contains('~') || visible.contains('`')
            || visible.contains('<') || visible.contains('>') || visible.contains('[')
            || visible.contains(']') || visible.contains('$')
    }

    #[test]
    fn editor_option_render_report() {
        let mut fails: Vec<String> = Vec::new();
        println!("\n  EDITOR OPTION RENDER PIPELINE  (markup hidden + styled in editor)");
        println!("  {:<28} {:<10} {:<10} {}", "option", "no-raw", "styled", "visible");
        println!("  {}", "-".repeat(70));
        for c in cases() {
            let mb = map_block(c.src);
            let no_raw = !has_raw_markup(&mb.visible)
                && c.visible.map_or(true, |v| mb.visible == v);
            let styled = (c.check)(&mb);
            let ok = no_raw && styled;
            println!(
                "  {:<28} {:<10} {:<10} {:?}",
                c.name,
                if no_raw { "PASS" } else { "FAIL" },
                if styled { "PASS" } else { "FAIL" },
                mb.visible,
            );
            if !ok {
                fails.push(format!("{} (no-raw={}, styled={})", c.name, no_raw, styled));
            }
        }
        println!("  {}", "-".repeat(70));
        println!("  {} / {} options fully render markup-free + styled\n",
                 cases().len() - fails.len(), cases().len());
        assert!(
            fails.is_empty(),
            "{} editor option(s) do not yet render styled with hidden markup:\n  - {}",
            fails.len(),
            fails.join("\n  - "),
        );
    }
}
