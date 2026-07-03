//! Conversion Hub UI (drop zone, 3-phase state machine, format grid, conversion).
//! Methods on MdApp, extracted from main.rs. The core does the real work via mdall_core.

use eframe::egui;
use crate::MdApp;
use crate::ViewMode;
use crate::theme;
use crate::output_format::OutputFormat;
use crate::ui::state::{ConversionHub, FileStatus, HubFile, HubPhase};
use crate::ui::icons::{self, Icon};
use mdall_core::{export, export_formats, import, source_embed};

/// Extensions accepted by the hub file picker (mirrors import_to_md).
const SUPPORTED_EXTS: &[&str] = &[
    "md", "markdown", "txt", "docx", "html", "htm", "epub", "odt", "rtf",
    "tex", "latex", "org", "rst", "wiki", "adoc", "asciidoc", "asc", "typ",
    "ipynb", "bib", "fb2", "pptx", "eml", "csv", "tsv", "rmd", "qmd",
    "py", "js", "ts", "rs", "c", "cpp", "java", "go", "rb", "php", "sh", "r",
];

/// Output formats offered in the hub (grid + per-file override).
const HUB_FORMATS: [OutputFormat; 13] = [
    OutputFormat::Pdf, OutputFormat::Docx, OutputFormat::Html, OutputFormat::Epub,
    OutputFormat::Md, OutputFormat::Odt, OutputFormat::Txt, OutputFormat::Tex,
    OutputFormat::Rtf, OutputFormat::Org, OutputFormat::Rst, OutputFormat::Adoc,
    OutputFormat::Ipynb,
];

/// Paint a vertical runic stave (Elder Futhark runes woven with Bifrost
/// ring-knots) into `rect`, in a soft bronze a few shades lighter than the
/// margin so it reads as a faded, tattoo-like engraving. Drawn procedurally so
/// it scales with the window and themes with one colour. `mirror` flips it
/// horizontally for the right-hand border.
pub(crate) fn paint_runic_stave(painter: &egui::Painter, rect: egui::Rect, col: egui::Color32, mirror: bool) {
    // Local stave space: 70 wide x 520 tall, scaled uniformly to fit `rect`.
    let s = (rect.width() / 70.0).min(rect.height() / 520.0).max(0.1);
    let ox = rect.center().x - 35.0 * s;
    let oy = rect.center().y - 260.0 * s;
    let p = |lx: f32, ly: f32| {
        let x = if mirror { 70.0 - lx } else { lx };
        egui::pos2(ox + x * s, oy + ly * s)
    };
    let st = egui::Stroke::new((3.2 * s).max(1.0), col);
    let kt = egui::Stroke::new((2.4 * s).max(0.8), col);
    let seg = |a: (f32, f32), b: (f32, f32)| painter.line_segment([p(a.0, a.1), p(b.0, b.1)], st);

    // Faint central spine.
    painter.line_segment([p(35.0, 40.0), p(35.0, 480.0)],
        egui::Stroke::new((1.0 * s).max(0.5), col.linear_multiply(0.5)));
    // Crowning arc (Bifrost) top + bottom.
    painter.line_segment([p(14.0, 32.0), p(56.0, 32.0)], kt);
    painter.line_segment([p(14.0, 488.0), p(56.0, 488.0)], kt);

    // Hagalaz (Heimdall) - two staves + a crossbar.
    seg((22.0, 48.0), (22.0, 92.0)); seg((48.0, 48.0), (48.0, 92.0)); seg((22.0, 63.0), (48.0, 77.0));
    // Bifrost ring-knots.
    painter.circle_stroke(p(40.0, 118.0), 11.0 * s, kt);
    painter.circle_stroke(p(28.0, 118.0), 11.0 * s, kt);
    // Tiwaz - stave + upward chevron (victory / the watchman's spear).
    seg((35.0, 148.0), (35.0, 192.0)); seg((23.0, 168.0), (35.0, 148.0)); seg((35.0, 148.0), (47.0, 168.0));
    // Mannaz - two staves bound by an inner V.
    seg((21.0, 216.0), (21.0, 260.0)); seg((49.0, 216.0), (49.0, 260.0));
    seg((21.0, 216.0), (35.0, 240.0)); seg((49.0, 216.0), (35.0, 240.0));
    painter.circle_stroke(p(35.0, 286.0), 8.0 * s, kt);
    // Othala - diamond of heritage with two legs.
    seg((35.0, 314.0), (49.0, 336.0)); seg((49.0, 336.0), (35.0, 350.0));
    seg((35.0, 350.0), (21.0, 336.0)); seg((21.0, 336.0), (35.0, 314.0));
    seg((21.0, 336.0), (17.0, 360.0)); seg((49.0, 336.0), (53.0, 360.0));
    // Raidho - the journey: stave, bow, riding leg.
    seg((22.0, 386.0), (22.0, 430.0)); seg((22.0, 386.0), (44.0, 392.0));
    seg((44.0, 392.0), (22.0, 398.0)); seg((22.0, 398.0), (48.0, 430.0));
    painter.circle_stroke(p(35.0, 455.0), 9.0 * s, kt);
}

