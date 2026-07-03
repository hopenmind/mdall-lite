//! Modal dialogs: equation editor, link/image insert, export picker.
//! Methods on MdApp, extracted from main.rs.

use eframe::egui;
use crate::MdApp;
use crate::equation_layout;
use crate::theme;
use crate::ui::icons::{self, Icon, IconSet};
use crate::ui::state::{EditorMode, ToastKind};
use mdall_core::{editor, export, render, inline_math, export_formats};

impl MdApp {
    pub(crate) fn show_equation_editor(&mut self, ctx: &egui::Context) {
        let mut open = self.eq_editor.visible;
        let win_resp = egui::Window::new("Equation Editor")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(true)
            .default_width(600.0)
            .max_height(400.0)
            .show(ctx, |ui| {
                let eq_label = if self.eq_editor.is_inline {
                    format!("Inline equation  ({}{}...{})",
                        self.eq_editor.inline_delim_open,
                        self.eq_editor.latex.chars().take(12).collect::<String>(),
                        self.eq_editor.inline_delim_close)
                } else {
                    format!("Equation #{}", self.eq_editor.index + 1)
                };
                ui.label(egui::RichText::new(eq_label).strong());
                ui.add_space(8.0);

                ui.label("LaTeX source:");
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.eq_editor.latex)
                            .font(egui::FontId::monospace(self.font_size))
                            .desired_width(f32::INFINITY)
                            .desired_rows(4),
                    );
                });

                ui.add_space(8.0);
                ui.label("Preview:");
                let frame = egui::Frame::default()
                    .fill(egui::Color32::from_rgb(248, 248, 255))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(205, 185, 145)))
                    .rounding(6.0)
                    .inner_margin(egui::Margin::symmetric(16.0, 20.0));
                frame.show(ui, |ui| {
                    ui.set_min_height(80.0);
                    let prepared_preview = self.prepare_latex(&self.eq_editor.latex);
                    ui.centered_and_justified(|ui| {
                        let preview_job = equation_layout::latex_to_layout_job(
                            &prepared_preview,
                            self.font_size + 8.0,
                            ui.available_width(),
                            ui.visuals().text_color(),
                        );
                        ui.label(preview_job);
                    });
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("  Apply  ").clicked() {
                        if self.eq_editor.is_inline {
                            self.apply_inline_equation_edit();
                        } else {
                            self.apply_equation_edit();
                        }
                        self.eq_editor.visible = false;
                    }
                    if ui.button("  Cancel  ").clicked() {
                        self.eq_editor.visible = false;
                    }
                });
            });

        if let Some(resp) = win_resp {
            let win_rect = resp.response.rect;
            ctx.input(|i| {
                if i.pointer.any_pressed() {
                    if let Some(pos) = i.pointer.interact_pos() {
                        if !win_rect.contains(pos) {
                            self.eq_editor.visible = false;
                        }
                    }
                }
            });
        }

        if !open { self.eq_editor.visible = false; }
    }

    fn apply_equation_edit(&mut self) {
        let target_idx = self.eq_editor.index;
        let new_latex  = self.eq_editor.latex.clone();

        // Prefer source_range if blocks are still in sync (most reliable)
        let range_opt = self.blocks.iter().find_map(|b| {
            if let editor::BlockKind::DisplayEquation { index, .. } = &b.kind {
                if *index == target_idx { Some(b.source_range.clone()) } else { None }
            } else {
                None
            }
        });

        if let Some(range) = range_opt {
            // Replace the whole $$...$$  block with the new latex
            let new_block = format!("$$\n{}\n$$\n", new_latex);
            let safe_end = range.end.min(self.source.len());
            self.source.replace_range(range.start..safe_end, &new_block);
            self.modified = true;
            self.segments_dirty = true;
            self.status_msg = "Equation updated".into();
        } else {
            // Fallback: find the raw latex string in source and replace it
            let equations = render::extract_equations(&self.source);
            if target_idx < equations.len() {
                let old = &equations[target_idx];
                if let Some(pos) = self.source.find(old.as_str()) {
                    let end = pos + old.len();
                    self.source.replace_range(pos..end, &new_latex);
                    self.modified = true;
                    self.segments_dirty = true;
                    self.status_msg = "Equation updated".into();
                }
            }
        }
    }

    /// Replace an inline $...$ or \(...\) equation in source.
    ///
    /// Uses `inline_orig_latex` to locate the exact run that was clicked,
    /// then replaces only that run's latex content, preserving delimiters and
    /// all surrounding text/equations in the same paragraph.
    fn apply_inline_equation_edit(&mut self) {
        let range      = self.eq_editor.inline_block_range.clone();
        let new_latex  = self.eq_editor.latex.trim().to_string();
        let orig_latex = self.eq_editor.inline_orig_latex.clone();
        let target_idx = self.eq_editor.inline_run_idx;
        let safe_end   = range.end.min(self.source.len());
        let block_text = self.source[range.start..safe_end].to_string();

        // Re-parse the block into runs, replace exactly the run at target_idx.
        // Using run_idx is precise: works correctly even when the same latex
        // appears multiple times in the same paragraph.
        let mut replaced = false;
        let new_runs: Vec<inline_math::InlineRun> = inline_math::split_inline(&block_text)
            .into_iter()
            .enumerate()
            .map(|(idx, r)| {
                if idx == target_idx {
                    if let inline_math::InlineRun::Equation { ref delim_open, ref delim_close, .. } = r {
                        replaced = true;
                        return inline_math::InlineRun::Equation {
                            latex:       new_latex.clone(),
                            delim_open:  delim_open.clone(),
                            delim_close: delim_close.clone(),
                        };
                    }
                }
                r
            })
            .collect();

        if replaced {
            let new_block = inline_math::serialize_runs(&new_runs);
            self.source.replace_range(range.start..safe_end, &new_block);
            self.modified       = true;
            self.segments_dirty = true;
            self.status_msg     = "Inline equation updated".into();
            // Remove old texture so it's re-rendered with new latex on next frame
            self.eq_tex_cache.remove(&orig_latex);
        } else {
            self.status_msg = "Inline equation not found in source".into();
        }
    }

    pub(crate) fn show_link_dialog(&mut self, ctx: &egui::Context) {
        let title = if self.link_dialog.is_image { "Insert Image" } else { "Insert Link" };
        let mut open = self.link_dialog.visible;
        egui::Window::new(title)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(false)
            .default_width(450.0)
            .show(ctx, |ui| {
                egui::Grid::new("link_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(if self.link_dialog.is_image { "Alt text:" } else { "Link text:" });
                        ui.text_edit_singleline(&mut self.link_dialog.text);
                        ui.end_row();
                        ui.label("URL / path:");
                        ui.text_edit_singleline(&mut self.link_dialog.url);
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if self.link_dialog.is_image {
                        if ui.button("Browse...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Images", &["png", "svg", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"])
                                .pick_file()
                            {
                                self.link_dialog.url = path.display().to_string();
                            }
                        }
                    }
                    if ui.button("Insert").clicked() {
                        let md = if self.link_dialog.is_image {
                            format!("![{}]({})", self.link_dialog.text, self.link_dialog.url)
                        } else {
                            format!("[{}]({})", self.link_dialog.text, self.link_dialog.url)
                        };
                        self.insert_text(&md);
                        self.link_dialog.visible = false;
                    }
                    if ui.button("Cancel").clicked() { self.link_dialog.visible = false; }
                });
            });
        self.link_dialog.visible = open;
    }

    /// Image properties popup: edit alt / url / width / alignment of an existing
    /// standalone image, then overwrite its source block (HTML when it carries
    /// geometry, plain Markdown otherwise).
    pub(crate) fn show_image_dialog(&mut self, ctx: &egui::Context) {
        use crate::ui::editor::{serialize_image, ImgAlign};
        let mut open = self.image_dialog.visible;
        let mut apply = false;
        egui::Window::new("Image Properties")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                egui::Grid::new("image_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Alt / caption:");
                        ui.text_edit_singleline(&mut self.image_dialog.alt);
                        ui.end_row();
                        ui.label("URL / path:");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.image_dialog.url);
                            if ui.button("Browse...").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Images",
                                        &["png", "svg", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"])
                                    .pick_file()
                                {
                                    self.image_dialog.url = path.display().to_string();
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Width (px):");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.image_dialog.width)
                                .desired_width(80.0).hint_text("auto"));
                            ui.label(egui::RichText::new("empty = original size")
                                .small().color(theme::TEXT_MUTED));
                        });
                        ui.end_row();
                        ui.label("Alignment:");
                        ui.horizontal(|ui| {
                            for (label, a) in [
                                ("Default", ImgAlign::None),
                                ("Left",    ImgAlign::Left),
                                ("Center",  ImgAlign::Center),
                                ("Right",   ImgAlign::Right),
                            ] {
                                ui.selectable_value(&mut self.image_dialog.align, a, label);
                            }
                        });
                        ui.end_row();
                    });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() { apply = true; }
                    if ui.button("Cancel").clicked() { self.image_dialog.visible = false; }
                });
            });

        if apply {
            let width = self.image_dialog.width
                .trim().trim_end_matches("px").trim().parse::<u32>().ok();
            let md = serialize_image(
                &self.image_dialog.alt, &self.image_dialog.url, width, self.image_dialog.align,
            );
            let r = self.image_dialog.replace.clone();
            let safe_end = r.end.min(self.source.len());
            let start = r.start.min(safe_end);
            self.source.replace_range(start..safe_end, &md);
            self.modified = true;
            self.segments_dirty = true;
            self.image_dialog.visible = false;
            open = false;
        }
        self.image_dialog.visible = open;
    }

    /// Visual table editor: a grid of editable cells with per-column alignment
    /// and add/remove row/column controls, serialized to a GFM pipe table on
    /// Insert (or Apply when editing a table already in the source).
    pub(crate) fn show_table_dialog(&mut self, ctx: &egui::Context) {
        use crate::ui::state::ColAlign;
        let mut open = self.table_dialog.visible;
        let mut close = false;
        let mut do_insert = false;
        let (mut add_row, mut del_row, mut add_col, mut del_col) = (false, false, false, false);
        let editing = self.table_dialog.replace.is_some();
        let title = if editing { "Edit Table" } else { "Insert Table" };

        egui::Window::new(title)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(true)
            .default_width(540.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} x {}", self.table_dialog.rows, self.table_dialog.cols))
                        .small().color(theme::TEXT_MUTED));
                    ui.separator();
                    if ui.button("+ Row").clicked() { add_row = true; }
                    if ui.add_enabled(self.table_dialog.rows > 2, egui::Button::new("- Row")).clicked() { del_row = true; }
                    ui.separator();
                    if ui.button("+ Column").clicked() { add_col = true; }
                    if ui.add_enabled(self.table_dialog.cols > 1, egui::Button::new("- Column")).clicked() { del_col = true; }
                });
                ui.label(egui::RichText::new("Row 1 is the header; the L / C / R row sets column alignment.")
                    .small().color(theme::TEXT_MUTED));
                ui.separator();

                egui::ScrollArea::both().max_height(380.0).show(ui, |ui| {
                    let cols = self.table_dialog.cols;
                    let rows = self.table_dialog.rows;
                    egui::Grid::new("table_editor_grid").striped(true).spacing([6.0, 6.0]).show(ui, |ui| {
                        // Per-column alignment selector.
                        for c in 0..cols {
                            let mut sel = self.table_dialog.aligns.get(c).copied().unwrap_or(ColAlign::Left);
                            ui.horizontal(|ui| {
                                if ui.selectable_label(sel == ColAlign::Left, "L").clicked() { sel = ColAlign::Left; }
                                if ui.selectable_label(sel == ColAlign::Center, "C").clicked() { sel = ColAlign::Center; }
                                if ui.selectable_label(sel == ColAlign::Right, "R").clicked() { sel = ColAlign::Right; }
                            });
                            if let Some(a) = self.table_dialog.aligns.get_mut(c) { *a = sel; }
                        }
                        ui.end_row();
                        // Editable cells (row 0 is the header).
                        for r in 0..rows {
                            for c in 0..cols {
                                let hint = if r == 0 { "Header" } else { "" };
                                ui.add(egui::TextEdit::singleline(&mut self.table_dialog.cells[r][c])
                                    .desired_width(120.0)
                                    .hint_text(hint));
                            }
                            ui.end_row();
                        }
                    });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if editing { "Apply" } else { "Insert" };
                    if ui.add_sized([96.0, 30.0], egui::Button::new(label)).clicked() { do_insert = true; }
                    if ui.button("Cancel").clicked() { close = true; }
                });
            });

        // Apply structural edits after rendering (avoids mutating the grid mid-frame).
        if add_row { self.table_dialog.add_row(); }
        if del_row { self.table_dialog.del_row(); }
        if add_col { self.table_dialog.add_col(); }
        if del_col { self.table_dialog.del_col(); }

        if do_insert {
            let md = self.table_dialog.to_markdown();
            if let Some(r) = self.table_dialog.replace.clone() {
                let safe_end = r.end.min(self.source.len());
                let start = r.start.min(safe_end);
                self.source.replace_range(start..safe_end, &md);
                self.modified = true;
                self.segments_dirty = true;
            } else {
                self.insert_text(&md);
            }
            close = true;
        }
        self.table_dialog.visible = open && !close;
    }

    pub(crate) fn show_export_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.export_dialog.visible;
        let meta = self.meta.clone();
        let src  = self.source.clone();
        // Resolve author figures relative to the open document's folder.
        let source_dir = self.current_file.as_ref().and_then(|f| f.parent().map(|p| p.to_path_buf()));

        egui::Window::new("Export Document")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                // ── Common formats ────────────────────────────────────────────
                ui.label(egui::RichText::new("Common formats").strong());
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                    let btn = |ui: &mut egui::Ui, label: &str| {
                        ui.add_sized([90.0, 40.0], egui::Button::new(label))
                    };

                    if btn(ui, "PDF").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_pdf();
                    }
                    if btn(ui, "HTML").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_html();
                    }
                    if btn(ui, "TXT").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("Text", &["txt"], |path| {
                            export_formats::export_txt(&src, path)
                        });
                    }
                    if btn(ui, "TeX").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("LaTeX", &["tex"], |path| {
                            export_formats::export_tex(&src, path, &meta)
                        });
                    }
                    if btn(ui, "DOCX").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("Word Document", &["docx"], |path| {
                            export_formats::export_docx(&src, path, &meta, source_dir.as_deref())
                        });
                    }
                    if btn(ui, "ODT").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("OpenDocument Text", &["odt"], |path| {
                            export_formats::export_odt(&src, path, &meta, source_dir.as_deref())
                        });
                    }
                    if btn(ui, "EPUB").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("EPUB eBook", &["epub"], |path| {
                            export_formats::export_epub(&src, path, &meta, source_dir.as_deref())
                        });
                    }
                });

                ui.add_space(12.0);

                // ── Other formats ─────────────────────────────────────────────
                ui.label(egui::RichText::new("Other formats").strong());
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                    let sml = |ui: &mut egui::Ui, label: &str| {
                        ui.add_sized([80.0, 28.0], egui::Button::new(label))
                    };

                    if sml(ui, "RTF").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("Rich Text", &["rtf"], |path| {
                            export_formats::export_rtf(&src, path, &meta, source_dir.as_deref())
                        });
                    }
                    if sml(ui, "Typst (.typ)").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("Typst Source", &["typ"], |path| {
                            export_formats::export_typst_src(&src, path, &meta)
                        });
                    }
                    if sml(ui, "Markdown").clicked() {
                        self.export_dialog.visible = false;
                        self.do_export_fmt("Markdown", &["md"], |path| {
                            std::fs::write(path, src.as_bytes()).map_err(|e| e.to_string())
                        });
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Equations exported as PNG images in DOCX / ODT / EPUB")
                        .small().color(egui::Color32::GRAY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() { self.export_dialog.visible = false; }
                    });
                });
            });

        if !open { self.export_dialog.visible = false; }
    }
}

