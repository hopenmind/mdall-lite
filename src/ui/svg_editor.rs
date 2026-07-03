//! Global SVG editor overlay. Mirrors the app's own Code / Editor / Split
//! paradigm for editing an inline `<svg>` block:
//!   - Code  : monospace SVG/XML editor with syntax highlighting + a Snippets
//!             menu (shapes, path commands, text, gradients, transforms).
//!   - Visual: live resvg render of the current code (the interpreter).
//!   - Split : code on the left, live render on the right.
//! The "Add" action inserts the finished `<svg>...</svg>` at the cursor (or
//! overwrites the block being edited). The visual shape-handle editor is a
//! later phase; this delivers the code surface, the interpreter, and insertion.

use crate::MdApp;
use eframe::egui;
use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, TextFormat};

#[derive(Clone, Copy, PartialEq)]
pub enum SvgTab {
    Code,
    Visual,
    Split,
}

pub struct SvgEditor {
    pub visible: bool,
    pub code: String,
    pub tab: SvgTab,
    /// Source byte offset to insert at on Add (when not replacing).
    pub insert_at: usize,
    /// Source byte range of an existing inline `<svg>` being edited.
    pub replace: Option<std::ops::Range<usize>>,
    /// Last preview URI, so a content change evicts the stale cached texture.
    last_preview_uri: Option<String>,
}

impl Default for SvgEditor {
    fn default() -> Self {
        Self {
            visible: false,
            code: String::new(),
            tab: SvgTab::Split,
            insert_at: 0,
            replace: None,
            last_preview_uri: None,
        }
    }
}

pub const DEFAULT_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 120\" width=\"200\" height=\"120\">\n  <rect x=\"10\" y=\"10\" width=\"180\" height=\"100\" rx=\"8\" fill=\"#eef2ff\" stroke=\"#4456a6\" stroke-width=\"2\"/>\n  <text x=\"100\" y=\"66\" font-size=\"18\" text-anchor=\"middle\" fill=\"#22336a\">SVG</text>\n</svg>";

impl SvgEditor {
    /// Insert a fragment just before the closing `</svg>` (indented), or append
    /// it when there is no closing tag yet.
    fn insert_snippet(&mut self, frag: &str) {
        let indented = format!("  {}\n", frag.replace('\n', "\n  "));
        if let Some(pos) = self.code.rfind("</svg>") {
            self.code.insert_str(pos, &indented);
        } else {
            if !self.code.ends_with('\n') && !self.code.is_empty() {
                self.code.push('\n');
            }
            self.code.push_str(frag);
            self.code.push('\n');
        }
    }

    /// The code wrapped as a complete `<svg>` element, ready to embed.
    fn finalize(&self) -> String {
        let c = self.code.trim();
        if c.starts_with("<svg") {
            c.to_string()
        } else {
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 120\">\n{}\n</svg>",
                c
            )
        }
    }
}

/// (command, meaning, example `d` attribute) for the Path-commands reference.
const PATH_COMMANDS: &[(&str, &str, &str)] = &[
    ("M", "moveto (absolute)", "M 20 60 L 100 60"),
    ("m", "moveto (relative)", "M 20 60 m 0 -20 L 100 40"),
    ("L", "lineto", "M 10 90 L 90 10"),
    ("H", "horizontal lineto", "M 10 50 H 110"),
    ("V", "vertical lineto", "M 60 10 V 110"),
    ("C", "cubic Bezier", "M 10 80 C 40 10 65 10 95 80"),
    ("S", "smooth cubic", "M 10 80 C 40 10 65 10 95 80 S 150 150 180 80"),
    ("Q", "quadratic Bezier", "M 10 80 Q 52 10 95 80"),
    ("T", "smooth quadratic", "M 10 80 Q 52 10 95 80 T 180 80"),
    ("A", "elliptical arc", "M 20 60 A 40 40 0 0 1 100 60"),
    ("Z", "closepath", "M 20 20 L 80 20 L 50 80 Z"),
];

/// Color palette for the SVG/XML highlighter, light or dark.
struct Palette {
    fg: Color32,
    tag: Color32,
    attr: Color32,
    val: Color32,
    comment: Color32,
    punct: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                fg: Color32::from_rgb(0xD8, 0xD2, 0xC4),
                tag: Color32::from_rgb(0x8F, 0xB5, 0xF0),
                attr: Color32::from_rgb(0x9E, 0xD0, 0xA8),
                val: Color32::from_rgb(0xE6, 0xB4, 0x6E),
                comment: Color32::from_rgb(0x7A, 0x77, 0x6E),
                punct: Color32::from_rgb(0xB0, 0xAB, 0x9E),
            }
        } else {
            Self {
                fg: Color32::from_rgb(0x2B, 0x27, 0x1E),
                tag: Color32::from_rgb(0x1D, 0x4E, 0xD8),
                attr: Color32::from_rgb(0x0F, 0x76, 0x3E),
                val: Color32::from_rgb(0xB4, 0x53, 0x09),
                comment: Color32::from_rgb(0x8A, 0x84, 0x76),
                punct: Color32::from_rgb(0x6B, 0x66, 0x5A),
            }
        }
    }
}

