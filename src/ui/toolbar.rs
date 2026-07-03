//! Top formatting toolbar (view modes, formatting, paragraph style, zoom, font).
//! show_toolbar() is called from MdApp::update().

use eframe::egui;
use crate::MdApp;
use crate::theme;
use crate::ViewMode;
use crate::ui::state::LinkDialog;
use crate::ui::commands::InlineFmt;
use crate::ui::icons::{self, Icon};
use crate::i18n::t;

/// Classic preset palette (3 rows × 6): greys, vivid, soft.
const COLOR_PRESETS: [(u8, u8, u8); 18] = [
    (0, 0, 0), (68, 68, 68), (136, 136, 136), (170, 170, 170), (210, 210, 210), (255, 255, 255),
    (231, 76, 60), (230, 126, 34), (241, 196, 15), (39, 174, 96), (41, 128, 185), (142, 68, 173),
    (255, 182, 193), (255, 160, 122), (144, 238, 144), (22, 160, 133), (173, 216, 230), (216, 191, 216),
];

/// Color control offering BOTH a preset palette AND the HSV mixer in one popup.
/// `label` = the swatch glyph ("A" text color, "H" highlight). `highlight` tints
/// the glyph background instead of its text. Returns Some(rgb) when a color is
/// picked this frame (palette click or mixer change).
fn color_picker_combo(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    rgb: [u8; 3],
    highlight: bool,
) -> Option<[u8; 3]> {
    let cur = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let _ = label; // the conventional pictogram is painted, not the letter
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
    {
        let dark = ui.visuals().dark_mode;
        let bg = if resp.hovered() { ui.visuals().widgets.hovered.bg_fill } else { theme::btn_fill(dark) };
        let ink = theme::text_soft(dark);
        let p = ui.painter();
        p.rect_filled(rect, 4.0, bg);
        p.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, theme::BORDER));
        if highlight {
            // Highlighter marker: a grey body + a chisel tip in the highlight colour
            // over a colour strip - the universal "highlight" pictogram.
            let strip = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 5.0, rect.bottom() - 6.0),
                egui::pos2(rect.right() - 5.0, rect.bottom() - 3.5),
            );
            p.rect_filled(strip, 1.0, cur);
            let body = vec![
                egui::pos2(rect.left() + 9.0, rect.top() + 4.0),
                egui::pos2(rect.left() + 13.0, rect.top() + 4.0),
                egui::pos2(rect.right() - 7.0, rect.bottom() - 8.0),
                egui::pos2(rect.right() - 11.0, rect.bottom() - 8.0),
            ];
            p.add(egui::Shape::convex_polygon(body, ink.gamma_multiply(0.45), egui::Stroke::new(1.0, ink)));
            let tip = vec![
                egui::pos2(rect.right() - 7.0, rect.bottom() - 8.0),
                egui::pos2(rect.right() - 11.0, rect.bottom() - 8.0),
                egui::pos2(rect.right() - 9.0, rect.bottom() - 4.5),
            ];
            p.add(egui::Shape::convex_polygon(tip, cur, egui::Stroke::NONE));
        } else {
            // "A" with a colour bar beneath it - the universal "text colour" icon.
            p.text(
                egui::pos2(rect.center().x, rect.top() + 2.0),
                egui::Align2::CENTER_TOP,
                "A",
                egui::FontId::proportional(14.0),
                ink,
            );
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 6.0, rect.bottom() - 6.0),
                egui::pos2(rect.right() - 6.0, rect.bottom() - 3.5),
            );
            p.rect_filled(bar, 1.0, cur);
        }
    }
    let resp = resp.on_hover_text(if highlight { "Highlight colour" } else { "Text colour" });
    let popup_id = egui::Id::new(("colorpop", id));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let mut result = None;
    // Set when a discrete palette swatch is clicked, so the popup closes right
    // after the colour is applied. The custom mixer does NOT close it (the user
    // is dragging to fine-tune; closing on every change would fight the drag).
    let mut close_after = false;
    egui::popup::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
            egui::Grid::new((id, "presets")).spacing([3.0, 3.0]).show(ui, |ui| {
                for (i, &(r, g, b)) in COLOR_PRESETS.iter().enumerate() {
                    let c = egui::Color32::from_rgb(r, g, b);
                    let (rect, s) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 3.0, c);
                    let edge = if s.hovered() {
                        egui::Stroke::new(2.0, theme::ACCENT)
                    } else {
                        egui::Stroke::new(1.0, theme::BORDER)
                    };
                    ui.painter().rect_stroke(rect, 3.0, edge);
                    if s.clicked() {
                        result = Some([r, g, b]);
                        close_after = true;
                    }
                    if (i + 1) % 6 == 0 {
                        ui.end_row();
                    }
                }
            });
            // HSV mixer tucked into a collapsible row so the popup stays compact
            // (palette only) by default; expand it for a custom colour. The picker
            // only edits an in-progress colour (persisted across frames); it is
            // applied to the text ONLY on the explicit Apply button - which also
            // closes the popup. Applying live on every drag would splice a new
            // <span> into the source each frame (nested-markup bug).
            egui::CollapsingHeader::new(egui::RichText::new("Custom...").small().color(theme::TEXT_2))
                .id_salt((id, "mixer"))
                .default_open(false)
                .show(ui, |ui| {
                    let mem_id = egui::Id::new((id, "mixval"));
                    let mut c = ui.data(|d| d.get_temp::<egui::Color32>(mem_id)).unwrap_or(cur);
                    egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
                    ui.data_mut(|d| d.insert_temp(mem_id, c));
                    ui.add_space(2.0);
                    if ui.add(egui::Button::new(
                        egui::RichText::new("Apply").small().color(theme::text_strong(false)),
                    ).min_size(egui::vec2(ui.available_width(), 20.0)))
                        .clicked()
                    {
                        result = Some([c.r(), c.g(), c.b()]);
                        close_after = true;
                        ui.data_mut(|d| d.remove::<egui::Color32>(mem_id));
                    }
                });
        },
    );
    if close_after {
        ui.memory_mut(|m| m.close_popup());
    }
    result
}