impl MdApp {
    /// Generic export helper: shows a save-file dialog then runs the export closure.
    fn do_export_fmt<F>(&mut self, type_name: &str, exts: &[&str], export_fn: F)
    where F: FnOnce(&std::path::Path) -> Result<(), String>
    {
        // Ensure custom LaTeX macros are active for equation rendering in this export.
        mdall_core::latex_macros::install_from_source(&self.source);
        let mut dlg = rfd::FileDialog::new().add_filter(type_name, exts);
        if let Some(ref f) = self.current_file {
            if let Some(dir) = f.parent() { dlg = dlg.set_directory(dir); }
            if let Some(stem) = f.file_stem() {
                dlg = dlg.set_file_name(&format!("{}.{}", stem.to_string_lossy(), exts[0]));
            }
        }
        if let Some(path) = dlg.save_file() {
            match export_fn(&path) {
                Ok(()) => {
                    self.status_msg = format!("{} exported", type_name);
                    let _ = open::that(&path);
                }
                Err(e) => self.status_msg = format!("Export error: {}", e),
            }
        }
    }

    pub(crate) fn show_metadata_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Document Metadata")
            .open(&mut self.show_metadata)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(false)
            .default_width(500.0)
            .show(ctx, |ui| {
                egui::Grid::new("meta_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        let m = &mut self.meta;
                        ui.label("Title:"); ui.text_edit_singleline(&mut m.title); ui.end_row();
                        ui.label("Author:"); ui.text_edit_singleline(&mut m.author); ui.end_row();
                        ui.label("Subject:"); ui.text_edit_singleline(&mut m.subject); ui.end_row();
                        ui.label("Keywords:"); ui.text_edit_singleline(&mut m.keywords); ui.end_row();
                        ui.label("DOI:"); ui.text_edit_singleline(&mut m.doi); ui.end_row();
                        ui.label("Version:"); ui.text_edit_singleline(&mut m.version); ui.end_row();
                        ui.label("Language:"); ui.text_edit_singleline(&mut m.lang); ui.end_row();
                        ui.label("Timestamp:");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut m.timestamp);
                            if ui.button("Now").clicked() {
                                m.timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
                            }
                        });
                        ui.end_row();
                        ui.label("Signature:"); ui.text_edit_singleline(&mut m.signature); ui.end_row();
                        ui.label("License:"); ui.text_edit_singleline(&mut m.license); ui.end_row();
                    });
                ui.add_space(12.0);
                if ui.button("Clear All").clicked() { self.meta = export::PdfMetadata::default(); }
            });
    }
}

