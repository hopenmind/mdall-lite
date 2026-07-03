//! MD -> ALL lite: the equation gallery.
//!
//! Lists every display equation in the loaded document as a card (number +
//! rendered preview + raw LaTeX). This is the lite editing surface: instead of a
//! full WYSIWYG document editor, the user reviews and (from L2 on) edits the
//! equations directly, and the conversion pipeline regenerates the images on
//! export. Read-only for now; the Edit button is wired in the next step.

use eframe::egui;
use mdall_core::editor::BlockKind;

use crate::{theme, MdApp, ViewMode};

impl MdApp {
    pub(crate) fn show_gallery(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let dark = self.dark_mode;
        let font_size = self.font_size;
        // An Edit click is recorded here and applied AFTER the scroll area, so the
        // render closure never needs a mutable borrow of self.
        let mut open_req: Option<(usize, String)> = None;

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

        // Snapshot the display equations up front (index + LaTeX), so the render
        // borrow never conflicts with a later source mutation (equation editing).
        let eqs: Vec<(usize, String)> = self
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::DisplayEquation { latex, index } => Some((*index, latex.clone())),
                _ => None,
            })
            .collect();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
                        ui.add_space(40.0);
                        ui.label(
                            egui::RichText::new(
                                "No equations yet. Open a document to see its equations here.",
                            )
                            .color(theme::text_faint(dark)),
                        );
                    });
                    return;
                }

                let card_w = ui.available_width().min(820.0);
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

                                // Rendered preview (same LaTeX -> LayoutJob path as
                                // the editor's equation dialog).
                                ui.add_space(6.0);
                                let job = crate::equation_layout::latex_to_layout_job(
                                    latex,
                                    font_size * 1.15,
                                    card_w - 24.0,
                                    theme::text_strong(dark),
                                );
                                ui.label(job);

                                // Raw LaTeX source.
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
            });

        // Open the equation editor overlay for the clicked equation. Its Apply
        // (apply_equation_edit) locates the block by `index` and rewrites its
        // $$...$$ in the source, then a re-parse refreshes this gallery.
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
    }
}