impl MdApp {
    pub(crate) fn show_toolbar(&mut self, ctx: &egui::Context) {
        let dark = self.dark_mode;
        egui::TopBottomPanel::top("toolbar")
            .min_height(56.0)
            .frame(egui::Frame::default()
                .fill(theme::panel_bg(dark))
                .inner_margin(egui::Margin { left: 6.0, right: 6.0, top: 3.0, bottom: 3.0 }))
            .show(ctx, |ui| self.toolbar_ui(ui));
    }

    /// Dispatch the WYSIWYG editing toolbar contents into `ui`: the minified bar
    /// (default) or the full detailed bar, per the Options preset. Used as a top
    /// panel for the full-screen Editor, or embedded in the Split rendered pane.
    pub(crate) fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        if self.toolbar_minified {
            self.toolbar_ui_minified(ui);
        } else {
            self.toolbar_ui_full(ui);
        }
    }

    /// The full WYSIWYG formatting toolbar: every control (formatting, headings,
    /// color, align, lists, media, zoom, font). Selectable in Options as "Full".
    /// "ABC" spell-mode toggle with a red wavy underline (the spell-check
    /// signifier), for the WYSIWYG editing toolbar. Active when spell mode is on.
    fn spell_toggle_button(&mut self, ui: &mut egui::Ui) {
        let on = self.spell_enabled;
        let resp = ui
            .selectable_label(on, egui::RichText::new("ABC").size(13.5).strong())
            .on_hover_text("Spell check (red squiggles + suggestions)");
        let r = resp.rect;
        crate::ui::editor::draw_wavy_underline(
            ui.painter(),
            r.left() + 3.0,
            r.right() - 3.0,
            r.bottom() - 2.0,
            egui::Color32::from_rgb(0xCC, 0x30, 0x30),
        );
        if resp.clicked() {
            self.toggle_spell_mode();
        }
    }

    fn toolbar_ui_full(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark_mode;
            // Warm button helpers - consistent fill across toolbar
            let warm_fill = theme::btn_fill(dark); // warm grey (not cold)
            let btn = |label: egui::RichText| {
                egui::Button::new(label.color(theme::text_soft(dark)))
                    .min_size(egui::vec2(32.0, 26.0))
                    .fill(warm_fill)
            };
            let ibtn = |label: &str| btn(egui::RichText::new(label).size(14.0));

            // ── Row 1: text formatting + headings ───────────────────────────
            // (The view-mode switcher moved to the top bar - see draw_view_switcher,
            //  which is hover-revealed on the clean converter home.)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;

                // Character formatting - buttons light up when cursor is in matching format
                // (only active in Editor mode; in Source mode buttons always appear normal)
                let wysiwyg = self.view_mode == ViewMode::Editor;
                let fmt = self.wysiwyg_fmt;
                // ── Format button helper: explicit colors, no surprise states ──
                // Active  = logo gold bg   + black text  (4.9:1 contrast ✓)
                // Inactive = warm grey bg  + dark text   (7:1 contrast ✓)
                let fmt_btn = |label: egui::RichText, active: bool| {
                    let (bg, txt) = if active {
                        (theme::ACCENT, theme::TEXT)                // gold + dark ink
                    } else {
                        (theme::btn_fill(dark), theme::text_soft(dark)) // warm grey + mid-dark
                    };
                    egui::Button::new(label.color(txt))
                        .min_size(egui::vec2(32.0, 26.0))
                        .fill(bg)
                };

                if icons::icon_toggle(ui, Icon::Bold, wysiwyg && fmt.bold, "Bold (Ctrl+B)").clicked() { self.toggle_inline_format(InlineFmt::Bold); }
                if icons::icon_toggle(ui, Icon::Italic, wysiwyg && fmt.italic, "Italic (Ctrl+I)").clicked() { self.toggle_inline_format(InlineFmt::Italic); }
                if icons::icon_toggle(ui, Icon::Underline, false, "Underline (Ctrl+U)").clicked() { self.toggle_inline_format(InlineFmt::Underline); }
                if icons::icon_toggle(ui, Icon::Strikethrough, wysiwyg && fmt.strikethrough, "Strikethrough").clicked() { self.toggle_inline_format(InlineFmt::Strike); }
                if ui.add(fmt_btn(egui::RichText::new("x²").size(15.0), false))
                    .on_hover_text("Superscript").clicked() { self.wrap_text("<sup>", "</sup>"); }
                if ui.add(fmt_btn(egui::RichText::new("x₂").size(15.0), false))
                    .on_hover_text("Subscript").clicked() { self.wrap_text("<sub>", "</sub>"); }
                if icons::icon_toggle(ui, Icon::Code, wysiwyg && fmt.code, "Inline Code").clicked() { self.toggle_inline_format(InlineFmt::Code); }
                self.spell_toggle_button(ui);
                if ui.add(fmt_btn(egui::RichText::new("ab̲").size(14.0), false))
                    .on_hover_text("Mark / Highlight").clicked() { self.wrap_text("<mark>", "</mark>"); }

                ui.separator();

                // ── Paragraph style - ComboBox Word-style (Normal / H1-H6) ────
                {
                    let current_style = if wysiwyg && fmt.heading > 0 {
                        match fmt.heading {
                            1 => "Heading 1", 2 => "Heading 2", 3 => "Heading 3",
                            4 => "Heading 4", 5 => "Heading 5", _ => "Heading 6",
                        }
                    } else { "Normal" };

                    let sel_txt = egui::RichText::new(current_style).size(12.0).color(theme::text_soft(dark));
                    egui::ComboBox::from_id_salt("para_style")
                        .width(90.0)
                        .selected_text(sel_txt)
                        .show_ui(ui, |ui| {
                            let _ = ui.selectable_label(current_style == "Normal",
                                egui::RichText::new("Normal").size(13.0).color(theme::text_soft(dark)));
                            ui.separator();
                            for (label, prefix, lvl, sz) in [
                                ("Heading 1", "# ",   1u8, 16.0f32),
                                ("Heading 2", "## ",  2,   14.5),
                                ("Heading 3", "### ", 3,   13.5),
                                ("Heading 4", "#### ",4,   12.5),
                                ("Heading 5", "##### ",5,   12.0),
                                ("Heading 6", "###### ",6,  11.5),
                            ] {
                                if ui.selectable_label(
                                    wysiwyg && fmt.heading == lvl,
                                    egui::RichText::new(label).size(sz).strong().color(theme::text_strong(dark)),
                                ).clicked() {
                                    self.insert_text(prefix);
                                }
                            }
                        });
                }

                ui.separator();

                // Text color - preset palette OR custom mixer
                if let Some(rgb) = color_picker_combo(ui, "textcol", "A", self.text_color, false) {
                    self.text_color = rgb;
                    let hex = format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    self.wrap_value_span("span", "color", &format!("#{}", hex));
                }
                // Highlight color - preset palette OR custom mixer
                if let Some(rgb) = color_picker_combo(ui, "hlcol", "H", self.highlight_color, true) {
                    self.highlight_color = rgb;
                    let hex = format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    self.wrap_value_span("mark", "background", &format!("#{}", hex));
                }

                // Theme toggle, Convert, and Settings now live in the menu bar
                // (see show_menu_bar); the toolbar stays focused on editing tools.
            });

            // ── Row 2: alignment + lists + insert + zoom + font ──────────────
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;

                // Paragraph alignment - labelled, with bigger icons
                if icons::icon_button(ui, Icon::AlignLeft, "Align Left").clicked() { self.wrap_block_align("left"); }
                if icons::icon_button(ui, Icon::AlignCenter, "Center").clicked() { self.wrap_block_align("center"); }
                if icons::icon_button(ui, Icon::AlignRight, "Align Right").clicked() { self.wrap_block_align("right"); }
                if icons::icon_button(ui, Icon::AlignJustify, "Justify").clicked() { self.wrap_block_align("justify"); }

                ui.separator();

                // Lists & structure
                if icons::icon_button(ui, Icon::ListBullet, "Bullet List").clicked() { self.insert_text("- "); }
                if icons::icon_button(ui, Icon::ListNumber, "Numbered List").clicked() { self.insert_text("1. "); }
                if icons::icon_button(ui, Icon::Quote, "Blockquote").clicked() { self.insert_text("> "); }
                if icons::icon_button(ui, Icon::Code, "Code Block").clicked() { self.insert_text("```\n\n```\n"); }
                if icons::icon_button(ui, Icon::Rule, "Horizontal Rule").clicked() { self.insert_text("---\n"); }
                if icons::icon_button(ui, Icon::Table, "Insert Table").clicked() {
                    self.open_table_dialog();
                }
                if ui.small_button("SVG")
                    .on_hover_text("SVG editor (Code / Visual / Split)").clicked() {
                    self.open_svg_editor();
                }

                ui.separator();

                // Equations - gold to match brand identity
                if ui.add(btn(egui::RichText::new("∑").size(18.0).color(theme::ACCENT)))
                    .on_hover_text("Equation Block (Ctrl+E)").clicked() {
                    self.insert_text("$$\n\\sum_{i=0}^{n} x_i\n$$\n");
                }
                if ui.add(btn(egui::RichText::new("∑ᵢ").size(14.0).color(theme::ACCENT_HOVER)))
                    .on_hover_text("Inline Equation ($)").clicked() {
                    self.wrap_text("$", "$");
                }

                ui.separator();

                // Links / images / search
                if icons::icon_button(ui, Icon::Link, "Insert Link (Ctrl+K)").clicked() {
                    self.open_link_dialog(false);
                }
                if icons::icon_button(ui, Icon::Image, "Insert Image").clicked() {
                    self.open_link_dialog(true);
                }
                if icons::icon_button(ui, Icon::Search, "Find & Replace (Ctrl+H)").clicked() {
                    self.show_search = !self.show_search;
                }

                ui.separator();

                // ── Zoom - boutons +/- + ComboBox presets ────────────────────
                if ui.add(ibtn("−")).on_hover_text("Zoom Out (Ctrl+-)").clicked() {
                    self.zoom_level = (self.zoom_level - 0.1).max(0.3);
                }
                {
                    let zoom_pct = (self.zoom_level * 100.0).round() as u32;
                    let zoom_sel = egui::RichText::new(format!("{}%", zoom_pct)).size(12.0).color(theme::text_soft(dark));
                    egui::ComboBox::from_id_salt("zoom_pick")
                        .width(56.0)
                        .selected_text(zoom_sel)
                        .show_ui(ui, |ui| {
                            for (pct, val) in [(50u32,0.5f32),(75,0.75),(100,1.0),(125,1.25),(150,1.5),(175,1.75),(200,2.0),(250,2.5),(300,3.0)] {
                                if ui.selectable_label(
                                    zoom_pct == pct,
                                    egui::RichText::new(format!("{}%", pct)).size(12.5).color(theme::text_soft(dark)),
                                ).clicked() {
                                    self.zoom_level = val;
                                }
                            }
                        });
                }
                if ui.add(ibtn("+")).on_hover_text("Zoom In (Ctrl+=)").clicked() {
                    self.zoom_level = (self.zoom_level + 0.1).min(3.0);
                }

                ui.separator();

                // Font
                let prev_font = self.selected_font.clone();
                egui::ComboBox::from_id_salt("font_sel")
                    .width(130.0)
                    .selected_text(egui::RichText::new(&self.selected_font).size(13.0))
                    .show_ui(ui, |ui| {
                        for (name, _path) in &self.font_list {
                            if name == "---" {
                                ui.separator();
                            } else {
                                ui.selectable_value(&mut self.selected_font, name.clone(),
                                    egui::RichText::new(name.as_str()).size(13.0));
                            }
                        }
                    });
                if self.selected_font != prev_font { self.apply_font_change(ui.ctx()); }

                // ── Font size applied to the SELECTION (wraps it in a font-size
                // span), not the global base. Pick a size, or drag/type one, then
                // it applies to the selected text (or to what you type next).
                {
                    let std_sizes = [8.0f32, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0,
                                     20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];
                    let sz_label = egui::RichText::new(format!("{}", self.apply_size as u32))
                        .size(12.0).color(theme::text_soft(dark));
                    let mut apply: Option<f32> = None;
                    egui::ComboBox::from_id_salt("font_size_pick")
                        .width(44.0)
                        .selected_text(sz_label)
                        .show_ui(ui, |ui| {
                            for &sz in &std_sizes {
                                if ui.selectable_label(
                                    (self.apply_size - sz).abs() < 0.5,
                                    egui::RichText::new(format!("{}", sz as u32)).size(12.5).color(theme::text_soft(dark)),
                                ).clicked() {
                                    self.apply_size = sz;
                                    apply = Some(sz);
                                }
                            }
                        });
                    let dv = ui.add(
                        egui::DragValue::new(&mut self.apply_size)
                            .range(6.0..=96.0)
                            .speed(0.5)
                            .max_decimals(1)
                    ).on_hover_text("Drag or type a size, then it applies to the selection");
                    if dv.drag_stopped() || dv.lost_focus() { apply = Some(self.apply_size); }
                    ui.label(egui::RichText::new("pt").size(11.0).color(theme::text_faint(dark)));
                    if let Some(sz) = apply {
                        self.wrap_value_span("span", "font-size", &format!("{}pt", sz as u32));
                    }
                }
            });

            // ── Gold identity stripe at bottom of toolbar ────────────────────
            // This 2px line appears in every mode (Hub / Source / Split / Editor)
            // and anchors the brand identity throughout the application.
            let r = ui.max_rect();
            ui.painter().line_segment(
                [egui::pos2(r.left(), r.bottom() - 1.0), egui::pos2(r.right(), r.bottom() - 1.0)],
                egui::Stroke::new(2.0, theme::ACCENT),
            );
    }

    /// The minified WYSIWYG editing toolbar (default): the high-frequency tools
    /// with large, gold-on-hover icons, plus "Heading" and "More" overflow menus.
    /// Reads instantly as easy editing, in the spirit of Notion / Typora / Docs.
    fn toolbar_ui_minified(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark_mode;
        let wysiwyg = self.view_mode == ViewMode::Editor;
        let fmt = self.wysiwyg_fmt;
        // Responsive: fold secondary tools into "More" when the bar is narrow
        // (e.g. a slim Split pane) rather than letting them overflow.
        // When the pane is tight, only the core character formats stay on the bar;
        // the rest fold into "More" so the centered row never overflows.
        let roomy = ui.available_width() >= 620.0;
        ui.add_space(3.0);
        // The most-used controls grouped Word-style (Heading, Font, B/I/U/S, colour,
        // align, lists, link, equation); the rest fold into "More". Centering this
        // row over the page is a follow-up (egui auto-centering needs a stable pass).
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;

            // Paragraph style.
            ui.menu_button(egui::RichText::new("Heading").size(13.0).color(theme::text_soft(dark)), |ui| {
                if ui.button("Heading 1").clicked() { self.insert_text("# "); ui.close_menu(); }
                if ui.button("Heading 2").clicked() { self.insert_text("## "); ui.close_menu(); }
                if ui.button("Heading 3").clicked() { self.insert_text("### "); ui.close_menu(); }
            });
            if roomy {
                ui.separator();
                let prev_font = self.selected_font.clone();
                egui::ComboBox::from_id_salt("font_sel_m")
                    .width(110.0)
                    .selected_text(egui::RichText::new(&self.selected_font).size(12.5))
                    .show_ui(ui, |ui| {
                        for (name, _path) in &self.font_list {
                            if name == "---" {
                                ui.separator();
                            } else {
                                ui.selectable_value(&mut self.selected_font, name.clone(),
                                    egui::RichText::new(name.as_str()).size(12.5));
                            }
                        }
                    });
                if self.selected_font != prev_font { self.apply_font_change(ui.ctx()); }
            }

            ui.separator();
            // Character formats (the most-used cluster).
            if icons::lively_icon_toggle(ui, Icon::Bold, wysiwyg && fmt.bold, "Bold (Ctrl+B)").clicked() { self.toggle_inline_format(InlineFmt::Bold); }
            if icons::lively_icon_toggle(ui, Icon::Italic, wysiwyg && fmt.italic, "Italic (Ctrl+I)").clicked() { self.toggle_inline_format(InlineFmt::Italic); }
            if icons::lively_icon_toggle(ui, Icon::Underline, false, "Underline (Ctrl+U)").clicked() { self.toggle_inline_format(InlineFmt::Underline); }
            if icons::lively_icon_toggle(ui, Icon::Strikethrough, wysiwyg && fmt.strikethrough, "Strikethrough").clicked() { self.toggle_inline_format(InlineFmt::Strike); }
            self.spell_toggle_button(ui);

            if roomy {
                ui.separator();
                if let Some(rgb) = color_picker_combo(ui, "textcol_m", "A", self.text_color, false) {
                    self.text_color = rgb;
                    let hex = format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    self.wrap_value_span("span", "color", &format!("#{}", hex));
                }
                if let Some(rgb) = color_picker_combo(ui, "hlcol_m", "H", self.highlight_color, true) {
                    self.highlight_color = rgb;
                    let hex = format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    self.wrap_value_span("mark", "background", &format!("#{}", hex));
                }
                ui.separator();
                if icons::lively_icon_button(ui, Icon::AlignLeft, "Align left").clicked() { self.wrap_block_align("left"); }
                if icons::lively_icon_button(ui, Icon::AlignCenter, "Center").clicked() { self.wrap_block_align("center"); }
                if icons::lively_icon_button(ui, Icon::AlignRight, "Align right").clicked() { self.wrap_block_align("right"); }
                ui.separator();
                if icons::lively_icon_button(ui, Icon::ListBullet, "Bullet list").clicked() { self.insert_text("- "); }
                if icons::lively_icon_button(ui, Icon::ListNumber, "Numbered list").clicked() { self.insert_text("1. "); }
                if icons::lively_icon_button(ui, Icon::Link, "Link (Ctrl+K)").clicked() { self.open_link_dialog(false); }
                if icons::lively_icon_button(ui, Icon::Sigma, "Equation").clicked() { self.insert_text("$$\n\n$$\n"); }
            }

            ui.separator();
            // Everything else.
            ui.menu_button(egui::RichText::new("More").size(13.0).color(theme::text_soft(dark)), |ui| {
                if !roomy {
                    if ui.button("Align left").clicked() { self.wrap_block_align("left"); ui.close_menu(); }
                    if ui.button("Align center").clicked() { self.wrap_block_align("center"); ui.close_menu(); }
                    if ui.button("Align right").clicked() { self.wrap_block_align("right"); ui.close_menu(); }
                    if ui.button("Bullet list").clicked() { self.insert_text("- "); ui.close_menu(); }
                    if ui.button("Numbered list").clicked() { self.insert_text("1. "); ui.close_menu(); }
                    if ui.button("Link").clicked() { self.open_link_dialog(false); ui.close_menu(); }
                    if ui.button("Equation").clicked() { self.insert_text("$$\n\n$$\n"); ui.close_menu(); }
                    ui.separator();
                }
                if ui.button("Quote").clicked() { self.insert_text("> "); ui.close_menu(); }
                if ui.button("Inline code").clicked() { self.toggle_inline_format(InlineFmt::Code); ui.close_menu(); }
                if ui.button("Code block").clicked() { self.insert_text("```\n\n```\n"); ui.close_menu(); }
                if ui.button("Image...").clicked() { self.open_link_dialog(true); ui.close_menu(); }
                if ui.button("Table...").clicked() { self.open_table_dialog(); ui.close_menu(); }
                if ui.button("Horizontal rule").clicked() { self.insert_text("\n---\n"); ui.close_menu(); }
                ui.separator();
                if ui.button("Find & Replace (Ctrl+H)").clicked() { self.show_search = true; ui.close_menu(); }
            });
        });

        let r = ui.max_rect();
        ui.painter().line_segment(
            [egui::pos2(r.left(), r.bottom() - 1.0), egui::pos2(r.right(), r.bottom() - 1.0)],
            egui::Stroke::new(2.0, theme::ACCENT),
        );
    }
}