/// Syntax-highlight SVG/XML source into a LayoutJob (tags, attribute names,
/// quoted values, comments). Pure ASCII tokenizing; slices only at ASCII bytes
/// so every boundary is valid UTF-8.
pub fn highlight_xml(text: &str, font_size: f32, dark: bool) -> LayoutJob {
    let p = Palette::new(dark);
    let mono = FontId::new(font_size, FontFamily::Monospace);
    let mut job = LayoutJob::default();
    let mut push = |job: &mut LayoutJob, s: &str, c: Color32| {
        if !s.is_empty() {
            job.append(
                s,
                0.0,
                TextFormat { font_id: mono.clone(), color: c, ..Default::default() },
            );
        }
    };
    let mut i = 0usize;
    let n = text.len();
    while i < n {
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
            push(&mut job, &rest[..end], p.comment);
            i += end;
            continue;
        }
        if rest.starts_with('<') {
            let tag_end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            highlight_tag(&mut push, &mut job, &rest[..tag_end], &p);
            i += tag_end;
            continue;
        }
        let next = rest.find('<').unwrap_or(rest.len());
        push(&mut job, &rest[..next], p.fg);
        i += next;
    }
    job
}

fn highlight_tag<F: FnMut(&mut LayoutJob, &str, Color32)>(
    push: &mut F,
    job: &mut LayoutJob,
    tag: &str,
    p: &Palette,
) {
    let b = tag.as_bytes();
    let len = tag.len();
    let mut j = 0usize;
    // Opening '<' and optional '/'.
    if j < len && b[j] == b'<' {
        push(job, "<", p.punct);
        j += 1;
    }
    if j < len && b[j] == b'/' {
        push(job, "/", p.punct);
        j += 1;
    }
    // Element name.
    let ns = j;
    while j < len && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'-' | b'_' | b':')) {
        j += 1;
    }
    push(job, &tag[ns..j], p.tag);
    // Attributes / punctuation until the end of the tag.
    while j < len {
        let c = b[j];
        if c == b'>' || c == b'/' || c == b'=' {
            push(job, &tag[j..j + 1], p.punct);
            j += 1;
        } else if (c as char).is_ascii_whitespace() {
            let ws = j;
            while j < len && (b[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            push(job, &tag[ws..j], p.fg);
        } else if c == b'"' || c == b'\'' {
            let q = c;
            let vs = j;
            j += 1;
            while j < len && b[j] != q {
                j += 1;
            }
            if j < len {
                j += 1;
            }
            push(job, &tag[vs..j], p.val);
        } else {
            let as_ = j;
            while j < len
                && !matches!(b[j], b'=' | b'>' | b'/' | b'"' | b'\'')
                && !(b[j] as char).is_ascii_whitespace()
            {
                j += 1;
            }
            if j == as_ {
                push(job, &tag[j..j + 1], p.fg);
                j += 1;
            } else {
                push(job, &tag[as_..j], p.attr);
            }
        }
    }
}

impl MdApp {
    /// Open the SVG editor to insert a new graphic at the cursor.
    pub(crate) fn open_svg_editor(&mut self) {
        // `cursor_pos` is a char index; the insertion uses a byte offset. Convert,
        // or a multi-byte char before the cursor would misplace or panic the insert.
        self.svg_editor.insert_at = crate::char_to_byte_index(&self.source, self.cursor_pos);
        self.svg_editor.replace = None;
        if self.svg_editor.code.trim().is_empty() {
            self.svg_editor.code = DEFAULT_SVG.to_string();
        }
        self.svg_editor.visible = true;
    }

    pub(crate) fn show_svg_editor(&mut self, ctx: &egui::Context) {
        let dark = self.dark_mode;
        let mut open = self.svg_editor.visible;
        let mut close = false;
        let mut do_add = false;

        egui::Window::new("SVG Editor")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(true)
            .default_width(820.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                // Tab bar + actions.
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.svg_editor.tab, SvgTab::Code, "Code");
                    ui.selectable_value(&mut self.svg_editor.tab, SvgTab::Visual, "Visual");
                    ui.selectable_value(&mut self.svg_editor.tab, SvgTab::Split, "Split");
                    ui.separator();
                    snippet_menu(ui, &mut self.svg_editor);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.svg_editor.replace.is_some() { "Apply" } else { "Add" };
                        if ui.add(egui::Button::new(egui::RichText::new(label).strong())).clicked() {
                            do_add = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
                ui.separator();

                let tab = self.svg_editor.tab;
                let avail = ui.available_size();
                match tab {
                    SvgTab::Code => {
                        self.svg_code_editor(ui, dark, avail.y - 4.0);
                    }
                    SvgTab::Visual => {
                        self.svg_preview(ui, avail);
                    }
                    SvgTab::Split => {
                        let half = (avail.x - 16.0) * 0.5;
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui(egui::vec2(half, avail.y), |ui| {
                                self.svg_code_editor(ui, dark, avail.y - 4.0);
                            });
                            ui.separator();
                            ui.allocate_ui(egui::vec2(half, avail.y), |ui| {
                                self.svg_preview(ui, egui::vec2(half, avail.y - 4.0));
                            });
                        });
                    }
                }
            });

        if do_add {
            let svg = self.svg_editor.finalize();
            let block = format!("\n{}\n", svg);
            if let Some(r) = self.svg_editor.replace.clone() {
                let safe_end = r.end.min(self.source.len());
                let start = r.start.min(safe_end);
                self.source.replace_range(start..safe_end, &svg);
            } else {
                let at = self.svg_editor.insert_at.min(self.source.len());
                self.source.insert_str(at, &block);
            }
            self.modified = true;
            self.segments_dirty = true;
            close = true;
        }
        self.svg_editor.visible = open && !close;
    }

    /// The monospace SVG/XML code surface with live syntax highlighting.
    fn svg_code_editor(&mut self, ui: &mut egui::Ui, dark: bool, height: f32) {
        let mut layouter = |ui: &egui::Ui, text: &str, wrap: f32| {
            let mut job = highlight_xml(text, 13.0, dark);
            job.wrap.max_width = wrap;
            ui.fonts(|f| f.layout_job(job))
        };
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.svg_editor.code)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(18)
                        .layouter(&mut layouter),
                );
            });
    }

    /// Live resvg render of the current code; evicts the stale texture on change.
    fn svg_preview(&mut self, ui: &mut egui::Ui, max: egui::Vec2) {
        let code = self.svg_editor.code.clone();
        // FNV-1a hash so a content change yields a fresh cache URI.
        let mut h: u64 = 0xcbf29ce484222325;
        for byte in code.as_bytes() {
            h ^= *byte as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let uri = format!("bytes://svgpreview-{:016x}.svg", h);
        if self.svg_editor.last_preview_uri.as_deref() != Some(uri.as_str()) {
            if let Some(old) = self.svg_editor.last_preview_uri.take() {
                ui.ctx().forget_image(&old);
            }
            self.svg_editor.last_preview_uri = Some(uri.clone());
        }
        egui::Frame::none()
            .fill(if self.dark_mode {
                Color32::from_rgb(0x1A, 0x17, 0x12)
            } else {
                Color32::from_rgb(0xFA, 0xF7, 0xEF)
            })
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add(
                        egui::Image::from_bytes(uri, code.into_bytes())
                            .max_size(max)
                            .show_loading_spinner(false),
                    );
                });
            });
    }
}