impl MdApp {
    /// Go-to-line popup for the Source code editor (Ctrl+G).
    pub(crate) fn show_goto_line_dialog(&mut self, ctx: &egui::Context) {
        if !self.goto_line_open {
            return;
        }
        let mut open = true;
        let mut go = false;
        egui::Window::new("Go to line")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Line:");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.goto_line_input).desired_width(80.0),
                    );
                    resp.request_focus();
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        go = true;
                    }
                    if ui.button("Go").clicked() {
                        go = true;
                    }
                });
            });
        if go {
            if let Ok(n) = self.goto_line_input.trim().parse::<usize>() {
                self.go_to_line(n);
            }
            self.goto_line_open = false;
        }
        if !open {
            self.goto_line_open = false;
        }
    }

    /// Options panel - appearance (theme), editor mode, default font, conversion.
    /// Light warm Heimdall is the default identity; dark is offered as an option.
    pub(crate) fn show_options_panel(&mut self, ctx: &egui::Context) {
        if !self.options_open {
            return;
        }
        let mut open = true;
        let prev_font = self.selected_font.clone();
        egui::Window::new("Options")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .resizable(false)
            .default_width(440.0)
            .show(ctx, |ui| {
                // ── Appearance ────────────────────────────────────────────────
                ui.label(egui::RichText::new("Appearance").strong().color(theme::TEXT));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Theme:");
                    // Light first - it is the default identity, not dark.
                    if ui.selectable_label(!self.dark_mode, "Light").clicked() {
                        self.dark_mode = false;
                    }
                    if ui.selectable_label(self.dark_mode, "Dark").clicked() {
                        self.dark_mode = true;
                    }
                });

                ui.add_space(10.0);

                // ── Editor ────────────────────────────────────────────────────
                ui.label(egui::RichText::new("Editor").strong().color(theme::TEXT));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    if ui
                        .selectable_label(self.editor_mode == EditorMode::SegmentedFlow, "Segmented flow")
                        .on_hover_text("Continuous document with inline rendered equations (default)")
                        .clicked()
                    {
                        self.editor_mode = EditorMode::SegmentedFlow;
                        self.segments_dirty = true;
                    }
                    if ui
                        .selectable_label(self.editor_mode == EditorMode::Block, "Block")
                        .on_hover_text("Click a block to edit its markdown source")
                        .clicked()
                    {
                        self.editor_mode = EditorMode::Block;
                        self.segments_dirty = true;
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Default font:");
                    egui::ComboBox::from_id_salt("opt_font")
                        .width(180.0)
                        .selected_text(egui::RichText::new(&self.selected_font).size(13.0))
                        .show_ui(ui, |ui| {
                            for (name, _path) in &self.font_list {
                                if name == "---" {
                                    ui.separator();
                                } else {
                                    ui.selectable_value(
                                        &mut self.selected_font,
                                        name.clone(),
                                        egui::RichText::new(name.as_str()).size(13.0),
                                    );
                                }
                            }
                        });
                });

                ui.add_space(10.0);

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Editing toolbar:");
                    if ui.selectable_label(self.toolbar_minified, "Minified")
                        .on_hover_text("Compact, icon-forward editing bar (default)").clicked()
                    { self.toolbar_minified = true; }
                    if ui.selectable_label(!self.toolbar_minified, "Full")
                        .on_hover_text("Every control: zoom, font, sizes, full formatting").clicked()
                    { self.toolbar_minified = false; }
                });

                ui.add_space(10.0);

                // ── Page layout ───────────────────────────────────────────────
                ui.label(egui::RichText::new("Page layout").strong().color(theme::TEXT));
                ui.separator();
                ui.checkbox(&mut self.show_page_numbers, "Show page numbers in the footer");
                ui.horizontal(|ui| {
                    ui.label("Header:");
                    ui.add(egui::TextEdit::singleline(&mut self.header_text)
                        .hint_text("optional, repeated on every page")
                        .desired_width(280.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Footer:");
                    ui.add(egui::TextEdit::singleline(&mut self.footer_text)
                        .hint_text("optional, repeated on every page")
                        .desired_width(280.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Page colour:");
                    ui.color_edit_button_srgb(&mut self.page_color);
                    ui.checkbox(&mut self.page_frame, "Frame around each page");
                });

                ui.add_space(10.0);

                // ── Accessibility ─────────────────────────────────────────────
                ui.label(egui::RichText::new("Accessibility").strong().color(theme::TEXT));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Icon set:");
                    if ui.selectable_label(matches!(self.icon_set, IconSet::Sober), "Sober")
                        .on_hover_text("Monochrome, gold on hover (default)").clicked()
                    { self.icon_set = IconSet::Sober; }
                    if ui.selectable_label(matches!(self.icon_set, IconSet::Colored), "Colored")
                        .on_hover_text("Coloured by family: colour and shape, never colour alone (WCAG 1.4.1)").clicked()
                    { self.icon_set = IconSet::Colored; }
                    if ui.selectable_label(matches!(self.icon_set, IconSet::HighContrast), "High contrast")
                        .on_hover_text("Full-strength glyphs (WCAG AAA)").clicked()
                    { self.icon_set = IconSet::HighContrast; }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Preview").size(12.0).color(theme::TEXT_MUTED));
                    ui.add_space(4.0);
                    // Reflect the selection immediately in the preview row.
                    icons::set_icon_set(self.icon_set);
                    for ic in [Icon::Bold, Icon::Italic, Icon::ListBullet, Icon::Quote, Icon::Link, Icon::Image, Icon::Sigma, Icon::Code] {
                        let _ = icons::lively_icon_button(ui, ic, "");
                    }
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Interface scale:");
                    ui.add(
                        egui::Slider::new(&mut self.ui_scale, 1.0..=2.0)
                            .step_by(0.05)
                            .custom_formatter(|n, _| format!("{}%", (n * 100.0).round() as i32)),
                    );
                });
                ui.checkbox(&mut self.a11y_high_contrast, "High contrast (maximum-contrast text and edges)");
                ui.checkbox(&mut self.a11y_reduced_motion, "Reduced motion (no UI animations)");
                ui.checkbox(&mut self.a11y_large_targets, "Larger click targets");

                ui.add_space(10.0);

                // ── Conversion ────────────────────────────────────────────────
                ui.label(egui::RichText::new("Conversion").strong().color(theme::TEXT));
                ui.separator();
                ui.radio_value(&mut self.conversion_settings.auto_save, false, "Ask where to save (Save As)");
                ui.radio_value(&mut self.conversion_settings.auto_save, true, "Auto-save next to source file");
                if self.conversion_settings.auto_save {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Affix:");
                        ui.radio_value(&mut self.conversion_settings.use_prefix, false, "Suffix");
                        ui.radio_value(&mut self.conversion_settings.use_prefix, true, "Prefix");
                        ui.text_edit_singleline(&mut self.conversion_settings.affix);
                    });
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(format!("{}:", crate::i18n::t("options.pdf_engine")));
                    if ui
                        .selectable_label(self.pdf_native, crate::i18n::t("options.pdf_engine.native"))
                        .on_hover_text(crate::i18n::t("options.pdf_engine.native_hint"))
                        .clicked()
                    {
                        self.pdf_native = true;
                    }
                    if ui
                        .selectable_label(!self.pdf_native, crate::i18n::t("options.pdf_engine.general"))
                        .on_hover_text(crate::i18n::t("options.pdf_engine.general_hint"))
                        .clicked()
                    {
                        self.pdf_native = false;
                    }
                });

                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.options_open = false;
                    }
                });
            });

        if self.selected_font != prev_font {
            self.apply_font_change(ctx);
        }
        if !open {
            self.options_open = false;
        }
    }

    /// Render transient bottom-right toast notifications, decrementing their
    /// lifetime by the frame delta and dropping expired ones.
    pub(crate) fn show_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        for t in &mut self.toasts {
            t.remaining -= dt;
        }
        self.toasts.retain(|t| t.remaining > 0.0);
        if self.toasts.is_empty() {
            return;
        }
        // Keep timing/animation alive while toasts are visible.
        ctx.request_repaint();

        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .interactable(true)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    let mut dismiss: Option<usize> = None;
                    for (i, t) in self.toasts.iter().enumerate() {
                        let accent = match t.kind {
                            ToastKind::Success => theme::SUCCESS,
                            ToastKind::Error => theme::ERROR,
                            ToastKind::Info => theme::ACCENT,
                        };
                        egui::Frame::default()
                            .fill(theme::SURFACE)
                            .stroke(egui::Stroke::new(1.0, theme::BORDER))
                            .rounding(8.0)
                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                            .shadow(egui::epaint::Shadow {
                                offset: egui::vec2(0.0, 2.0),
                                blur: 8.0,
                                spread: 0.0,
                                color: egui::Color32::from_black_alpha(40),
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (dr, _) =
                                        ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                                    ui.painter().circle_filled(dr.center(), 4.0, accent);
                                    ui.label(egui::RichText::new(&t.message).size(12.5).color(theme::TEXT));
                                    if icons::icon_button(ui, Icon::Close, "").clicked() {
                                        dismiss = Some(i);
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }
                    if let Some(i) = dismiss {
                        self.toasts.remove(i);
                    }
                });
            });
    }
}