impl MdApp {
    /// Code-editor toolbar for the Source view: Markdown syntax insertion plus
    /// code utilities (line numbers, wrap, indent, comment, duplicate). Distinct
    /// from the WYSIWYG formatting toolbar used by the rendered Editor.
    pub(crate) fn show_source_toolbar(&mut self, ctx: &egui::Context) {
        let dark = self.dark_mode;
        egui::TopBottomPanel::top("source_toolbar")
            .min_height(56.0)
            .frame(egui::Frame::default()
                .fill(theme::panel_bg(dark))
                .inner_margin(egui::Margin { left: 6.0, right: 6.0, top: 3.0, bottom: 3.0 }))
            .show(ctx, |ui| self.source_toolbar_ui(ui));
    }

    /// The Source code-editor toolbar contents, drawn into `ui`. Top panel for
    /// the full-screen Source view, or embedded above the source pane in Split.
    pub(crate) fn source_toolbar_ui(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark_mode;
                let txt = |ui: &mut egui::Ui, label: &str, tip: &str| -> egui::Response {
                    ui.add(
                        egui::Button::new(egui::RichText::new(label).size(12.0).color(theme::text_soft(dark)))
                            .min_size(egui::vec2(30.0, 26.0))
                            .fill(theme::btn_fill(dark)),
                    )
                    .on_hover_text(tip)
                };

                // ── Row 1: Markdown syntax ──────────────────────────────────
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(egui::RichText::new("Markdown").size(11.0).color(theme::TEXT_MUTED));
                    ui.separator();
                    if icons::icon_button(ui, Icon::Bold, "Bold  **").clicked() { self.toggle_inline_format(InlineFmt::Bold); }
                    if icons::icon_button(ui, Icon::Italic, "Italic  *").clicked() { self.toggle_inline_format(InlineFmt::Italic); }
                    if icons::icon_button(ui, Icon::Code, "Inline code  `").clicked() { self.toggle_inline_format(InlineFmt::Code); }
                    if txt(ui, "H1", "Heading 1").clicked() { self.insert_text("# "); }
                    if txt(ui, "H2", "Heading 2").clicked() { self.insert_text("## "); }
                    if txt(ui, "H3", "Heading 3").clicked() { self.insert_text("### "); }
                    ui.separator();
                    if icons::icon_button(ui, Icon::Link, "Link  [](url)").clicked() { self.open_link_dialog(false); }
                    if icons::icon_button(ui, Icon::Image, "Image  ![](src)").clicked() { self.open_link_dialog(true); }
                    if icons::icon_button(ui, Icon::ListBullet, "Bullet list").clicked() { self.insert_text("- "); }
                    if icons::icon_button(ui, Icon::ListNumber, "Numbered list").clicked() { self.insert_text("1. "); }
                    if txt(ui, "[ ]", "Task list item").clicked() { self.insert_text("- [ ] "); }
                    if icons::icon_button(ui, Icon::Quote, "Blockquote").clicked() { self.insert_text("> "); }
                    ui.separator();
                    if icons::icon_button(ui, Icon::Table, "Insert table").clicked() {
                        self.open_table_dialog();
                    }
                    if txt(ui, "SVG", "SVG editor (Code / Visual / Split)").clicked() { self.open_svg_editor(); }
                    if icons::icon_button(ui, Icon::Rule, "Horizontal rule").clicked() { self.insert_text("\n---\n"); }
                    if icons::icon_button(ui, Icon::Sigma, "Equation block  $$").clicked() { self.insert_text("$$\n\n$$\n"); }
                    if txt(ui, "```", "Code block").clicked() { self.insert_text("```\n\n```\n"); }
                    if txt(ui, "[^]", "Footnote reference").clicked() { self.wrap_text("[^", "]"); }
                });

                ui.add_space(2.0);

                // ── Row 2: code-editor utilities ────────────────────────────
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(egui::RichText::new("Code").size(11.0).color(theme::TEXT_MUTED));
                    ui.separator();
                    if ui.selectable_label(self.source_line_numbers, egui::RichText::new("1,2,3").size(12.0))
                        .on_hover_text("Toggle line numbers").clicked()
                    { self.source_line_numbers = !self.source_line_numbers; }
                    if ui.selectable_label(self.source_wrap, egui::RichText::new("Wrap").size(12.0))
                        .on_hover_text("Soft-wrap long lines").clicked()
                    { self.source_wrap = !self.source_wrap; }
                    ui.separator();
                    if txt(ui, ">>", "Indent selection").clicked() { self.indent_selection(); }
                    if txt(ui, "<<", "Outdent selection").clicked() { self.outdent_selection(); }
                    if txt(ui, "<!--", "Toggle comment").clicked() { self.toggle_source_comment(); }
                    if txt(ui, "Dup", "Duplicate line").clicked() { self.duplicate_line(); }
                    ui.separator();
                    if icons::icon_button(ui, Icon::Search, "Find & Replace (Ctrl+H)").clicked() { self.show_search = !self.show_search; }
                });
    }

    pub(crate) fn show_search_bar(&mut self, ctx: &egui::Context) {
        if self.show_search {
            egui::TopBottomPanel::top("searchbar").show(ctx, |ui| {
                ui.add_space(4.0);
                let prev_query = self.search_query.clone();

                // ── Row 1: Find ───────────────────────────────────────────
                ui.horizontal(|ui| {
                    let (sr, _) = ui.allocate_exact_size(egui::vec2(22.0, 26.0), egui::Sense::hover());
                    icons::paint_icon(ui.painter(), Icon::Search, sr.shrink(5.0),
                        ui.visuals().widgets.inactive.fg_stroke.color);

                    // Multiline search field - grows with content, max ~4 lines visible.
                    // Enter inserts a newline (for multiline LaTeX blocks).
                    // Ctrl+Enter = find next.  Shift+Ctrl+Enter = find prev.
                    let search_resp = egui::ScrollArea::vertical()
                        .id_salt("search_scroll")
                        .max_height(80.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.search_query)
                                    .desired_width(240.0)
                                    .desired_rows(1)
                                    .hint_text("Find...  (Enter = newline, Ctrl+Enter = search)"),
                            )
                        })
                        .inner;

                    // Recompute whenever query changes
                    if self.search_query != prev_query {
                        self.search_match_idx = 0;
                        self.compute_search_matches();
                    }

                    // Ctrl+Enter → find next/prev (Enter is kept for newlines)
                    if search_resp.has_focus()
                        && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter))
                    {
                        if ui.input(|i| i.modifiers.shift) {
                            self.do_find_prev();
                        } else {
                            self.do_find_next();
                        }
                    }

                    // Match counter
                    let count = self.search_matches.len();
                    if !self.search_query.is_empty() {
                        let label = if count == 0 {
                            egui::RichText::new("No match")
                                .color(egui::Color32::from_rgb(200, 60, 60))
                                .size(12.0)
                        } else {
                            egui::RichText::new(format!("{}/{}", self.search_match_idx + 1, count))
                                .color(egui::Color32::GRAY)
                                .size(12.0)
                        };
                        ui.label(label);
                    }

                    // Prev / Next
                    if ui.add_sized([24.0, 26.0], egui::Button::new("◀"))
                        .on_hover_text("Previous match  Shift+F3").clicked()
                    {
                        self.do_find_prev();
                    }
                    if ui.add_sized([24.0, 26.0], egui::Button::new("▶"))
                        .on_hover_text("Next match  F3  or  Ctrl+Enter").clicked()
                    {
                        self.do_find_next();
                    }

                    // Case-sensitive toggle
                    let aa_color = if self.search_case_sensitive {
                        egui::Color32::from_rgb(60, 120, 220)
                    } else {
                        egui::Color32::GRAY
                    };
                    if ui.add_sized(
                        [28.0, 26.0],
                        egui::SelectableLabel::new(
                            self.search_case_sensitive,
                            egui::RichText::new("Aa").color(aa_color).size(12.0),
                        ),
                    )
                    .on_hover_text("Case sensitive")
                    .clicked()
                    {
                        self.search_case_sensitive = !self.search_case_sensitive;
                        self.search_match_idx = 0;
                        self.compute_search_matches();
                    }

                    // Toggle Replace row
                    let replace_icon = if self.search_show_replace { "⊟" } else { "⊞" };
                    if ui.add_sized([24.0, 26.0], egui::Button::new(replace_icon))
                        .on_hover_text(if self.search_show_replace {
                            "Hide replace"
                        } else {
                            "Show replace  Ctrl+H"
                        })
                        .clicked()
                    {
                        self.search_show_replace = !self.search_show_replace;
                    }

                    // Close
                    if icons::icon_button(ui, Icon::Close, "Close  Escape").clicked() {
                        self.show_search = false;
                    }
                });

                // ── Row 2: Replace (Ctrl+H mode only) ────────────────────
                if self.search_show_replace {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⟳")
                                .size(14.0)
                                .color(egui::Color32::GRAY),
                        );

                        // Multiline replace field - supports full LaTeX block replacement
                        egui::ScrollArea::vertical()
                            .id_salt("replace_scroll")
                            .max_height(80.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.replace_query)
                                        .desired_width(240.0)
                                        .desired_rows(1)
                                        .hint_text("Replace with...  (multiline supported)"),
                                );
                            });

                        let has_match = !self.search_matches.is_empty();
                        ui.vertical(|ui| {
                            ui.add_space(4.0);
                            if ui.add_enabled(
                                has_match,
                                egui::Button::new("Replace").min_size(egui::vec2(70.0, 24.0)),
                            )
                            .on_hover_text("Replace this occurrence  Ctrl+R")
                            .clicked()
                            {
                                self.do_replace_current();
                            }
                            if ui.add_enabled(
                                has_match,
                                egui::Button::new("Replace All").min_size(egui::vec2(70.0, 24.0)),
                            )
                            .on_hover_text("Replace all occurrences")
                            .clicked()
                            {
                                self.do_replace_all();
                            }
                        });
                    });
                }
                ui.add_space(4.0);
            });
        }
    }
}