/// The Snippets menu: shapes, text, gradients, transforms, and a path-command
/// reference, each inserting a worked example before `</svg>`.
fn snippet_menu(ui: &mut egui::Ui, editor: &mut SvgEditor) {
    ui.menu_button("Snippets", |ui| {
        snippet_group(ui, editor, "Shapes", &[
            ("Rectangle", "<rect x=\"10\" y=\"10\" width=\"80\" height=\"50\" rx=\"4\" fill=\"#cdd9ff\" stroke=\"#3344aa\" stroke-width=\"2\"/>"),
            ("Circle", "<circle cx=\"50\" cy=\"50\" r=\"30\" fill=\"#fcd34d\" stroke=\"#b45309\" stroke-width=\"2\"/>"),
            ("Ellipse", "<ellipse cx=\"60\" cy=\"40\" rx=\"40\" ry=\"24\" fill=\"#bbf7d0\" stroke=\"#15803d\"/>"),
            ("Line", "<line x1=\"0\" y1=\"0\" x2=\"100\" y2=\"60\" stroke=\"#444444\" stroke-width=\"2\"/>"),
            ("Polyline", "<polyline points=\"0,40 30,10 60,40 90,10\" fill=\"none\" stroke=\"#7c3aed\" stroke-width=\"2\"/>"),
            ("Polygon", "<polygon points=\"50,5 95,40 78,95 22,95 5,40\" fill=\"#93c5fd\" stroke=\"#1e40af\"/>"),
            ("Path", "<path d=\"M 10 80 C 40 10 65 10 95 80 S 150 150 180 80\" fill=\"none\" stroke=\"#db2777\" stroke-width=\"2\"/>"),
        ]);
        snippet_group(ui, editor, "Text", &[
            ("Text", "<text x=\"10\" y=\"30\" font-size=\"16\" fill=\"#222222\">Label</text>"),
            ("Centered", "<text x=\"100\" y=\"60\" font-size=\"18\" text-anchor=\"middle\" fill=\"#222222\">Title</text>"),
            ("Multi-color (tspan)", "<text x=\"10\" y=\"30\" font-size=\"16\"><tspan fill=\"#cc0000\">red</tspan> <tspan fill=\"#0000cc\">blue</tspan></text>"),
        ]);
        snippet_group(ui, editor, "Gradient", &[
            ("Linear", "<defs>\n  <linearGradient id=\"grad1\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">\n    <stop offset=\"0%\" stop-color=\"#60a5fa\"/>\n    <stop offset=\"100%\" stop-color=\"#1e3a8a\"/>\n  </linearGradient>\n</defs>\n<rect x=\"0\" y=\"0\" width=\"120\" height=\"80\" fill=\"url(#grad1)\"/>"),
            ("Radial", "<defs>\n  <radialGradient id=\"grad2\">\n    <stop offset=\"0%\" stop-color=\"#ffffff\"/>\n    <stop offset=\"100%\" stop-color=\"#f59e0b\"/>\n  </radialGradient>\n</defs>\n<circle cx=\"60\" cy=\"60\" r=\"50\" fill=\"url(#grad2)\"/>"),
        ]);
        snippet_group(ui, editor, "Group & transform", &[
            ("Group <g>", "<g fill=\"#dddddd\" stroke=\"#333333\">\n  <rect x=\"0\" y=\"0\" width=\"40\" height=\"40\"/>\n  <rect x=\"50\" y=\"0\" width=\"40\" height=\"40\"/>\n</g>"),
            ("translate", "<g transform=\"translate(20,10)\">\n  <circle cx=\"0\" cy=\"0\" r=\"10\"/>\n</g>"),
            ("rotate", "<g transform=\"rotate(15 50 50)\">\n  <rect x=\"30\" y=\"30\" width=\"40\" height=\"40\" fill=\"#a78bfa\"/>\n</g>"),
            ("scale", "<g transform=\"scale(1.5)\">\n  <circle cx=\"30\" cy=\"30\" r=\"12\"/>\n</g>"),
        ]);
        ui.menu_button("Path commands", |ui| {
            for (cmd, desc, example) in PATH_COMMANDS {
                if ui.button(format!("{cmd}  -  {desc}")).clicked() {
                    editor.insert_snippet(&format!(
                        "<path d=\"{example}\" fill=\"none\" stroke=\"#333333\" stroke-width=\"2\"/>"
                    ));
                    ui.close_menu();
                }
            }
        });
    });
}

