//! MD -> ALL lite: the equation + image gallery.
//!
//! Lists every display equation and every image reference in the loaded document
//! as cards. This is the lite editing surface: instead of a full WYSIWYG editor,
//! the user reviews and edits the equations (Edit -> equation editor) and the
//! images (Edit -> image properties), and the conversion pipeline regenerates the
//! output on export.

use eframe::egui;
use mdall_core::editor::BlockKind;

use crate::{theme, MdApp, ViewMode};

/// A markdown image reference `![alt](path)` found in the source, with its byte
/// range over the document (used to rewrite exactly that occurrence).
#[derive(Clone)]
struct ImageRef {
    alt: String,
    path: String,
    range: std::ops::Range<usize>,
}

/// Scan the source for `![alt](path)` image references. Byte ranges are over
/// `src`. A path is bounded by the first `)`, so a title / attribute suffix
/// (`![a](p "t")`) is kept inside the range but not split out (fine for editing).
fn scan_images(src: &str) -> Vec<ImageRef> {
    let b = src.as_bytes();
    let n = src.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < n {
        if b[i] == b'!' && b[i + 1] == b'[' {
            let alt_start = i + 2;
            if let Some(ar) = src[alt_start..].find(']') {
                let alt_end = alt_start + ar;
                if alt_end + 1 < n && b[alt_end + 1] == b'(' {
                    let path_start = alt_end + 2;
                    if let Some(pr) = src[path_start..].find(')') {
                        let path_end = path_start + pr;
                        out.push(ImageRef {
                            alt: src[alt_start..alt_end].to_string(),
                            path: src[path_start..path_end].to_string(),
                            range: i..path_end + 1,
                        });
                        i = path_end + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Resolve a (possibly relative) local image path to a `file://` URI that egui
/// can load. Returns `None` for remote / data URIs or missing files (the app is
/// offline, so remote thumbnails are intentionally not fetched).
fn image_uri(cur_dir: &Option<std::path::PathBuf>, path: &str) -> Option<String> {
    if path.starts_with("http://") || path.starts_with("https://") || path.starts_with("data:") {
        return None;
    }
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(d) = cur_dir {
        d.join(p)
    } else {
        p.to_path_buf()
    };
    if abs.is_file() {
        Some(format!("file://{}", abs.display()))
    } else {
        None
    }
}

impl MdApp {
    pub(crate) fn show_gallery(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let dark = self.dark_mode;
        let font_size = self.font_size;
        // Edit clicks are recorded here and applied AFTER the scroll area, so the
        // render closure never needs a mutable borrow of self.
        let mut open_req: Option<(usize, String)> = None;
        let mut img_edit_req: Option<ImageRef> = None;

        // Top bar: back to the converter hub (lite navigation).
        let mut go_home = false;
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("\u{2190} Converter").color(theme::ACCENT))
                .clicked()
            {
                go_home = true;
            }
            if let Some(p) = &self.current_file {
                if let Some(name) = p.file_name() {
                    ui.label(
                        egui::RichText::new(name.to_string_lossy()).color(theme::text_faint(dark)),
                    );
                }
            }
        });
        if go_home {
            self.view_mode = ViewMode::Converter;
            return;
        }
        ui.separator();

        // Snapshot equations + images up front, so the render borrow never
        // conflicts with a later source mutation (editing).
        let eqs: Vec<(usize, String)> = self
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::DisplayEquation { latex, index } => Some((*index, latex.clone())),
                _ => None,
            })
            .collect();
        let imgs = scan_images(&self.source);
        let cur_dir = self
            .current_file
            .as_ref()
            .and_then(|f| f.parent().map(|d| d.to_path_buf()));

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let card_w = ui.available_width().min(820.0);

                // ── Equations ────────────────────────────────────────────────
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("Equations").color(theme::text_strong(dark)));
                    let n = eqs.len();
                    ui.label(
                        egui::RichText::new(format!(
                            "{n} equation{} in this document",
                            if n == 1 { "" } else { "s" }
                        ))
                        .color(theme::text_faint(dark)),
                    );
                });
                ui.add_space(12.0);

                if eqs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No equations in this document.")
                                .color(theme::text_faint(dark)),
                        );
                    });
                } else {
                    for (idx, latex) in &eqs {
                        ui.vertical_centered(|ui| {
                            egui::Frame::group(ui.style())
                                .fill(theme::surface_soft_c(dark))
                                .rounding(egui::Rounding::same(8.0))
                                .inner_margin(egui::Margin::same(12.0))
                                .show(ui, |ui| {
                                    ui.set_width(card_w);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("Eq. {}", idx + 1))
                                                .strong()
                                                .color(theme::ACCENT),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.button("Edit").clicked() {
                                                    open_req = Some((*idx, latex.clone()));
                                                }
                                            },
                                        );
                                    });

                                    ui.add_space(6.0);
                                    let job = crate::equation_layout::latex_to_layout_job(
                                        latex,
                                        font_size * 1.15,
                                        card_w - 24.0,
                                        theme::text_strong(dark),
                                    );
                                    ui.label(job);

                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(latex.as_str())
                                            .monospace()
                                            .color(theme::text_soft(dark)),
                                    );
                                });
                        });
                        ui.add_space(10.0);
                    }
                }

                // ── Images ───────────────────────────────────────────────────
                ui.add_space(18.0);
                ui.separator();
                ui.add_space(14.0);
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("Images").color(theme::text_strong(dark)));
                    let n = imgs.len();
                    ui.label(
                        egui::RichText::new(format!(
                            "{n} image{} in this document",
                            if n == 1 { "" } else { "s" }
                        ))
                        .color(theme::text_faint(dark)),
                    );
                });
                ui.add_space(12.0);

                if imgs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No images. Add one with ![alt](path) syntax.")
                                .color(theme::text_faint(dark)),
                        );
                    });
                } else {
                    for (n, img) in imgs.iter().enumerate() {
                        ui.vertical_centered(|ui| {
                            egui::Frame::group(ui.style())
                                .fill(theme::surface_soft_c(dark))
                                .rounding(egui::Rounding::same(8.0))
                                .inner_margin(egui::Margin::same(12.0))
                                .show(ui, |ui| {
                                    ui.set_width(card_w);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("Img. {}", n + 1))
                                                .strong()
                                                .color(theme::ACCENT),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.button("Edit").clicked() {
                                                    img_edit_req = Some(img.clone());
                                                }
                                            },
                                        );
                                    });

                                    // Thumbnail (local files only; the app is offline).
                                    ui.add_space(6.0);
                                    match image_uri(&cur_dir, &img.path) {
                                        Some(uri) => {
                                            ui.add(
                                                egui::Image::new(uri.as_str())
                                                    .max_height(150.0)
                                                    .max_width(card_w - 24.0)
                                                    .fit_to_original_size(1.0),
                                            );
                                        }
                                        None => {
                                            ui.label(
                                                egui::RichText::new(
                                                    "(preview unavailable - remote or missing file)",
                                                )
                                                .italics()
                                                .color(theme::text_faint(dark)),
                                            );
                                        }
                                    }

                                    // Alt caption + raw path.
                                    ui.add_space(8.0);
                                    if !img.alt.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&img.alt)
                                                .color(theme::text_soft(dark)),
                                        );
                                    }
                                    ui.label(
                                        egui::RichText::new(img.path.as_str())
                                            .monospace()
                                            .size(font_size * 0.85)
                                            .color(theme::text_faint(dark)),
                                    );
                                });
                        });
                        ui.add_space(10.0);
                    }
                }
            });

        // Deferred: open the equation editor for the clicked equation. Its Apply
        // (apply_equation_edit) rewrites the $$...$$ by index and re-parses.
        if let Some((index, latex)) = open_req {
            self.eq_editor = crate::ui::state::EquationEditor {
                visible: true,
                latex,
                index,
                is_inline: false,
                inline_block_range: 0..0,
                inline_delim_open: String::new(),
                inline_delim_close: String::new(),
                inline_orig_latex: String::new(),
                inline_run_idx: 0,
            };
        }

        // Deferred: open the image properties dialog for the clicked image. Its
        // Apply rewrites the `![...](...)` at `replace` and re-parses the document.
        if let Some(img) = img_edit_req {
            self.image_dialog = crate::ui::state::ImageDialog {
                visible: true,
                alt: img.alt,
                url: img.path,
                width: String::new(),
                align: crate::ui::editor::ImgAlign::None,
                replace: img.range,
            };
        }
    }
}