impl MdApp {
    fn draw_view_switcher(&mut self, ui: &mut egui::Ui, dark: bool) {
        let view_btn = |label: &str, active: bool| {
            let (bg, txt) = if active {
                (theme::ACCENT, theme::TEXT)
            } else {
                (theme::btn_fill(dark), theme::text_soft(dark))
            };
            egui::Button::new(egui::RichText::new(label).size(12.0).color(txt)).fill(bg)
        };
        if ui.add_sized([44.0, 22.0], view_btn(&t("view.hub"), self.view_mode == ViewMode::Converter)).clicked() { self.view_mode = ViewMode::Converter; }
        if ui.add_sized([54.0, 22.0], view_btn(&t("view.source"), self.view_mode == ViewMode::Source)).clicked() { self.view_mode = ViewMode::Source; }
        if ui.add_sized([42.0, 22.0], view_btn(&t("view.split"), self.view_mode == ViewMode::Split)).clicked() { self.view_mode = ViewMode::Split; }
        if ui.add_sized([54.0, 22.0], view_btn(&t("view.editor"), self.view_mode == ViewMode::Editor)).clicked() { self.view_mode = ViewMode::Editor; self.segments_dirty = true; }
    }

    /// Top bar. On the converter home it stays hidden as a thin hint strip and
    /// only reveals the view switcher + menus when `revealed` (pointer near the
    /// top edge or a menu is open). In editor modes it is always the full bar.
    pub(crate) fn show_menu_bar(&mut self, ctx: &egui::Context, home: bool, revealed: bool) {
        let dark = self.dark_mode;
        if home && !revealed {
            egui::TopBottomPanel::top("menubar")
                .exact_height(26.0)
                .frame(egui::Frame::default()
                    .fill(theme::panel_bg(dark))
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{2261}  Menu").size(12.5).color(theme::text_soft(dark)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("\u{25BE}  hover for menus").size(11.0).color(theme::TEXT_MUTED));
                        });
                    });
                });
            return;
        }
        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Frameless menu-bar look without egui::menu::bar (which breaks the
                // right-aligned cluster's width). menu_button works in any Ui.
                ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                // Frameless inactive buttons so menu labels read like a classic menu
                // bar (the view switcher and Convert keep their explicit fills).
                ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                // File / Edit / Insert ... sit at the far left (classic menu bar).
                // The view switcher moved to its own framed bar just below.
                ui.menu_button(t("menu.file"), |ui| {
                    if ui.button("New          Ctrl+N").clicked() { self.do_new(); ui.close_menu(); }
                    if ui.button("Open         Ctrl+O").clicked() { self.do_open(); ui.close_menu(); }
                    if ui.button("Save         Ctrl+S").clicked() { self.do_save(); ui.close_menu(); }
                    if ui.button("Save As...   Ctrl+Shift+S").clicked() { self.do_save_as(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("\u{21BA} Import DOCX...").on_hover_text(
                        "Re-import a .docx exported by MD -> ALL.\n\
                         Recovers original markdown + LaTeX intact."
                    ).clicked() {
                        self.do_import_docx();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Export As...").clicked() { self.export_dialog.visible = true; ui.close_menu(); }
                    if ui.button("Quick PDF").clicked() { self.do_export_pdf(); ui.close_menu(); }
                    if ui.button("Quick HTML").clicked() { self.do_export_html(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Print        Ctrl+P").clicked() { self.do_print(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Exit").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.menu_button(t("menu.edit"), |ui| {
                    if ui.button("Undo            Ctrl+Z").clicked() { self.do_undo(); ui.close_menu(); }
                    if ui.button("Redo            Ctrl+Y").clicked() { self.do_redo(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Find & Replace   Ctrl+H").clicked() { self.show_search = !self.show_search; ui.close_menu(); }
                });
                ui.menu_button(t("menu.insert"), |ui| {
                    if ui.button("Heading 1").clicked() { self.insert_text("# "); ui.close_menu(); }
                    if ui.button("Heading 2").clicked() { self.insert_text("## "); ui.close_menu(); }
                    if ui.button("Heading 3").clicked() { self.insert_text("### "); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Bold              Ctrl+B").clicked() { self.toggle_inline_format(InlineFmt::Bold); ui.close_menu(); }
                    if ui.button("Italic            Ctrl+I").clicked() { self.toggle_inline_format(InlineFmt::Italic); ui.close_menu(); }
                    if ui.button("Underline         Ctrl+U").clicked() { self.toggle_inline_format(InlineFmt::Underline); ui.close_menu(); }
                    if ui.button("Strikethrough").clicked() { self.toggle_inline_format(InlineFmt::Strike); ui.close_menu(); }
                    if ui.button("Inline Code").clicked() { self.toggle_inline_format(InlineFmt::Code); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Code Block").clicked() { self.insert_text("```\n\n```\n"); ui.close_menu(); }
                    if ui.button("Equation Block    Ctrl+E").clicked() { self.insert_text("$$\n\\sum_{i=0}^{n} x_i\n$$\n"); ui.close_menu(); }
                    if ui.button("Inline Equation").clicked() { self.wrap_text("$", "$"); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Link...           Ctrl+K").clicked() {
                        self.link_dialog = LinkDialog { visible: true, text: String::new(), url: String::new(), is_image: false };
                        ui.close_menu();
                    }
                    if ui.button("Image...").clicked() {
                        self.link_dialog = LinkDialog { visible: true, text: String::new(), url: String::new(), is_image: true };
                        ui.close_menu();
                    }
                    if ui.button("Image from File...").clicked() { self.do_insert_image_file(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Table").clicked() { self.open_table_dialog(); ui.close_menu(); }
                    if ui.button("List Item").clicked() { self.insert_text("- "); ui.close_menu(); }
                    if ui.button("Blockquote").clicked() { self.insert_text("> "); ui.close_menu(); }
                    if ui.button("Horizontal Rule").clicked() { self.insert_text("---\n"); ui.close_menu(); }
                });
                if ui.button(t("menu.metadata")).clicked() { self.show_metadata = true; }
                if ui.button(t("menu.modules")).clicked() { self.module_open = true; }
                if ui.button("Settings").on_hover_text("Application options").clicked() {
                    self.options_open = true;
                }

                // Right cluster: Convert (a core promise, reachable everywhere), the
                // theme toggle, and the ABC spell-mode toggle. right-to-left in the
                // same row gets the full remaining width (status-bar pattern).
                // Push the cluster to the right edge, then render it directly
                // (left-to-right) rather than through a nested right-to-left layout,
                // which rendered nothing in this row.
                ui.add_space((ui.available_width() - 150.0).max(8.0));
                if ui.add(egui::Button::new(egui::RichText::new("Convert").strong().color(theme::TEXT))
                    .fill(theme::ACCENT_PALE)
                    .stroke(egui::Stroke::new(1.0, theme::ACCENT)))
                    .on_hover_text("Export to PDF, DOCX, HTML, and every supported format")
                    .clicked()
                {
                    self.export_dialog.visible = true;
                }
                ui.add_space(4.0);
                let (ti, tt) = if dark {
                    (Icon::Sun, "Switch to light theme")
                } else {
                    (Icon::Moon, "Switch to dark theme")
                };
                if icons::icon_button(ui, ti, tt).clicked() { self.dark_mode = !self.dark_mode; }
            });
        });

        // The view switcher gets its own framed strip just below the menu bar,
        // left-aligned so it sits at the left edge of the editing surface below.
        egui::TopBottomPanel::top("viewswitcher")
            .frame(egui::Frame::default()
                .fill(theme::panel_bg(dark))
                .inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 3.0, bottom: 3.0 }))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::default()
                        .fill(theme::btn_fill(dark))
                        .stroke(egui::Stroke::new(1.0, theme::BORDER))
                        .rounding(7.0)
                        .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            self.draw_view_switcher(ui, dark);
                        });
                });
            });
    }

    pub(crate) fn show_status_bar(&mut self, ctx: &egui::Context) {
        let dark = self.dark_mode;
        egui::TopBottomPanel::bottom("statusbar")
            .frame(egui::Frame::default()
                .fill(theme::surface_soft_c(dark))
                .inner_margin(egui::Margin::symmetric(8.0, 3.0)))
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let muted = theme::text_faint(dark);
                let dot = if self.modified {
                    egui::RichText::new("\u{25CF} ").color(theme::ACCENT).size(11.5)
                } else {
                    egui::RichText::new("").size(11.5)
                };
                ui.label(dot);
                ui.label(egui::RichText::new(&self.status_msg).size(11.5).color(theme::text_soft(dark)));
                ui.separator();
                if let Some(ref p) = self.current_file {
                    ui.label(egui::RichText::new(p.display().to_string()).size(11.5).color(muted));
                } else {
                    ui.label(egui::RichText::new("Untitled").size(11.5).color(muted));
                }
                // Reviewer-feedback toggle (only when an imported DOCX carried any).
                if !self.review_items.is_empty() {
                    ui.separator();
                    let on = self.show_review_panel;
                    if ui.selectable_label(on, egui::RichText::new(
                        format!("\u{1F4DD} {} ({})", t("panel.review"), self.review_items.len())
                    ).size(11.5)).clicked() {
                        self.show_review_panel = !on;
                    }
                }
                // Spelling toggle (only when a dictionary is loaded + enabled).
                if self.spell_enabled && self.spell.is_some() {
                    ui.separator();
                    let on = self.show_spelling_panel;
                    if ui.selectable_label(on, egui::RichText::new(
                        format!("\u{2713} {} ({})", t("panel.spelling"), self.spell_issues.len())
                    ).size(11.5)).clicked() {
                        self.show_spelling_panel = !on;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // cursor_pos is a CHAR index - count newlines among the first N
                    // chars (slicing source[..cursor_pos] as bytes panics on multibyte
                    // text, e.g. an accented title like "théorème").
                    let lines = self.source.chars().take(self.cursor_pos)
                        .filter(|&c| c == '\n').count() + 1;
                    let char_count = self.source.chars().count();
                    let words = mdall_core::stats::word_count(&self.source);
                    ui.label(egui::RichText::new(
                        format!("Ln {} | {} words | {} chars | {}%",
                            lines, words, char_count, (self.zoom_level * 100.0) as u32)
                    ).size(11.5).color(muted));
                    ui.separator();
                    ui.label(egui::RichText::new(match self.view_mode {
                        ViewMode::Converter => t("view.hub"),
                        ViewMode::Source    => t("view.source"),
                        ViewMode::Split     => t("view.split"),
                        ViewMode::Editor    => t("view.editor"),
                    }).size(11.5).color(muted));
                });
            });
        });
    }
}