fn snippet_group(ui: &mut egui::Ui, editor: &mut SvgEditor, title: &str, items: &[(&str, &str)]) {
    ui.menu_button(title, |ui| {
        for (label, frag) in items {
            if ui.button(*label).clicked() {
                editor.insert_snippet(frag);
                ui.close_menu();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_wraps_bare_fragments() {
        let mut e = SvgEditor::default();
        e.code = "<circle cx=\"5\" cy=\"5\" r=\"4\"/>".into();
        let out = e.finalize();
        assert!(out.starts_with("<svg"));
        assert!(out.contains("<circle"));
        assert!(out.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn finalize_keeps_full_svg() {
        let mut e = SvgEditor::default();
        e.code = DEFAULT_SVG.into();
        assert_eq!(e.finalize(), DEFAULT_SVG);
    }

    #[test]
    fn snippet_inserts_before_closing_tag() {
        let mut e = SvgEditor::default();
        e.code = "<svg>\n</svg>".into();
        e.insert_snippet("<rect/>");
        let rect = e.code.find("<rect").unwrap();
        let close = e.code.find("</svg>").unwrap();
        assert!(rect < close, "snippet must land before </svg>");
    }

    #[test]
    fn highlighter_covers_all_text() {
        // Every byte of the input must appear in exactly one section.
        let src = "<rect x=\"1\" fill=\"#abc\"/> <!-- c -->text";
        let job = highlight_xml(src, 13.0, false);
        let total: usize = job.sections.iter().map(|s| s.byte_range.len()).sum();
        assert_eq!(total, src.len(), "highlighter dropped or duplicated bytes");
    }
}
