//! MD -> ALL lite: the equation gallery.
//!
//! Lists every display equation in the loaded document as a card (number +
//! rendered preview + raw LaTeX). This is the lite editing surface: instead of a
//! full WYSIWYG document editor, the user reviews and (from L2 on) edits the
//! equations directly, and the conversion pipeline regenerates the images on
//! export. Read-only for now; the Edit button is wired in the next step.

use eframe::egui;
use mdall_core::editor::BlockKind;

use crate::{theme, MdApp};

impl MdApp {
    pub(crate) fn show_gallery(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let dark = self.dark_mode;

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
                                            // Wired to the equation editor in L2.
                                            let _ = ui.button("Edit");
                                        },
                                    );
                                });

                                // Rendered preview (same LaTeX -> LayoutJob path as
                                // the editor's equation dialog).
                                ui.add_space(6.0);
                                let job = crate::equation_layout::latex_to_layout_job(
                                    latex,
                                    self.font_size * 1.15,
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
    }
}