impl MdApp {
    // ── Conversion Hub ────────────────────────────────────────────────────────

    /// Proper custom format button - one widget, correct contrast everywhere.
    ///
    /// Architecture: `allocate_exact_size(Sense::click())` = the ONE interactive region.
    /// Everything drawn by painter. No egui::Button underneath.
    ///
    /// Contrast guarantees:
    ///   badge text (white) on badge fill (brand color): always ≥ 4.5:1
    ///   label text (TEXT_2 dark) on button bg (near-transparent): ≥ 7:1
    fn hub_format_button(ui: &mut egui::Ui, fmt: OutputFormat, btn_w: f32) -> egui::Response {
        let btn_h   = 62.0;
        let badge_s = 34.0;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(btn_w, btn_h),
            egui::Sense::click(),
        );

        if !ui.is_rect_visible(rect) { return resp; }

        let col      = fmt.color();
        let hovered  = resp.hovered();
        let clicked  = resp.is_pointer_button_down_on();

        // ── Button background ────────────────────────────────────────────────
        let bg_alpha = if clicked { 35u8 } else if hovered { 22u8 } else { 10u8 };
        let bg = egui::Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), bg_alpha);
        ui.painter().rect_filled(rect, 8.0, bg);

        // Border - subtle at rest, more visible on hover
        let bd_alpha = if hovered { 180u8 } else { 60u8 };
        let bd_width = if hovered { 1.5 } else { 1.0 };
        ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(bd_width,
            egui::Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), bd_alpha)));

        // ── Badge (format color bg, white text) ──────────────────────────────
        let badge_x = rect.center().x - badge_s * 0.5;
        let badge_y = rect.min.y + 7.0;
        let badge_r = egui::Rect::from_min_size(
            egui::pos2(badge_x, badge_y), egui::vec2(badge_s, badge_s),
        );
        let brd = badge_s * 0.23;
        // Badge fill (solid brand color - white text reads on this)
        ui.painter().rect_filled(badge_r, brd, col);
        // Subtle gloss highlight top-left
        ui.painter().rect_filled(
            egui::Rect::from_min_size(badge_r.min + egui::vec2(2.0, 2.0),
                egui::vec2(badge_s * 0.52, badge_s * 0.38)),
            egui::Rounding { nw: brd, ne: brd * 0.4, sw: 0.0, se: 0.0 },
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 45),
        );
        // Badge abbreviation - WHITE on colored badge = high contrast ✓
        ui.painter().text(
            badge_r.center(),
            egui::Align2::CENTER_CENTER,
            fmt.label(),
            egui::FontId::new(badge_s * 0.30, egui::FontFamily::Proportional),
            egui::Color32::WHITE,
        );

        // ── Label below badge - DARK text on near-transparent bg = contrast ✓ ─
        let label_y = badge_y + badge_s + 3.5;
        ui.painter().text(
            egui::pos2(rect.center().x, label_y),
            egui::Align2::CENTER_TOP,
            fmt.label(),
            egui::FontId::new(10.0, egui::FontFamily::Proportional),
            theme::TEXT_2, // dark warm #6B5440 - not the format color!
        );

        // Hover ring
        if hovered {
            ui.painter().rect_stroke(rect.expand(1.5), 9.0,
                egui::Stroke::new(2.0, col));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        resp
    }

    pub(crate) fn show_converter_hub(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // ── Detect drag & drop (multi-file) ─────────────────────────────────
        self.conversion_hub.hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        for file in &dropped {
            if let Some(path) = &file.path {
                self.add_hub_file(path.clone());
            }
        }

        // ── Process the batch queue, one file per frame (keeps UI responsive) ─
        if self.conversion_hub.converting {
            self.process_queue();
            ctx.request_repaint();
        }

        let avail = ui.available_size();

        // ── Runic Heimdall borders framing the central column ────────────────
        // Soft bronze a few shades lighter than the #5C4A38 margin for a faded,
        // engraved feel. Right border mirrored. Drawn behind the centred content.
        let margin_w = (avail.x * 0.12).clamp(90.0, 200.0);
        {
            let area = ui.max_rect();
            let rune_col = egui::Color32::from_rgb(122, 100, 74); // softly blended, ~2 shades up from #5C4A38
            let left = egui::Rect::from_min_max(area.left_top(), egui::pos2(area.left() + margin_w, area.bottom()));
            let right = egui::Rect::from_min_max(egui::pos2(area.right() - margin_w, area.top()), area.right_bottom());
            paint_runic_stave(ui.painter(), left.shrink2(egui::vec2(8.0, 56.0)), rune_col, false);
            paint_runic_stave(ui.painter(), right.shrink2(egui::vec2(8.0, 56.0)), rune_col, true);
        }

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(ui.max_rect()), |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                // Same vertical margin as the runic borders (56px top & bottom)
                // so the white card aligns to exactly the runes' height.
                ui.add_space(56.0);

                let card = egui::Frame::default()
                    .fill(egui::Color32::WHITE)
                    .rounding(12.0)
                    .inner_margin(egui::Margin { left: 32.0, right: 32.0, top: 0.0, bottom: 24.0 })
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0), blur: 20.0, spread: 0.0,
                        color: egui::Color32::from_rgba_unmultiplied(42, 31, 15, 50),
                    });

                card.show(ui, |ui| {
                    // Fill the central column between the runic borders, and span
                    // the same vertical band as the runes (56px margin top & bottom).
                    let card_w = (avail.x - 2.0 * margin_w - 80.0).clamp(560.0, 1300.0);
                    let card_h = (avail.y - 112.0).max(320.0);
                    ui.set_min_width(card_w);
                    ui.set_max_width(card_w);
                    ui.set_min_height(card_h);
                    ui.set_max_height(card_h);

                    // ── Gold accent stripe - visual identity anchor ───────────
                    {
                        let w = ui.available_width();
                        let (r, _) = ui.allocate_exact_size(egui::vec2(w, 3.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, egui::Rounding { nw: 12.0, ne: 12.0, sw: 0.0, se: 0.0 }, theme::ACCENT);
                    }
                    ui.add_space(18.0);

                    // ── Logo (large) + tagline, centred at the top of the card ──
                    ui.vertical_centered(|ui| {
                        if let Some(ref tex) = self.logo_tex {
                            let ts = tex.size_vec2();
                            let logo_h = (avail.y * 0.20).clamp(130.0, 210.0);
                            let logo_sz = egui::vec2(ts.x * logo_h / ts.y, logo_h);
                            let (lr, _) = ui.allocate_exact_size(logo_sz, egui::Sense::hover());
                            ui.painter().image(tex.id(), lr,
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE);
                        }
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new("Write your equations once.  Export everywhere.  Recover everything.")
                            .size(12.5).color(theme::TEXT_MUTED));
                    });
                    ui.add_space(18.0);

                    // ── Unified drop + browse zone (click = browse, drop = load) ──
                    let drop_h = 112.0;
                    let drop_w = ui.available_width();
                    let (drop_rect, zone_resp) = ui.allocate_exact_size(
                        egui::vec2(drop_w, drop_h), egui::Sense::click(),
                    );
                    let hovering = self.conversion_hub.hovering;
                    let active   = hovering || zone_resp.hovered();
                    let zone_fill = if hovering { theme::ACCENT_PALE } else { theme::SURFACE_SOFT };
                    let zone_stroke_col = if active { theme::ACCENT } else { theme::BORDER };
                    ui.painter().rect_filled(drop_rect, 8.0, zone_fill);
                    ui.painter().rect_stroke(drop_rect, 8.0, egui::Stroke::new(1.5, zone_stroke_col));
                    if zone_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    // Gold tick corners - dashed border feel
                    {
                        let t = 14.0_f32;
                        let c = egui::Color32::from_rgba_unmultiplied(201, 146, 10, 120);
                        let s = egui::Stroke::new(2.0, c);
                        let r = drop_rect;
                        ui.painter().line_segment([r.left_top(), r.left_top() + egui::vec2(t, 0.0)], s);
                        ui.painter().line_segment([r.left_top(), r.left_top() + egui::vec2(0.0, t)], s);
                        ui.painter().line_segment([r.right_top(), r.right_top() + egui::vec2(-t, 0.0)], s);
                        ui.painter().line_segment([r.right_top(), r.right_top() + egui::vec2(0.0, t)], s);
                        ui.painter().line_segment([r.left_bottom(), r.left_bottom() + egui::vec2(t, 0.0)], s);
                        ui.painter().line_segment([r.left_bottom(), r.left_bottom() + egui::vec2(0.0, -t)], s);
                        ui.painter().line_segment([r.right_bottom(), r.right_bottom() + egui::vec2(-t, 0.0)], s);
                        ui.painter().line_segment([r.right_bottom(), r.right_bottom() + egui::vec2(0.0, -t)], s);
                    }
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(drop_rect), |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.add_space(14.0);
                            let (ir, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                            let (icon, tint) = if hovering { (Icon::ChevronDown, theme::ACCENT) }
                                               else { (Icon::Open, theme::TEXT_2) };
                            icons::paint_icon(ui.painter(), icon, ir.shrink(3.0), tint);
                            ui.label(egui::RichText::new(
                                if hovering { "Release to load" } else { "Drop files here - or click to browse" }
                            ).size(14.0).strong().color(theme::TEXT));
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("Load your file(s) and edit and/or convert!")
                                .size(11.5).color(theme::TEXT_MUTED));
                        });
                    });
                    if zone_resp.clicked() {
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter("All supported", SUPPORTED_EXTS)
                            .add_filter("All files", &["*"])
                            .pick_files()
                        {
                            for p in paths { self.add_hub_file(p); }
                        }
                    }

                    // Quick path into a blank document in the Split editor.
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        if ui.add(egui::Button::new(
                            egui::RichText::new("Open the editor  (blank document, Split view)")
                                .size(13.0).color(theme::TEXT))
                            .fill(theme::ACCENT_PALE)
                            .stroke(egui::Stroke::new(1.0, theme::ACCENT))
                            .min_size(egui::vec2(300.0, 34.0)))
                            .on_hover_text("Start a new empty document in the source + rendered Split view")
                            .clicked()
                        {
                            self.open_blank_split_editor();
                        }
                    });

                    // ── File list + batch actions ────────────────────────────
                    if !self.conversion_hub.files.is_empty() {
                        ui.add_space(10.0);
                        {
                            let w = ui.available_width();
                            let (r, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
                            ui.painter().line_segment([r.left_center(), r.right_center()],
                                egui::Stroke::new(1.0, theme::BORDER));
                        }
                        ui.add_space(8.0);

                        let n = self.conversion_hub.files.len();
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} file{}", n, if n > 1 { "s" } else { "" }))
                                .size(12.5).strong().color(theme::TEXT_2));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Clear all").clicked() {
                                    self.conversion_hub = ConversionHub::default();
                                }
                                if ui.small_button("Add files...").clicked() {
                                    if let Some(paths) = rfd::FileDialog::new()
                                        .add_filter("All supported", SUPPORTED_EXTS)
                                        .add_filter("All files", &["*"])
                                        .pick_files()
                                    { for p in paths { self.add_hub_file(p); } }
                                }
                            });
                        });
                        ui.add_space(6.0);

                        let mut remove: Option<usize> = None;
                        let mut open_editor: Option<usize> = None;
                        egui::ScrollArea::vertical().max_height(170.0).id_salt("hub_files").show(ui, |ui| {
                            for i in 0..self.conversion_hub.files.len() {
                                self.hub_file_row(ui, i, &mut remove, &mut open_editor);
                                ui.add_space(4.0);
                            }
                        });
                        if let Some(i) = remove {
                            if i < self.conversion_hub.files.len() {
                                self.conversion_hub.files.remove(i);
                                self.conversion_hub.selected = None;
                                if self.conversion_hub.files.is_empty() {
                                    self.conversion_hub.phase = HubPhase::Idle;
                                }
                            }
                        }
                        if let Some(i) = open_editor { self.open_hub_file_in_editor(i); }

                        // ── Primary actions ──────────────────────────────────
                        ui.add_space(8.0);
                        let single = self.conversion_hub.files.len() == 1;
                        ui.horizontal(|ui| {
                            let btn_w = (ui.available_width() - 12.0) / 2.0;
                            if single {
                                if ui.add_sized([btn_w, 36.0],
                                    egui::Button::new(egui::RichText::new("Open in Editor").size(13.0).strong().color(theme::TEXT_2))
                                        .fill(theme::SURFACE_SOFT)
                                        .stroke(egui::Stroke::new(1.5, theme::ACCENT))
                                ).clicked() {
                                    self.open_hub_file_in_editor(0);
                                }
                            } else {
                                ui.add_sized([btn_w, 36.0],
                                    egui::Label::new(egui::RichText::new("Open a file via its ⚙ to edit")
                                        .size(11.0).italics().color(theme::TEXT_MUTED)));
                            }
                            ui.add_space(12.0);
                            let convert_label = if single { "Convert..." } else { "Convert all..." };
                            let picking = self.conversion_hub.pick_format;
                            if ui.add_sized([btn_w, 36.0],
                                egui::Button::new(egui::RichText::new(
                                    if picking { format!("{}  ▲", convert_label) }
                                    else { format!("{}  ▼", convert_label) }
                                ).size(13.0).strong().color(egui::Color32::WHITE))
                                .fill(if picking { theme::ACCENT_HOVER } else { theme::ACCENT })
                            ).clicked() {
                                self.conversion_hub.pick_format = !self.conversion_hub.pick_format;
                            }
                        });

                        // ── Format picker ────────────────────────────────────
                        if self.conversion_hub.pick_format {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(3.0, 16.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 1.0, theme::ACCENT);
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(
                                    if single { "Choose output format" } else { "Choose output format for all" }
                                ).size(12.5).strong().color(theme::TEXT_2));
                            });
                            ui.add_space(8.0);
                            const COLS: usize = 4;
                            let avail_w = ui.available_width();
                            let btn_w = (avail_w - (COLS as f32 - 1.0) * 8.0) / COLS as f32;
                            let mut clicked_fmt: Option<OutputFormat> = None;
                            egui::Grid::new("fmt_grid").num_columns(COLS).spacing([8.0, 8.0]).show(ui, |ui| {
                                for (i, &fmt) in HUB_FORMATS.iter().enumerate() {
                                    if Self::hub_format_button(ui, fmt, btn_w).clicked() { clicked_fmt = Some(fmt); }
                                    if (i + 1) % COLS == 0 { ui.end_row(); }
                                }
                            });
                            if let Some(fmt) = clicked_fmt {
                                self.conversion_hub.pick_format = false;
                                if single { self.convert_single(0, fmt); }
                                else { self.start_batch(fmt); }
                            }
                        }

                        // ── Batch progress (bronze loader in the border palette) ──
                        if self.conversion_hub.converting {
                            let done = self.conversion_hub.queue_index;
                            let total = self.conversion_hub.files.len().max(1);
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(22.0).color(theme::ACCENT_HOVER));
                                ui.add_space(10.0);
                                ui.add(egui::ProgressBar::new(done as f32 / total as f32)
                                    .fill(theme::ACCENT)
                                    .text(egui::RichText::new(format!("Converting {}/{}", done.min(total), total))
                                        .color(theme::TEXT)));
                            });
                            ui.ctx().request_repaint(); // keep the spinner turning
                        }

                        // ── Global status ────────────────────────────────────
                        let status = self.conversion_hub.status.clone();
                        if !status.is_empty() {
                            ui.add_space(8.0);
                            let sc = if self.conversion_hub.is_error { theme::ERROR } else { theme::SUCCESS };
                            ui.label(egui::RichText::new(&status).size(12.0).color(sc));
                        }
                    }

                    // ── Conversion settings (collapsible) ────────────────────
                    ui.add_space(10.0);
                    egui::CollapsingHeader::new(
                        egui::RichText::new("Output settings").size(11.5).color(egui::Color32::GRAY)
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        ui.radio_value(&mut self.conversion_settings.auto_save, false,
                            "Ask where to save (Save As)");
                        ui.radio_value(&mut self.conversion_settings.auto_save, true,
                            "Auto-save next to source file");
                        ui.label(egui::RichText::new("Batch always auto-saves next to each source.")
                            .size(10.0).color(egui::Color32::GRAY).italics());
                        if self.conversion_settings.auto_save {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("Affix:");
                                ui.radio_value(&mut self.conversion_settings.use_prefix, false, "Suffix");
                                ui.radio_value(&mut self.conversion_settings.use_prefix, true, "Prefix");
                                ui.add(egui::TextEdit::singleline(&mut self.conversion_settings.affix)
                                    .desired_width(80.0));
                            });
                        }
                    });
                }); // card
            }); // top_down
        }); // allocate_new_ui
    }

    /// Import a file to Markdown source.
    ///
    /// Supports: .md, .markdown, .txt, .docx (lossless MD -> ALL export or generic),
    ///           .html/.htm, .epub, .odt, .rtf
    pub(crate) fn import_to_md(path: &std::path::Path) -> Result<String, String> {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "md" | "markdown" => {
                mdall_core::text_encoding::read_text(path)
            }
            "txt" => {
                mdall_core::text_encoding::read_text(path)
            }
            "docx" => {
                // Try MD -> ALL lossless first (embedded md-to-all-source.xml)
                match source_embed::import_docx_source(path) {
                    Ok(md) => return Ok(md),
                    Err(_) => {}
                }
                // Fall back to generic Word XML extraction
                import::docx_generic_to_md(path)
            }
            "html" | "htm" => {
                let html = mdall_core::text_encoding::read_text(path)?;
                import::html_to_md(&html)
            }
            "epub" => import::epub_to_md(path),
            "odt"  => import::odt_to_md(path),
            "rtf"  => import::rtf_to_md(path),
            // Extended markup formats
            "tex" | "latex" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::tex_to_md(&s)
            }
            "org" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::org_to_md(&s)
            }
            "rst" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::rst_to_md(&s)
            }
            "wiki" | "mediawiki" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::wiki_to_md(&s)
            }
            "adoc" | "asciidoc" | "asc" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::adoc_to_md(&s)
            }
            "typ" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::typ_to_md(&s)
            }
            // Structured data
            "ipynb" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::ipynb_to_md(&s)
            }
            "bib" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::bib_to_md(&s)
            }
            "fb2"  => import::fb2_to_md(path),
            "pptx" => import::pptx_to_md(path),
            "eml"  => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::eml_to_md(&s)
            }
            "csv" | "tsv" => {
                let s = mdall_core::text_encoding::read_text(path)?;
                import::csv_to_md(&s)
            }
            // R Markdown / Quarto - already Markdown, read as-is
            "rmd" | "qmd" | "rmarkdown" => {
                mdall_core::text_encoding::read_text(path)
            }
            // Source code files → fenced code block
            "py"            => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "python") }
            "js"            => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "javascript") }
            "ts"            => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "typescript") }
            "rs"            => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "rust") }
            "c"             => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "c") }
            "cpp"|"cxx"|"cc"=> { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "cpp") }
            "java"          => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "java") }
            "go"            => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "go") }
            "rb"            => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "ruby") }
            "php"           => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "php") }
            "sh"|"bash"|"zsh"=>{ let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "bash") }
            "r"             => { let s = mdall_core::text_encoding::read_text(path)?; import::code_to_md(&s, "r") }
            ext => Err(format!(
                ".{} import is not yet implemented.\n\
                 Supported: md, txt, docx, html, epub, odt, rtf, tex, org, rst,\
                 wiki, adoc, typ, ipynb, bib, fb2, pptx, eml, csv, tsv, rmd, qmd,\
                 py, js, ts, rs, c, cpp, java, go, rb, php, sh, r", ext
            )),
        }
    }

    /// Output file path for `src` with extension `ext`, per the conversion
    /// settings. `force_auto` skips the Save As dialog (used by batch, which
    /// always auto-saves next to each source). None = dialog cancelled.
    fn output_path_for(
        &self,
        src: &std::path::Path,
        ext: &str,
        force_auto: bool,
    ) -> Option<std::path::PathBuf> {
        let stem = src.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let suggested = if self.conversion_settings.use_prefix {
            format!("{}_{}.{}", self.conversion_settings.affix, stem, ext)
        } else {
            format!("{}_{}.{}", stem, self.conversion_settings.affix, ext)
        };

        if force_auto || self.conversion_settings.auto_save {
            let dir = src.parent().unwrap_or(std::path::Path::new("."));
            Some(dir.join(&suggested))
        } else {
            let mut dlg = rfd::FileDialog::new()
                .set_file_name(&suggested)
                .add_filter(&ext.to_uppercase(), &[ext])
                .add_filter("All files", &["*"]);
            if let Some(dir) = src.parent() {
                dlg = dlg.set_directory(dir);
            }
            dlg.save_file()
        }
    }

    /// Add a file to the hub (drag&drop or browse), de-duplicated.
    fn add_hub_file(&mut self, path: std::path::PathBuf) {
        if self.conversion_hub.files.iter().any(|f| f.path == path) {
            return;
        }
        self.conversion_hub.files.push(HubFile::new(path));
        self.conversion_hub.phase = HubPhase::FileReady;
        self.conversion_hub.status = String::new();
        self.conversion_hub.is_error = false;
        self.conversion_hub.converted_md = None;
    }

    /// Import file `i` into the editor and switch to Editor view.
    fn open_hub_file_in_editor(&mut self, i: usize) {
        let path = match self.conversion_hub.files.get(i) {
            Some(f) => f.path.clone(),
            None => return,
        };
        match Self::import_to_md(&path) {
            Ok(md) => {
                self.source = md.clone();
                self.current_file = Some(path);
                self.modified = false;
                self.segments_dirty = true;
                self.conversion_hub.converted_md = Some(md);
                self.view_mode = ViewMode::Editor;
            }
            Err(e) => {
                self.conversion_hub.status = format!("Import error: {}", e);
                self.conversion_hub.is_error = true;
            }
        }
    }

    /// One file row in the hub list: status dot, ext badge, name, options + remove.
    fn hub_file_row(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        remove: &mut Option<usize>,
        open_editor: &mut Option<usize>,
    ) {
        let (name, ext, status, msg, out) = {
            let f = &self.conversion_hub.files[i];
            (
                f.path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                f.path.extension().and_then(|e| e.to_str()).unwrap_or("?").to_uppercase(),
                f.status,
                f.message.clone(),
                f.output_path.clone(),
            )
        };
        let selected = self.conversion_hub.selected == Some(i);
        egui::Frame::default()
            .fill(if selected { theme::SURFACE_SOFT } else { theme::SURFACE })
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .rounding(6.0)
            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Status indicator
                    let (sr, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    match status {
                        FileStatus::Done => {
                            ui.painter().add(egui::Shape::line(
                                vec![
                                    egui::pos2(sr.left() + 0.20 * sr.width(), sr.top() + 0.55 * sr.height()),
                                    egui::pos2(sr.left() + 0.42 * sr.width(), sr.top() + 0.78 * sr.height()),
                                    egui::pos2(sr.left() + 0.82 * sr.width(), sr.top() + 0.25 * sr.height()),
                                ],
                                egui::Stroke::new(2.0, theme::SUCCESS),
                            ));
                        }
                        FileStatus::Failed => {
                            let s = egui::Stroke::new(2.0, theme::ERROR);
                            ui.painter().line_segment([sr.left_top() + egui::vec2(3.0, 3.0), sr.right_bottom() - egui::vec2(3.0, 3.0)], s);
                            ui.painter().line_segment([sr.right_top() + egui::vec2(-3.0, 3.0), sr.left_bottom() + egui::vec2(3.0, -3.0)], s);
                        }
                        FileStatus::Pending => {
                            ui.painter().circle_filled(sr.center(), 3.0, theme::TEXT_MUTED);
                        }
                    }
                    ui.label(egui::RichText::new(&ext).size(10.0).strong().color(theme::ACCENT_HOVER));
                    ui.label(egui::RichText::new(&name).size(12.5).color(theme::TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::icon_button(ui, Icon::Close, "Remove").clicked() {
                            *remove = Some(i);
                        }
                        if icons::icon_button(ui, Icon::Settings, "Options").clicked() {
                            self.conversion_hub.selected = if selected { None } else { Some(i) };
                        }
                        if let Some(p) = &out {
                            if ui.small_button("Open").clicked() { let _ = open::that(p); }
                        }
                    });
                });
                if selected {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Convert to:").size(11.0).color(theme::TEXT_2));
                        let cur_label = self.conversion_hub.files[i].target
                            .map(|f| f.label().to_string())
                            .unwrap_or_else(|| "Batch default".into());
                        egui::ComboBox::from_id_salt(("file_fmt", i))
                            .selected_text(cur_label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.conversion_hub.files[i].target, None, "Batch default");
                                for f in HUB_FORMATS {
                                    ui.selectable_value(&mut self.conversion_hub.files[i].target, Some(f), f.label());
                                }
                            });
                        if ui.small_button("Open in editor").clicked() {
                            *open_editor = Some(i);
                        }
                    });
                    if !msg.is_empty() {
                        let c = if status == FileStatus::Failed { theme::ERROR } else { theme::TEXT_MUTED };
                        ui.label(egui::RichText::new(&msg).size(10.5).color(c));
                    }
                }
            });
    }

    /// Convert one file to `fmt`. `force_auto` saves next to the source with no
    /// dialog (batch). Returns the output path on success; never corrupts.
    fn convert_one(
        &self,
        src: &std::path::Path,
        fmt: OutputFormat,
        force_auto: bool,
    ) -> Result<std::path::PathBuf, String> {
        let md = Self::import_to_md(src)?;
        if fmt == OutputFormat::Md {
            let out = self.output_path_for(src, "md", force_auto).ok_or("cancelled")?;
            std::fs::write(&out, &md).map_err(|e| e.to_string())?;
            return Ok(out);
        }
        let ext = fmt.ext();
        let out = self.output_path_for(src, ext, force_auto).ok_or("cancelled")?;
        let metadata = self.meta.clone();
        let src_dir = src.parent();
        // Batch conversion renders a different document than the editor, so install
        // THIS file's custom LaTeX macros for the equation renderers (the editor's
        // per-frame table would otherwise be wrong here).
        mdall_core::latex_macros::install_from_source(&md);
        let result: Result<(), String> = match fmt {
            OutputFormat::Pdf  => export::export_pdf(&md, &out, &metadata, src_dir),
            OutputFormat::Html => export::export_html(&md, &out, &metadata, src_dir),
            OutputFormat::Txt  => export_formats::export_txt(&md, &out),
            OutputFormat::Tex  => export_formats::export_tex(&md, &out, &metadata),
            OutputFormat::Rtf  => export_formats::export_rtf(&md, &out, &metadata, src_dir),
            OutputFormat::Docx => export_formats::export_docx(&md, &out, &metadata, src_dir),
            OutputFormat::Odt  => export_formats::export_odt(&md, &out, &metadata, src_dir),
            OutputFormat::Epub => export_formats::export_epub(&md, &out, &metadata, src_dir),
            OutputFormat::Org  => export_formats::export_org(&md, &out, &metadata),
            OutputFormat::Rst  => export_formats::export_rst(&md, &out, &metadata),
            OutputFormat::Adoc => export_formats::export_adoc(&md, &out, &metadata),
            OutputFormat::Ipynb => export_formats::export_ipynb(&md, &out, &metadata),
            // MD is handled by the early return above; keep a real write here as
            // defense-in-depth so a future refactor can never turn this into a panic.
            OutputFormat::Md   => std::fs::write(&out, &md).map_err(|e| e.to_string()),
        };
        result?;
        if out.exists() { Ok(out) } else { Err("export finished but output not found".into()) }
    }

    /// Convert a single loaded file synchronously (respects the auto-save setting).
    fn convert_single(&mut self, i: usize, fmt: OutputFormat) {
        let path = match self.conversion_hub.files.get(i) { Some(f) => f.path.clone(), None => return };
        match self.convert_one(&path, fmt, false) {
            Ok(out) => {
                let name = out.file_name().unwrap_or_default().to_string_lossy().to_string();
                if let Some(f) = self.conversion_hub.files.get_mut(i) {
                    f.status = FileStatus::Done;
                    f.output_path = Some(out);
                    f.message = "Converted".into();
                }
                self.conversion_hub.status = format!("Converted → {}", name);
                self.conversion_hub.is_error = false;
            }
            Err(e) => {
                if e != "cancelled" {
                    if let Some(f) = self.conversion_hub.files.get_mut(i) {
                        f.status = FileStatus::Failed;
                        f.message = e.clone();
                    }
                    self.conversion_hub.status = format!("Error: {}", e);
                    self.conversion_hub.is_error = true;
                }
            }
        }
    }

    /// Start a batch: set the shared target, reset and kick off the per-frame queue.
    fn start_batch(&mut self, fmt: OutputFormat) {
        self.conversion_hub.batch_target = Some(fmt);
        self.conversion_hub.converting = true;
        self.conversion_hub.queue_index = 0;
        self.conversion_hub.status = String::new();
        self.conversion_hub.is_error = false;
    }

    /// Process one file of the batch queue (called once per frame while converting).
    /// Batch always auto-saves next to each source so mixed extensions stay sane.
    fn process_queue(&mut self) {
        let total = self.conversion_hub.files.len();
        let i = self.conversion_hub.queue_index;
        if i >= total {
            self.conversion_hub.converting = false;
            let failed = self.conversion_hub.files.iter()
                .filter(|f| f.status == FileStatus::Failed).count();
            self.conversion_hub.status = if failed == 0 {
                format!("Batch complete - {} file{} converted", total, if total > 1 { "s" } else { "" })
            } else {
                format!("Batch finished - {} converted, {} failed", total - failed, failed)
            };
            self.conversion_hub.is_error = failed > 0;
            return;
        }
        let (path, fmt) = {
            let f = &self.conversion_hub.files[i];
            (f.path.clone(), f.target.or(self.conversion_hub.batch_target))
        };
        if let Some(fmt) = fmt {
            match self.convert_one(&path, fmt, true) {
                Ok(out) => {
                    if let Some(f) = self.conversion_hub.files.get_mut(i) {
                        f.status = FileStatus::Done;
                        f.output_path = Some(out);
                        f.message = "Converted".into();
                    }
                }
                Err(e) => {
                    if let Some(f) = self.conversion_hub.files.get_mut(i) {
                        f.status = FileStatus::Failed;
                        f.message = e;
                    }
                }
            }
        }
        self.conversion_hub.queue_index += 1;
    }
}
