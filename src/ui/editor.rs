//! Editor views: welcome screen, raw source editor, WYSIWYG editor.
//! Methods on MdApp, extracted from main.rs.

use eframe::egui;
use crate::MdApp;
use crate::theme;
use crate::wysiwyg;
use crate::wysiwyg_map;
use crate::equation_layout;
use mdall_core::{editor, equation_renderer};
use crate::ui::state::{EditorMode, EquationEditor, LinkDialog};
use crate::{char_to_byte_index, byte_to_char_index, detect_format_at};

impl MdApp {
    pub(crate) fn show_welcome(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            // ── Logo - no duplicate text, the logo IS the title ──────────────
            if let Some(ref tex) = self.logo_tex {
                let ts    = tex.size_vec2();
                let max_h = 180.0;
                let scale = (max_h / ts.y).min(max_h / ts.x);
                let size  = egui::vec2(ts.x * scale, ts.y * scale);
                ui.image(egui::load::SizedTexture::new(tex.id(), size));
            }
            ui.add_space(20.0);
            // Tagline
            ui.label(egui::RichText::new("Write your equations once.  Export everywhere.  Recover everything.")
                .size(13.0).color(theme::TEXT_2));
            ui.add_space(32.0);
            // Gold separator
            {
                let w = (ui.available_width() * 0.3).min(200.0);
                let (r, _) = ui.allocate_exact_size(egui::vec2(w, 2.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 1.0, theme::ACCENT_PALE);
            }
            ui.add_space(20.0);
            ui.label(egui::RichText::new("File → New  or  File → Open  to get started")
                .size(12.0).color(theme::TEXT_MUTED));
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Or drop a file on the Conversion Hub  (⇄ Hub)")
                .size(11.5).color(theme::TEXT_MUTED));
        });
    }

    /// Paint the runic stave into the wood desktop margins on either side of the
    /// page, like the converter hub, whenever there is room for it. Fixed (does
    /// not scroll with the page) so it reads as an engraved border.
    fn paint_desktop_runes(&self, ui: &egui::Ui, page_w: f32) {
        let desk = ui.max_rect();
        let margin_w = (desk.width() - page_w) / 2.0;
        if margin_w < 76.0 {
            return; // too narrow for the stave
        }
        // Soft bronze a couple of shades up from the desktop wood (themes with it).
        let dk = theme::desktop_bg(self.dark_mode);
        let up = |c: u8, d: u8| c.saturating_add(d);
        let col = egui::Color32::from_rgb(up(dk.r(), 30), up(dk.g(), 26), up(dk.b(), 18));
        let stave_w = margin_w.min(150.0);
        let inset = egui::vec2(8.0, 56.0);
        let left = egui::Rect::from_min_max(
            desk.left_top(),
            egui::pos2(desk.left() + stave_w, desk.bottom()),
        ).shrink2(inset);
        let right = egui::Rect::from_min_max(
            egui::pos2(desk.right() - stave_w, desk.top()),
            desk.right_bottom(),
        ).shrink2(inset);
        crate::ui::hub::paint_runic_stave(ui.painter(), left, col, false);
        crate::ui::hub::paint_runic_stave(ui.painter(), right, col, true);
    }

    pub(crate) fn show_source_editor(&mut self, ui: &mut egui::Ui) {
        // ── Code-editor panel ────────────────────────────────────────────────
        // Flat full-width surface, monospace, Markdown syntax highlighting, and
        // an optional line-number gutter. This is the source-as-code view, a
        // deliberate counterpart to the rendered A4 page editor.
        let dark = self.dark_mode;
        // Light sand surface (brand parchment) for the code editor; warm dark in
        // dark mode. Keeps the code view tied to the Heimdall colour identity.
        let panel_bg = if dark {
            egui::Color32::from_rgb(0x22, 0x1E, 0x17)
        } else {
            egui::Color32::from_rgb(0xF3, 0xEA, 0xD5)
        };
        ui.painter().rect_filled(ui.max_rect(), 0.0, panel_bg);

        let font_size = self.font_size;
        let mono = egui::FontId::monospace(font_size);
        let line_count = self.source.lines().count().max(1);
        let digits = (line_count + 1).to_string().len().max(2);
        let char_w = ui.fonts(|f| f.glyph_width(&mono, '0'));
        let show_gutter = self.source_line_numbers;
        let gutter_w = if show_gutter { char_w * digits as f32 + 18.0 } else { 0.0 };
        let wrap = self.source_wrap;

        let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
            let mut job = crate::ui::source_highlight::highlight_markdown(text, font_size, dark);
            job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
            ui.fonts(|f| f.layout_job(job))
        };

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(gutter_w + 6.0);

                    let avail = ui.available_width();
                    let output = egui::TextEdit::multiline(&mut self.source)
                        .id(egui::Id::new("source_edit"))
                        .font(mono.clone())
                        .frame(false)
                        .desired_width(if wrap { avail } else { f32::INFINITY })
                        .desired_rows(40)
                        .layouter(&mut layouter)
                        .show(ui);

                    if self.request_source_focus {
                        output.response.request_focus();
                        self.request_source_focus = false;
                    }
                    if let Some((pos, anchor)) = self.pending_cursor.take() {
                        let primary = egui::text::CCursor::new(pos);
                        let secondary = egui::text::CCursor::new(anchor);
                        let mut state = output.state.clone();
                        state.cursor.set_char_range(Some(egui::text::CCursorRange { primary, secondary }));
                        state.store(ui.ctx(), egui::Id::new("source_edit"));
                    }
                    if output.response.changed() {
                        self.modified = true;
                        self.segments_dirty = true;
                        self.status_msg = "Modified".into();
                    }
                    if let Some(cr) = output.cursor_range {
                        self.cursor_pos = cr.primary.ccursor.index;
                        self.selection_anchor = cr.secondary.ccursor.index;
                    }

                    // Line-number gutter, aligned to the laid-out rows.
                    if show_gutter {
                        let origin = output.galley_pos;
                        let num_col = if dark {
                            egui::Color32::from_rgb(0x6A, 0x60, 0x50)
                        } else {
                            egui::Color32::from_rgb(0xB6, 0xA8, 0x95)
                        };
                        let gx = origin.x - 10.0;
                        let mut ln = 1usize;
                        let mut starts = true;
                        for row in output.galley.rows.iter() {
                            if starts {
                                ui.painter().text(
                                    egui::pos2(gx, origin.y + row.rect.top()),
                                    egui::Align2::RIGHT_TOP,
                                    ln.to_string(),
                                    mono.clone(),
                                    num_col,
                                );
                                ln += 1;
                            }
                            starts = row.ends_with_newline;
                        }
                    }
                });
                ui.add_space(8.0);
            });
    }

    /// Full-document WYSIWYG editor - used in Preview mode.
    ///
    /// A single `TextEdit::multiline` spans the whole document. The custom layouter
    /// (`build_document_layout_job`) renders headings large, bold/italic styled,
    /// equations as purple boxes, code monospace, etc.
    ///
    /// WYSIWYG editor - works like LibreOffice Writer / Word.
    /// Every text block is always directly editable (no click-to-activate).
    /// LaTeX equations are rendered as Typst PNG images; clicking one opens the
    /// equation editor dialog to modify the LaTeX source.
    /// Editor surface dispatcher - selects the rendering mode chosen in Options.
    /// Default is the segmented continuous flow; Block is a single live-preview surface.
    pub(crate) fn show_wysiwyg_editor(&mut self, ui: &mut egui::Ui) {
        match self.editor_mode {
            EditorMode::SegmentedFlow => self.show_editor_segmented(ui),
            EditorMode::Block => self.show_editor_block(ui),
        }
    }

    /// Block mode: the whole document as one continuous `TextEdit` with the
    /// WYSIWYG live-preview layouter (headings sized, emphasis styled, syntax
    /// faint). One cursor, type anywhere. Lower-fidelity than segmented flow
    /// (equations stay as `$$...$$` text, not rendered images) but robust and fast.
    fn show_editor_block(&mut self, ui: &mut egui::Ui) {
        const PAGE_W: f32 = 794.0;
        const MARGIN_Y: f32 = 56.0;
        const DESKTOP_PAD: f32 = 24.0;

        // Re-parse if the toolbar mutated the source this frame, so block ranges
        // match the source before slicing (avoids a mid-UTF-8 slice panic).
        if self.segments_dirty {
            self.blocks = editor::parse_document(&self.source);
        }

        ui.painter().rect_filled(ui.max_rect(), 0.0, theme::desktop_bg(self.dark_mode));
        let available_w = ui.available_width();
        let page_w = PAGE_W.min(available_w - 16.0);
        self.paint_desktop_runes(ui, page_w);
        let fs = self.font_size;

        self.show_ruler(ui, available_w, page_w);
        let (ml, mr) = (self.margin_left, self.margin_right);
        let content_w = (page_w - ml - mr).max(100.0);

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(DESKTOP_PAD);
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    let page_frame = egui::Frame::default()
                        .fill(egui::Color32::WHITE)
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(4.0, 6.0),
                            blur: 18.0,
                            spread: 0.0,
                            color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90),
                        })
                        .inner_margin(egui::Margin {
                            left: ml,
                            right: mr,
                            top: MARGIN_Y,
                            bottom: MARGIN_Y,
                        });

                    page_frame.show(ui, |ui| {
                        ui.set_width(content_w);

                        let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                            let job = wysiwyg::build_layout_job(
                                text,
                                wrap_width,
                                fs,
                                ui.visuals(),
                                wysiwyg::WysiwygTag::Paragraph,
                            );
                            ui.fonts(|f| f.layout_job(job))
                        };

                        let output = egui::TextEdit::multiline(&mut self.source)
                            .id(egui::Id::new("block_edit"))
                            .desired_width(f32::INFINITY)
                            .desired_rows(40)
                            .layouter(&mut layouter)
                            .show(ui);

                        if self.request_source_focus {
                            output.response.request_focus();
                            self.request_source_focus = false;
                        }
                        if let Some((pos, anchor)) = self.pending_cursor.take() {
                            let primary = egui::text::CCursor::new(pos);
                            let secondary = egui::text::CCursor::new(anchor);
                            let mut state = output.state.clone();
                            state.cursor.set_char_range(Some(egui::text::CCursorRange { primary, secondary }));
                            state.store(ui.ctx(), egui::Id::new("block_edit"));
                        }
                        if output.response.changed() {
                            self.modified = true;
                            self.segments_dirty = true;
                            self.status_msg = "Modified".into();
                        }
                        if let Some(cr) = output.cursor_range {
                            self.cursor_pos = cr.primary.ccursor.index;
                            self.selection_anchor = cr.secondary.ccursor.index;
                        }
                    });
                });
                ui.add_space(DESKTOP_PAD);
            });
    }

    fn show_editor_segmented(&mut self, ui: &mut egui::Ui) {
        const PAGE_W: f32    = 794.0;
        const PAGE_H: f32    = 1123.0;   // A4 height @ 96dpi (794 x 1123 px)
        const MARGIN_Y: f32  = 56.0;
        const PAGE_GAP: f32  = 26.0;     // visual gap between stacked sheets
        const DESKTOP_PAD: f32 = 24.0;

        // The formatting toolbar is rendered earlier this same frame and may have
        // just mutated the source (e.g. Bold wrapping the selection). Re-parse the
        // blocks now so their byte ranges match the current source before anything
        // slices it - a stale range can land inside a multi-byte character and
        // panic. The flag stays set so update() still runs the full rebuild
        // (spelling, macros, equation textures) on the next frame.
        if self.segments_dirty {
            self.blocks = editor::parse_document(&self.source);
        }

        // Plan B steps 3 + 7: document-level selection (Ctrl+A select-all and
        // cross-block drag-select), painted across blocks. egui's per-block TextEdit
        // selection cannot cross block boundaries, so the gesture is tracked here on
        // the app, using last frame's block-hit cache (layout is stable frame to
        // frame; the cache is cleared later in this frame's block loop). Consuming
        // Ctrl+A also stops the focused block running its own single-block select-all.
        {
            let (pressed, down, released, pos) = ui.input(|i| (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
            ));
            if pressed {
                // A fresh press clears any selection and arms a potential drag from
                // the pressed position. It is promoted to a real document selection
                // only once the pointer leaves the anchor block, so a plain click and
                // an in-block drag keep egui's native single-block behaviour.
                self.doc_selection = None;
                self.doc_drag_anchor = pos.and_then(|p| self.docpos_at(ui, p));
                self.doc_dragging = false;
            } else if down {
                if let (Some(anchor), Some(p)) = (self.doc_drag_anchor, pos) {
                    if let Some(head) = self.docpos_at(ui, p) {
                        if self.doc_dragging || head.block != anchor.block {
                            self.doc_dragging = true;
                            self.doc_selection =
                                Some(crate::doc_select::DocSelection { anchor, head });
                        }
                    }
                }
            } else if released {
                self.doc_drag_anchor = None;
                self.doc_dragging = false;
            }
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
            self.doc_selection = self.whole_doc_selection();
        }
        // Plan B step 8: Ctrl+C over a (non-caret) document selection copies the
        // visible text across blocks. The platform delivers copy as Event::Copy (not
        // a key), so match that; remove it from the queue so the focused block does
        // not then overwrite the clipboard with its own single-block copy.
        if self.doc_selection.map_or(false, |s| !s.is_caret())
            && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
        {
            if let Some(txt) = self.doc_selection_text() {
                ui.output_mut(|o| o.copied_text = txt);
            }
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Copy)));
        }

        // Plan B step 7b (the visual keystone): while a cross-block drag is active,
        // no per-block TextEdit may keep focus - a focused block would run its own
        // native single-block selection on top of the painted document selection.
        // Because the gesture above reads the GLOBAL pointer (not a widget that
        // captured the press), simply surrendering focus is enough to make every
        // block render inert for the duration; the default path is untouched (this
        // only fires while doc_dragging, which is false in all normal editing).
        if self.doc_dragging {
            if let Some(id) = ui.memory(|m| m.focused()) {
                ui.memory_mut(|m| m.surrender_focus(id));
            }
        }

        let desktop_color = theme::desktop_bg(self.dark_mode);
        ui.painter().rect_filled(ui.max_rect(), 0.0, desktop_color);

        let available_w = ui.available_width();
        let page_w      = PAGE_W.min(available_w - 16.0);
        self.paint_desktop_runes(ui, page_w);
        let fs          = self.font_size;

        // ── Pre-cache equation PNG textures (display + inline, Typst renderer) ──
        {
            // Collect all latex strings that need a texture: display equations + inline $...$
            let mut pending_latexes: Vec<String> = Vec::new();

            for b in &self.blocks {
                match &b.kind {
                    editor::BlockKind::DisplayEquation { latex, .. } => {
                        if !self.eq_tex_cache.contains_key(latex) {
                            pending_latexes.push(latex.clone());
                        }
                    }
                    // Inline $...$ no longer needs a Typst texture: it renders as its
                    // Unicode form inside the wrapping region (see map_block).
                    _ => {}
                }
            }
            pending_latexes.dedup();

            for (cache_idx, latex) in pending_latexes.iter().enumerate() {
                if self.eq_tex_cache.contains_key(latex) { continue; }
                // Expand custom macros + sanitize before rasterizing so Typst
                // does not choke on \label / \sket / spacing macros.
                let prepared = self.prepare_latex(latex);
                let (png_opt, err_opt) = equation_renderer::render_equation_png(&prepared, 2.0);
                if let Some(err) = err_opt {
                    self.status_msg = format!("Eq render error: {}", err);
                }
                if let Some(png) = png_opt {
                    if let Ok(img) = image::load_from_memory(&png) {
                        let rgba = img.into_rgba8();
                        let sz   = [rgba.width() as usize, rgba.height() as usize];
                        let color_img = egui::ColorImage::from_rgba_unmultiplied(sz, rgba.as_raw());
                        let handle = ui.ctx().load_texture(
                            format!("eq_pre_{}", cache_idx), color_img, egui::TextureOptions::LINEAR,
                        );
                        self.eq_tex_cache.insert(latex.clone(), handle);
                    }
                }
            }
        }

        // Snapshots - avoid borrow conflicts inside closures
        let blocks      = self.blocks.clone();
        let source_snap = self.source.clone();

        self.show_ruler(ui, available_w, page_w);
        let (ml, mr) = (self.margin_left, self.margin_right);
        let content_w = (page_w - ml - mr).max(100.0);

        // Deferred state changes collected during the block loop
        let mut pending_change: Option<(std::ops::Range<usize>, String)> = None;
        let mut block_op: Option<BlockOp> = None;
        // A keystroke typed in the trailing append region (collected in the loop,
        // materialized into the source after the ScrollArea like the others).
        let mut append_request: Option<String> = None;

        // Equation open requests - resolved after the ScrollArea
        #[allow(dead_code)]
        enum OpenEqReq {
            Display { latex: String, index: usize },
            Inline  { latex: String, block_range: std::ops::Range<usize>,
                      delim_open: String, delim_close: String,
                      /// Index of the clicked run inside the Vec<InlineRun> for this block.
                      run_idx: usize },
        }
        let mut open_eq: Option<OpenEqReq> = None;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            // Don't let the scroll area swallow drags - the block reorder grip needs them.
            .drag_to_scroll(false)
            .show(ui, |ui| {
            ui.add_space(DESKTOP_PAD);
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {

            // Print-layout: the editing surface is a stack of discrete A4 sheets, not
            // one ribbon that grows. The sheets (white, drop shadow, page number) are
            // PAINTED behind the content; the content flows across them via the
            // page-break spacers inserted in the block loop below. The frame itself
            // keeps only the page margins.
            let page_frame = egui::Frame::default()
                .inner_margin(egui::Margin {
                    left: ml, right: mr, top: MARGIN_Y, bottom: MARGIN_Y,
                });

            page_frame.show(ui, |ui| {
                ui.set_width(content_w);

                // Paint the A4 sheets behind the content. The page count comes from the
                // previous frame's measured heights and converges in one frame.
                let content_tl = ui.cursor().min;
                let sheet_x    = content_tl.x - ml;
                let origin_y   = content_tl.y - MARGIN_Y; // top of sheet 1
                let painted_pages = self.page_count.max(1);
                for k in 0..painted_pages {
                    let top  = origin_y + k as f32 * (PAGE_H + PAGE_GAP);
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(sheet_x, top), egui::vec2(page_w, PAGE_H));
                    let shadow = egui::epaint::Shadow {
                        offset: egui::vec2(4.0, 6.0), blur: 18.0, spread: 0.0,
                        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90),
                    };
                    ui.painter().add(shadow.as_shape(rect, egui::Rounding::same(2.0)));
                    let fill = egui::Color32::from_rgb(
                        self.page_color[0], self.page_color[1], self.page_color[2]);
                    ui.painter().rect_filled(rect, 2.0, fill);
                    if self.page_frame {
                        ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, theme::BORDER));
                    }
                    let foot = egui::FontId::proportional(11.0);
                    if self.show_page_numbers {
                        ui.painter().text(
                            egui::pos2(rect.center().x, rect.bottom() - MARGIN_Y * 0.5),
                            egui::Align2::CENTER_CENTER,
                            format!("{}", k + 1),
                            foot.clone(),
                            theme::TEXT_MUTED,
                        );
                    }
                    if !self.header_text.is_empty() {
                        ui.painter().text(
                            egui::pos2(rect.center().x, rect.top() + MARGIN_Y * 0.5),
                            egui::Align2::CENTER_CENTER,
                            self.header_text.as_str(),
                            foot.clone(),
                            theme::TEXT_MUTED,
                        );
                    }
                    if !self.footer_text.is_empty() {
                        ui.painter().text(
                            egui::pos2(rect.left() + ml, rect.bottom() - MARGIN_Y * 0.5),
                            egui::Align2::LEFT_CENTER,
                            self.footer_text.as_str(),
                            foot,
                            theme::TEXT_MUTED,
                        );
                    }
                }

                // Nearest neighbour top-level text region (paragraph/heading/quote,
                // skipping images/tables/etc.) for cross-block caret navigation.
                let focusable_neighbor = |from: usize, dir: i32| -> Option<egui::Id> {
                    let mut i = from as i32 + dir;
                    while i >= 0 && (i as usize) < blocks.len() {
                        let idx = i as usize;
                        let ok = match &blocks[idx].kind {
                            editor::BlockKind::Heading(_) | editor::BlockKind::BlockQuote => true,
                            editor::BlockKind::Paragraph => {
                                let r  = &blocks[idx].source_range;
                                let se = r.end.min(source_snap.len());
                                parse_standalone_image(&source_snap[r.start..se]).is_none()
                            }
                            _ => false,
                        };
                        if ok { return Some(egui::Id::new(("wysiwyg_block", idx))); }
                        i += dir;
                    }
                    None
                };

                // ── Block reorder (drag the left-margin grip) ──
                // Per-block vertical extent this frame, for drop-target hit-testing.
                let mut block_extents: Vec<(f32, f32)> = Vec::with_capacity(blocks.len());
                // Document-level hit-test cache (plan B): rebuilt fresh each frame.
                self.block_hits.clear();
                let mut drag_live: Option<(usize, f32)> = None;  // (from, pointer_y) while dragging
                let mut drag_drop: Option<(usize, f32)> = None;  // (from, pointer_y) on release
                // Image selection bookkeeping: deselect when a click lands elsewhere.
                let mut image_interacted = false;
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.selected_image = None;
                }

                for (block_idx, block) in blocks.iter().enumerate() {
                    let source_range = block.source_range.clone();
                    // Page-break spacer: if this block starts a new page (decided last
                    // frame from the block heights), jump the cursor to that page's
                    // content top so it lands on the next sheet, not in the gap.
                    if self.page_breaks.contains(&block_idx) {
                        let k = self.page_breaks.iter().filter(|&&b| b <= block_idx).count();
                        let target = origin_y + MARGIN_Y + k as f32 * (PAGE_H + PAGE_GAP);
                        let cur = ui.cursor().top();
                        if target > cur { ui.add_space(target - cur); }
                    }
                    let blk_y0 = ui.cursor().top();

                    match &block.kind {

                        // ── Display equation: Typst PNG, click → equation editor ──
                        editor::BlockKind::DisplayEquation { latex, index } => {
                            let idx = *index;
                            ui.add_space(8.0);
                            let frame = egui::Frame::default()
                                .fill(theme::EQ_BG)
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 190, 150)))
                                .rounding(6.0)
                                .inner_margin(egui::vec2(16.0, 10.0));
                            let tex_info = self.eq_tex_cache.get(latex).map(|t| {
                                let raw_sz = t.size_vec2();
                                (t.id(), egui::vec2(raw_sz.x * 0.5, raw_sz.y * 0.5))
                            });
                            // Macro-expanded + sanitized form for the fallback layout job.
                            let prepared_eq = self.prepare_latex(latex);
                            let clicked = frame.show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(egui::RichText::new(format!("Eq. {}", idx + 1))
                                        .size(10.0).color(egui::Color32::GRAY));
                                    ui.add_space(4.0);
                                    let resp = if let Some((tex_id, logical_size)) = tex_info {
                                        let max_w = (ui.available_width() - 4.0).max(1.0);
                                        let display_size = if logical_size.x > max_w {
                                            let ratio = max_w / logical_size.x;
                                            egui::vec2(max_w, logical_size.y * ratio)
                                        } else { logical_size };
                                        ui.add(egui::Image::new(
                                            egui::load::SizedTexture::new(tex_id, display_size)
                                        ).sense(egui::Sense::click()))
                                    } else {
                                        // Typst not available - fallback valign rendering
                                        let preview_job = equation_layout::latex_to_layout_job(
                                            &prepared_eq, fs + 4.0,
                                            (ui.available_width() - 8.0).max(1.0),
                                            ui.visuals().text_color(),
                                        );
                                        ui.add(egui::Label::new(preview_job).wrap().sense(egui::Sense::click()))
                                    };
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    resp.clicked()
                                }).inner
                            }).inner;
                            if clicked {
                                open_eq = Some(OpenEqReq::Display { latex: latex.clone(), index: idx });
                            }
                            ui.add_space(8.0);
                        }

                        // ── Horizontal rule ───────────────────────────────────────
                        editor::BlockKind::HorizontalRule => {
                            ui.add_space(8.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 1.0), egui::Sense::hover()
                            );
                            ui.painter().line_segment(
                                [egui::pos2(rect.left(), rect.center().y),
                                 egui::pos2(rect.right(), rect.center().y)],
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(178, 178, 190)),
                            );
                            ui.add_space(8.0);
                        }

                        // Paragraphs with inline $...$ are no longer special-cased:
                        // they fall through to the generic text-region arm below,
                        // which renders inline math as its Unicode form inside ONE
                        // wrapping, editable region (see wysiwyg_map::map_block).
                        // Clicking an inline equation opens the LaTeX editor
                        // (handled in render_rich_text_region_nav). Display $$...$$
                        // equations keep their Typst image (DisplayEquation arm).

                        // ── HTML comment block: hidden metadata (e.g. the
                        //    `<!-- mdall:latex-macros -->` preamble), render nothing ──
                        editor::BlockKind::HtmlBlock
                            if {
                                let se = source_range.end.min(source_snap.len());
                                source_snap[source_range.start..se].trim_start().starts_with("<!--")
                            } => {}

                        // ── Standalone image: rendered as a real image, not alt text ──
                        editor::BlockKind::Paragraph | editor::BlockKind::HtmlBlock
                            if {
                                let se = source_range.end.min(source_snap.len());
                                parse_standalone_image(&source_snap[source_range.start..se]).is_some()
                            } =>
                        {
                            let se  = source_range.end.min(source_snap.len());
                            let img = parse_standalone_image(&source_snap[source_range.start..se])
                                .expect("guard ensured standalone image");
                            // Replace only the image markup, not the trailing blank line
                            // an HtmlBlock range includes (keeps the block separator).
                            let content_len = source_snap[source_range.start..se].trim_end().len();
                            let replace_range = source_range.start..(source_range.start + content_len);
                            let uri = image_uri(&img.url, &self.current_file);
                            ui.add_space(6.0);
                            let selected = self.selected_image.as_ref() == Some(&replace_range);
                            let mut want_select = false;
                            let mut want_props  = false;
                            let mut want_align: Option<ImgAlign> = None;
                            let mut want_delete = false;
                            // Width committed during this frame's resize drag (px).
                            let mut resize_to: Option<u32> = None;
                            // Alignment of the figure within the page.
                            let layout = match img.align {
                                ImgAlign::Left  => egui::Layout::top_down(egui::Align::Min),
                                ImgAlign::Right => egui::Layout::top_down(egui::Align::Max),
                                _               => egui::Layout::top_down(egui::Align::Center),
                            };
                            let resize_id = egui::Id::new(("img-resize", block_idx));
                            ui.with_layout(layout, |ui| {
                                let avail = (ui.available_width() - 8.0).max(16.0);
                                // In-progress drag width takes precedence so the image
                                // scales live while a frame handle is dragged.
                                let drag_w: Option<f32> = ui.ctx().data(|d| d.get_temp(resize_id));
                                let shown_w = drag_w
                                    .or(img.width.map(|w| w as f32))
                                    .map_or(avail, |w| w.clamp(32.0, avail));
                                let resp = ui.add(
                                    egui::Image::new(uri)
                                        .max_width(shown_w)
                                        .show_loading_spinner(true)
                                        .sense(egui::Sense::click_and_drag()),
                                );
                                let rect = resp.rect;
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(if selected {
                                        egui::CursorIcon::Move
                                    } else {
                                        egui::CursorIcon::PointingHand
                                    });
                                }
                                // Single click → select (show frame); double click → properties.
                                if resp.clicked() { want_select = true; image_interacted = true; }
                                if resp.double_clicked() { want_props = true; image_interacted = true; }
                                // Drag the image BODY → move the block (reuses the reorder path).
                                if resp.dragged() {
                                    image_interacted = true;
                                    if let Some(pos) = resp.interact_pointer_pos() {
                                        drag_live = Some((block_idx, pos.y));
                                        ui.ctx().data_mut(|d| d.insert_temp(
                                            egui::Id::new("blk-drag-y"), pos.y));
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                    }
                                }
                                if resp.drag_stopped() {
                                    let py = resp.interact_pointer_pos().map(|p| p.y)
                                        .or_else(|| ui.ctx().data(|d| d.get_temp::<f32>(
                                            egui::Id::new("blk-drag-y"))))
                                        .or_else(|| ui.ctx().pointer_latest_pos().map(|p| p.y));
                                    if let Some(py) = py { drag_drop = Some((block_idx, py)); }
                                    ui.ctx().data_mut(|d| d.remove::<f32>(
                                        egui::Id::new("blk-drag-y")));
                                }

                                // ── Selection frame + edge/corner resize handles ──
                                if selected {
                                    ui.painter().rect_stroke(
                                        rect.expand(2.0), 2.0,
                                        egui::Stroke::new(1.5, theme::ACCENT));
                                    let hs = 9.0;
                                    // (handle center, anchor_left?) - drag adjusts WIDTH
                                    // (aspect-locked) from the opposite edge.
                                    let handles: [(egui::Pos2, bool); 8] = [
                                        (rect.left_top(),      false),
                                        (rect.center_top(),    true),
                                        (rect.right_top(),     true),
                                        (rect.right_center(),  true),
                                        (rect.right_bottom(),  true),
                                        (rect.center_bottom(), true),
                                        (rect.left_bottom(),   false),
                                        (rect.left_center(),   false),
                                    ];
                                    for (hi, (c, anchor_left)) in handles.iter().enumerate() {
                                        let hr = egui::Rect::from_center_size(
                                            *c, egui::vec2(hs, hs));
                                        let hresp = ui.interact(
                                            hr, resize_id.with(hi), egui::Sense::drag());
                                        ui.painter().rect_filled(hr, 1.5, theme::ACCENT);
                                        ui.painter().rect_stroke(hr, 1.5,
                                            egui::Stroke::new(1.0, egui::Color32::WHITE));
                                        if hresp.hovered() || hresp.dragged() {
                                            ui.ctx().set_cursor_icon(
                                                egui::CursorIcon::ResizeHorizontal);
                                        }
                                        if hresp.dragged() {
                                            image_interacted = true;
                                            if let Some(pos) = hresp.interact_pointer_pos() {
                                                let w = if *anchor_left {
                                                    pos.x - rect.left()
                                                } else {
                                                    rect.right() - pos.x
                                                };
                                                ui.ctx().data_mut(|d| d.insert_temp(
                                                    resize_id, w.clamp(32.0, avail)));
                                            }
                                        }
                                        if hresp.drag_stopped() {
                                            image_interacted = true;
                                            if let Some(w) = ui.ctx().data(
                                                |d| d.get_temp::<f32>(resize_id)) {
                                                resize_to = Some(w.round().max(16.0) as u32);
                                            }
                                            ui.ctx().data_mut(|d| d.remove::<f32>(resize_id));
                                        }
                                    }
                                }
                                // Right-click → image editing options.
                                resp.context_menu(|ui| {
                                    if ui.button("Properties...").clicked() {
                                        want_props = true; ui.close_menu();
                                    }
                                    ui.menu_button("Align", |ui| {
                                        if ui.button("Left").clicked() {
                                            want_align = Some(ImgAlign::Left); ui.close_menu();
                                        }
                                        if ui.button("Center").clicked() {
                                            want_align = Some(ImgAlign::Center); ui.close_menu();
                                        }
                                        if ui.button("Right").clicked() {
                                            want_align = Some(ImgAlign::Right); ui.close_menu();
                                        }
                                    });
                                    ui.separator();
                                    if ui.button("Delete image").clicked() {
                                        want_delete = true; ui.close_menu();
                                    }
                                });
                                if !img.alt.is_empty() {
                                    ui.add_space(2.0);
                                    ui.label(egui::RichText::new(&img.alt)
                                        .italics().size(fs * 0.85).color(theme::TEXT_MUTED));
                                }
                            });
                            if let Some(w) = resize_to {
                                if pending_change.is_none() {
                                    pending_change = Some((
                                        replace_range.clone(),
                                        serialize_image(&img.alt, &img.url, Some(w), img.align),
                                    ));
                                }
                            }
                            if want_props {
                                self.image_dialog = crate::ui::state::ImageDialog {
                                    visible: true,
                                    alt: img.alt.clone(),
                                    url: img.url.clone(),
                                    width: img.width.map(|w| w.to_string()).unwrap_or_default(),
                                    align: img.align,
                                    replace: replace_range.clone(),
                                };
                            } else if want_select {
                                self.selected_image = Some(replace_range.clone());
                            }
                            if let Some(al) = want_align {
                                if pending_change.is_none() {
                                    pending_change = Some((
                                        replace_range.clone(),
                                        serialize_image(&img.alt, &img.url, img.width, al),
                                    ));
                                }
                            }
                            if want_delete && pending_change.is_none() {
                                // Remove the image markup; the now-empty block is dropped on re-parse.
                                pending_change = Some((replace_range.clone(), String::new()));
                                self.selected_image = None;
                            }
                            ui.add_space(6.0);
                        }

                        // ── Frame: <div class="frame"> bordered box, interior editable ──
                        editor::BlockKind::HtmlBlock
                            if {
                                let se = source_range.end.min(source_snap.len());
                                parse_frame(&source_snap[source_range.start..se], 0).is_some()
                            } =>
                        {
                            let se = source_range.end.min(source_snap.len());
                            let (interior, range) =
                                parse_frame(&source_snap[source_range.start..se], source_range.start)
                                    .expect("guard ensured frame");
                            ui.add_space(4.0);
                            egui::Frame::default()
                                .fill(theme::SURFACE_SOFT)
                                .stroke(egui::Stroke::new(1.0, theme::ACCENT_PALE))
                                .rounding(6.0)
                                .inner_margin(egui::Margin::same(10.0))
                                .show(ui, |ui| {
                                    self.render_rich_text_region(
                                        ui, egui::Id::new(("frame", block_idx)),
                                        &interior, &range, fs,
                                        &blocks, &mut pending_change,
                                    );
                                });
                            ui.add_space(4.0);
                        }

                        // ── Styled habillage box: any <div style="..."> carrying a
                        //    background, border or text colour (callouts, theorem /
                        //    note boxes, colored containers). Rendered as a styled
                        //    frame; the interior stays fully editable, and the raw
                        //    <div> markup is still there in the Source view. ──
                        editor::BlockKind::HtmlBlock
                            if {
                                let se = source_range.end.min(source_snap.len());
                                parse_styled_div(&source_snap[source_range.start..se], 0).is_some()
                            } =>
                        {
                            let se = source_range.end.min(source_snap.len());
                            let (style, interior, range) =
                                parse_styled_div(&source_snap[source_range.start..se], source_range.start)
                                    .expect("guard ensured styled div");
                            ui.add_space(3.0);
                            let mut frame = egui::Frame::default()
                                .inner_margin(egui::Margin::same(10.0))
                                .rounding(6.0);
                            if let Some(fill) = style.fill { frame = frame.fill(fill); }
                            if let Some(stroke) = style.stroke { frame = frame.stroke(stroke); }
                            frame.show(ui, |ui| {
                                if let Some(c) = style.text_color {
                                    ui.visuals_mut().override_text_color = Some(c);
                                }
                                self.render_rich_text_region_aligned(
                                    ui, egui::Id::new(("styled_div", block_idx)),
                                    &interior, &range, fs, &blocks, &mut pending_change, style.align,
                                );
                            });
                            ui.add_space(3.0);
                        }

                        // ── Aligned block: <div style="text-align:..."> interior ──
                        editor::BlockKind::HtmlBlock
                            if {
                                let se = source_range.end.min(source_snap.len());
                                parse_aligned_div(&source_snap[source_range.start..se], 0).is_some()
                            } =>
                        {
                            let se = source_range.end.min(source_snap.len());
                            let (val, interior, range) =
                                parse_aligned_div(&source_snap[source_range.start..se], source_range.start)
                                    .expect("guard ensured aligned div");
                            let align = match val.as_str() {
                                "center" => egui::Align::Center,
                                "right"  => egui::Align::Max,
                                _        => egui::Align::Min, // left / justify
                            };
                            ui.add_space(2.0);
                            self.render_rich_text_region_aligned(
                                ui, egui::Id::new(("wysiwyg_block", block_idx)),
                                &interior, &range, fs, &blocks, &mut pending_change, align,
                            );
                            ui.add_space(2.0);
                        }

                        // ── Table: editable grid, no pipe markup shown (ADR-002 §8) ──
                        editor::BlockKind::Table => {
                            let safe_end  = source_range.end.min(source_snap.len());
                            let block_src = source_snap[source_range.start..safe_end].to_string();
                            let rows = parse_table(&block_src, source_range.start);
                            if rows.is_empty() {
                                // Malformed table: keep it editable via the legacy path.
                                let mut buf = block_src.clone();
                                let out = egui::TextEdit::multiline(&mut buf)
                                    .id(egui::Id::new(("table_raw", block_idx)))
                                    .font(egui::FontId::monospace(fs))
                                    .desired_width(f32::INFINITY)
                                    .frame(false)
                                    .show(ui);
                                if out.response.changed() && pending_change.is_none() {
                                    pending_change = Some((source_range.clone(), buf));
                                }
                            } else {
                                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
                                // Column alignment from the GFM delimiter row (:--/:-:/--:).
                                let aligns = TableModel::parse(&block_src)
                                    .map(|m| m.aligns).unwrap_or_default();
                                let col_halign = |c: usize| match aligns.get(c) {
                                    Some(ColAlign::Center) => egui::Align::Center,
                                    Some(ColAlign::Right)  => egui::Align::Max,
                                    _                      => egui::Align::Min,
                                };
                                let border = egui::Stroke::new(
                                    1.0, egui::Color32::from_rgb(200, 200, 210));
                                // Distribute the page width across columns so cells get a
                                // sensible width (without this, a Grid cell's available
                                // width is tiny and the wrapping layouter collapses each
                                // cell to ~1 character per line).
                                let cell_w = (content_w / cols.max(1) as f32 - 16.0).max(56.0);
                                ui.add_space(4.0);
                                egui::Frame::default()
                                    .stroke(border)
                                    .rounding(4.0)
                                    .show(ui, |ui| {
                                        egui::Grid::new(egui::Id::new(("table", block_idx)))
                                            .min_col_width(cell_w)
                                            .spacing(egui::vec2(0.0, 0.0))
                                            .show(ui, |ui| {
                                                for (r_idx, row) in rows.iter().enumerate() {
                                                    for c_idx in 0..cols {
                                                        let mut cell_frame = egui::Frame::none()
                                                            .stroke(border)
                                                            .inner_margin(egui::Margin::symmetric(6.0, 4.0));
                                                        if r_idx == 0 {
                                                            cell_frame = cell_frame.fill(theme::SURFACE_SOFT);
                                                        }
                                                        let cell_resp = cell_frame.show(ui, |ui| {
                                                            ui.set_width(cell_w);
                                                            if let Some(cell) = row.get(c_idx) {
                                                                self.render_rich_text_region_aligned(
                                                                    ui,
                                                                    egui::Id::new((
                                                                        "table_cell", block_idx, r_idx, c_idx,
                                                                    )),
                                                                    &cell.text, &cell.range, fs,
                                                                    &blocks, &mut pending_change,
                                                                    col_halign(c_idx),
                                                                );
                                                            } else {
                                                                ui.label(" ");
                                                            }
                                                        }).response;

                                                        // Right-click a cell: structural row/column edits.
                                                        cell_resp.context_menu(|ui| {
                                                            let mut op: Option<TableModel> = TableModel::parse(&block_src);
                                                            let mut act: Option<&str> = None;
                                                            if ui.button("Insert row below").clicked() { act = Some("rb"); ui.close_menu(); }
                                                            if ui.button("Insert row above").clicked() { act = Some("ra"); ui.close_menu(); }
                                                            if ui.button("Delete row").clicked() { act = Some("dr"); ui.close_menu(); }
                                                            ui.separator();
                                                            if ui.button("Insert column right").clicked() { act = Some("cr"); ui.close_menu(); }
                                                            if ui.button("Insert column left").clicked() { act = Some("cl"); ui.close_menu(); }
                                                            if ui.button("Delete column").clicked() { act = Some("dc"); ui.close_menu(); }
                                                            if let (Some(a), Some(m)) = (act, op.as_mut()) {
                                                                match a {
                                                                    "rb" => m.insert_row(r_idx),
                                                                    "ra" => m.rows.insert(r_idx.min(m.rows.len()), vec![String::new(); m.cols().max(1)]),
                                                                    "dr" => m.delete_row(r_idx),
                                                                    "cr" => m.insert_col(c_idx + 1),
                                                                    "cl" => m.insert_col(c_idx),
                                                                    "dc" => m.delete_col(c_idx),
                                                                    _ => {}
                                                                }
                                                                if pending_change.is_none() {
                                                                    pending_change = Some((source_range.clone(), m.to_source()));
                                                                }
                                                            }
                                                        });

                                                        // Tab in the last cell appends a new row and focuses it.
                                                        let is_last = r_idx + 1 == rows.len() && c_idx + 1 == cols;
                                                        if is_last && cell_resp.has_focus()
                                                            && ui.input(|i| i.key_pressed(egui::Key::Tab))
                                                            && pending_change.is_none()
                                                        {
                                                            if let Some(mut m) = TableModel::parse(&block_src) {
                                                                let new_row = m.rows.len();
                                                                m.insert_row(m.rows.len().saturating_sub(1));
                                                                pending_change = Some((source_range.clone(), m.to_source()));
                                                                self.region_focus_req = Some((
                                                                    egui::Id::new(("table_cell", block_idx, new_row, 0)),
                                                                    CaretAim::Start,
                                                                ));
                                                            }
                                                        }
                                                    }
                                                    ui.end_row();
                                                }
                                            });
                                    });
                                ui.add_space(4.0);
                            }
                        }

                        // ── Blockquote: indented quote with a gold bar, no "> " shown ──
                        editor::BlockKind::BlockQuote => {
                            let safe_end = source_range.end.min(source_snap.len());
                            let block_src = source_snap[source_range.start..safe_end].to_string();
                            ui.add_space(4.0);
                            let inner = egui::Frame::default()
                                .fill(theme::SURFACE_SOFT)
                                .rounding(4.0)
                                .inner_margin(egui::Margin { left: 14.0, right: 8.0, top: 6.0, bottom: 6.0 })
                                .show(ui, |ui| {
                                    self.render_rich_text_region_nav(
                                        ui, egui::Id::new(("wysiwyg_block", block_idx)),
                                        &block_src, &source_range, fs,
                                        &blocks, &mut pending_change,
                                        focusable_neighbor(block_idx, -1),
                                        focusable_neighbor(block_idx, 1),
                                        false, &mut block_op, f32::INFINITY, egui::Align::Min,
                                    )
                                });
                            // Gold accent bar in the left gutter = the quote marker.
                            let r = inner.response.rect;
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 3.0, r.bottom())),
                                egui::Rounding::ZERO,
                                theme::ACCENT,
                            );
                            ui.add_space(4.0);
                        }

                        // ── Bullet / ordered list: per-item, markers painted, ─────
                        //    content editable markup-free (ADR-002 lists step). The
                        //    "- " / "N. " markers live in the source but are NEVER
                        //    shown as text; each item's content is a reusable rich
                        //    region mapped to its own source sub-range.
                        editor::BlockKind::BulletList | editor::BlockKind::OrderedList => {
                            let safe_end  = source_range.end.min(source_snap.len());
                            let block_src = source_snap[source_range.start..safe_end].to_string();
                            let ordered   = matches!(block.kind, editor::BlockKind::OrderedList);
                            ui.add_space(2.0);

                            let mut line_start = 0usize; // byte offset within block_src
                            let mut item_idx   = 0usize;
                            for raw_line in block_src.split_inclusive('\n') {
                                let line        = raw_line.trim_end_matches('\n').trim_end_matches('\r');
                                let line_len    = line.len();
                                let indent_bytes = line.len() - line.trim_start().len();
                                let t           = &line[indent_bytes..];

                                // Detect the leading list marker and its byte length.
                                let (marker_disp, marker_len): (Option<String>, usize) =
                                    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
                                        (Some("•".to_string()), 2)
                                    } else if t == "-" || t == "*" || t == "+" {
                                        (Some("•".to_string()), t.len())
                                    } else if ordered {
                                        let digits: String =
                                            t.chars().take_while(|c| c.is_ascii_digit()).collect();
                                        let rest = &t[digits.len()..];
                                        if !digits.is_empty() && (rest.starts_with(". ") || rest.starts_with(") ")) {
                                            (Some(format!("{}.", digits)), digits.len() + 2)
                                        } else {
                                            (None, 0)
                                        }
                                    } else {
                                        (None, 0) // continuation / wrapped line
                                    };

                                // Source marker to prepend when this item is split
                                // (Enter) or pasted into several lines → new sibling.
                                let indent_ws = &line[..indent_bytes];
                                let marker_src = if ordered {
                                    let digits: String =
                                        t.chars().take_while(|c| c.is_ascii_digit()).collect();
                                    let n: u64 = digits.parse().unwrap_or(0);
                                    format!("{}{}. ", indent_ws, n + 1)
                                } else if t.starts_with("* ") || t == "*" {
                                    format!("{}* ", indent_ws)
                                } else if t.starts_with("+ ") || t == "+" {
                                    format!("{}+ ", indent_ws)
                                } else {
                                    format!("{}- ", indent_ws)
                                };

                                let content_start = line_start + indent_bytes + marker_len;
                                let content_end   = (line_start + line_len).max(content_start);
                                let content_src   = block_src[content_start..content_end].to_string();
                                let content_range = (source_range.start + content_start)
                                    ..(source_range.start + content_end);
                                let indent_px = (indent_bytes as f32) * 4.0;
                                let marker_gutter = 22.0;

                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    if indent_px > 0.0 { ui.add_space(indent_px); }
                                    match &marker_disp {
                                        Some(m) => {
                                            ui.add_sized(
                                                [marker_gutter, fs + 4.0],
                                                egui::Label::new(
                                                    egui::RichText::new(m).size(fs).color(theme::ACCENT),
                                                ),
                                            );
                                        }
                                        None => { ui.add_space(marker_gutter); }
                                    }
                                    self.render_rich_text_region(
                                        ui,
                                        egui::Id::new(("wysiwyg_list_item", block_idx, item_idx)),
                                        &content_src, &content_range, fs,
                                        &blocks, &mut pending_change,
                                    );
                                });

                                // Enter / multi-line paste inside an item → split into
                                // sibling items: prefix every line after the first with
                                // this item's marker so they parse as real list items
                                // (not unmarked continuation lines).
                                if let Some((rng, newtext)) = pending_change.clone() {
                                    if rng == content_range && newtext.contains('\n') {
                                        let rebuilt = newtext
                                            .split('\n')
                                            .enumerate()
                                            .map(|(i, part)| if i == 0 {
                                                part.to_string()
                                            } else {
                                                format!("{}{}", marker_src, part)
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        pending_change = Some((content_range.clone(), rebuilt));
                                    }
                                }

                                line_start += raw_line.len();
                                item_idx   += 1;
                            }
                            ui.add_space(2.0);
                        }

                        // ── All text blocks: always-active WYSIWYG TextEdit ───────
                        // ── Paragraph mixing text + inline image(s): flow on a wrapped line ──
                        editor::BlockKind::Paragraph
                            if {
                                let se = source_range.end.min(source_snap.len());
                                let s  = &source_snap[source_range.start..se];
                                parse_standalone_image(s).is_none()
                                    && parse_inline_image_segments(s, 0).is_some()
                            } =>
                        {
                            let se = source_range.end.min(source_snap.len());
                            let segs = parse_inline_image_segments(
                                &source_snap[source_range.start..se], source_range.start)
                                .expect("guard ensured inline segments");
                            ui.add_space(2.0);
                            let mut img_click: Option<(std::ops::Range<usize>, String, String)> = None;
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (si, seg) in segs.iter().enumerate() {
                                    match seg {
                                        InlineSeg::Text(r) => {
                                            let a = r.start.min(source_snap.len());
                                            let b = r.end.min(source_snap.len());
                                            let sub = &source_snap[a..b];
                                            if sub.is_empty() { continue; }
                                            // Size the editable text run to its content so it flows.
                                            let mb = self.mapped_block(sub);
                                            let cw = {
                                                let mut job = wysiwyg_map::render_buffer_job(
                                                    &mb.visible, &mb, fs, ui.visuals());
                                                job.wrap.max_width = ui.available_width().max(40.0);
                                                ui.fonts(|f| f.layout_job(job)).rect.width()
                                            };
                                            let mut noop = None;
                                            self.render_rich_text_region_nav(
                                                ui, egui::Id::new(("inlimg_txt", block_idx, si)),
                                                sub, r, fs, &blocks, &mut pending_change,
                                                None, None, false, &mut noop, cw + 3.0, egui::Align::Min,
                                            );
                                        }
                                        InlineSeg::Image { alt, url, range } => {
                                            let uri = image_uri(url, &self.current_file);
                                            let resp = ui.add(
                                                egui::Image::new(uri)
                                                    .max_height(fs * 3.0)
                                                    .max_width(ui.available_width().max(48.0))
                                                    .show_loading_spinner(true)
                                                    .sense(egui::Sense::click()),
                                            );
                                            if resp.hovered() {
                                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                            }
                                            if resp.clicked() {
                                                img_click = Some((range.clone(), alt.clone(), url.clone()));
                                            }
                                        }
                                    }
                                }
                            });
                            if let Some((range, alt, url)) = img_click {
                                self.image_dialog = crate::ui::state::ImageDialog {
                                    visible: true, alt, url,
                                    width: String::new(), align: ImgAlign::None, replace: range,
                                };
                            }
                            ui.add_space(2.0);
                        }

                        // ── Fenced code block: render the code body markup-free in a
                        //    monospace box; the ``` fences + language tag live ONLY in
                        //    the source (invariant), never shown in the editor. ──
                        editor::BlockKind::FencedCode { .. } => {
                            let safe_end = source_range.end.min(source_snap.len());
                            let block_src = source_snap[source_range.start..safe_end].to_string();
                            let (lang, code) = editor::code_block_content(&block_src);
                            // Byte offset of the code body in the source (just past the
                            // opening fence line), for caret mapping.
                            let body_off = source_range.start
                                + block_src.lines().next().map(|l| l.len() + 1).unwrap_or(0);

                            ui.add_space(4.0);
                            let mut changed_code: Option<String> = None;
                            egui::Frame::none()
                                .fill(ui.visuals().extreme_bg_color)
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .rounding(6.0)
                                .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
                                .show(ui, |ui| {
                                    if !lang.is_empty() {
                                        ui.label(egui::RichText::new(&lang).monospace().small().weak());
                                    }
                                    let mut buf = code.clone();
                                    let rows = buf.lines().count().max(1);
                                    let out = egui::TextEdit::multiline(&mut buf)
                                        .id(egui::Id::new(("wysiwyg_block", block_idx)))
                                        .font(egui::FontId::monospace(fs * 0.95))
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(rows)
                                        .frame(false)
                                        .show(ui);
                                    if let Some(ref cr) = out.cursor_range {
                                        let bb = char_to_byte_index(&buf, cr.primary.ccursor.index);
                                        let src_b = (body_off + bb).min(self.source.len());
                                        self.cursor_pos = byte_to_char_index(&self.source, src_b);
                                        self.selection_anchor = self.cursor_pos;
                                    }
                                    if out.response.changed() {
                                        changed_code = Some(buf.clone());
                                    }
                                });
                            if let Some(new_code) = changed_code {
                                if pending_change.is_none() {
                                    pending_change = Some((
                                        source_range.clone(),
                                        format!("```{}\n{}\n```", lang, new_code),
                                    ));
                                }
                            }
                            ui.add_space(4.0);
                        }

                        _ => {
                            let safe_end = source_range.end.min(source_snap.len());
                            let block_src = source_snap[source_range.start..safe_end].to_string();

                            // Heading spacing before
                            match &block.kind {
                                editor::BlockKind::Heading(1) => ui.add_space(10.0),
                                editor::BlockKind::Heading(2) => ui.add_space(6.0),
                                editor::BlockKind::Heading(_) => ui.add_space(4.0),
                                _ => ui.add_space(2.0),
                            }

                            let output = if matches!(block.kind,
                                editor::BlockKind::Paragraph | editor::BlockKind::Heading(_))
                            {
                                // ── ADR-002: true-WYSIWYG, no markup shown (reusable region) ──
                                // Headings render larger; their `#` stay hidden in the source.
                                let render_size = match block.kind {
                                    editor::BlockKind::Heading(1) => fs * 2.0,
                                    editor::BlockKind::Heading(2) => fs * 1.6,
                                    editor::BlockKind::Heading(3) => fs * 1.3,
                                    editor::BlockKind::Heading(4) => fs * 1.15,
                                    editor::BlockKind::Heading(_) => fs * 1.05,
                                    _ => fs,
                                };
                                self.render_rich_text_region_nav(
                                    ui, egui::Id::new(("wysiwyg_block", block_idx)),
                                    &block_src, &source_range, render_size,
                                    &blocks, &mut pending_change,
                                    focusable_neighbor(block_idx, -1),
                                    focusable_neighbor(block_idx, 1),
                                    true, &mut block_op, f32::INFINITY, egui::Align::Min,
                                )
                            } else {
                                // ── Legacy faint-markup path for lists/quotes/tables/code/etc. ──
                                let tag  = wysiwyg::WysiwygTag::from_block_kind(&block.kind);
                                let mut buf = block_src.clone();
                                let rows = buf.lines().count().max(1);
                                let out = egui::TextEdit::multiline(&mut buf)
                                    .id(egui::Id::new(("wysiwyg_block", block_idx)))
                                    .font(egui::FontId::proportional(fs))
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(rows)
                                    .frame(false)
                                    .layouter(&mut |ui, string, wrap| {
                                        let job = wysiwyg::build_layout_job(string, wrap, fs, ui.visuals(), tag);
                                        ui.fonts(|f| f.layout_job(job))
                                    })
                                    .show(ui);
                                if let Some(ref cr) = out.cursor_range {
                                    let char_idx   = cr.primary.ccursor.index;
                                    let anchor_idx = cr.secondary.ccursor.index;
                                    let byte_in_block  = char_to_byte_index(&buf, char_idx);
                                    let byte_in_source = source_range.start + byte_in_block;
                                    let block_char_start = byte_to_char_index(&self.source, source_range.start);
                                    self.cursor_pos       = block_char_start + char_idx;
                                    self.selection_anchor = block_char_start + anchor_idx;
                                    self.wysiwyg_fmt = detect_format_at(byte_in_source, &self.source, &blocks);
                                }
                                if out.response.changed() && pending_change.is_none() {
                                    pending_change = Some((source_range.clone(), buf.clone()));
                                }
                                out.response
                            };

                            // Heading spacing after
                            match &block.kind {
                                editor::BlockKind::Heading(1) => ui.add_space(6.0),
                                editor::BlockKind::Heading(2) => ui.add_space(4.0),
                                editor::BlockKind::Heading(_) => ui.add_space(2.0),
                                _ => {}
                            }

                            // Context menu on this block. Track the last real
                            // selection so "Add comment" survives the right-click
                            // collapsing the live selection (egui moves the caret
                            // on press). Cleared on a plain left click.
                            let has_sel = self.cursor_pos != self.selection_anchor;
                            if has_sel {
                                self.last_sel = Some((
                                    self.cursor_pos.min(self.selection_anchor),
                                    self.cursor_pos.max(self.selection_anchor),
                                ));
                            } else if output.clicked() {
                                self.last_sel = None;
                            }
                            let sel_for_comment = if has_sel {
                                Some((
                                    self.cursor_pos.min(self.selection_anchor),
                                    self.cursor_pos.max(self.selection_anchor),
                                ))
                            } else {
                                self.last_sel
                            };
                            output.context_menu(|ui| {
                                ui.label(egui::RichText::new("Clipboard").small().weak());
                                if ui.button("✂  Cut          Ctrl+X").clicked() {
                                    let (s, e) = (self.cursor_pos.min(self.selection_anchor),
                                                  self.cursor_pos.max(self.selection_anchor));
                                    let txt = self.source[
                                        char_to_byte_index(&self.source, s)
                                        ..char_to_byte_index(&self.source, e)].to_string();
                                    ui.output_mut(|o| o.copied_text = txt);
                                    self.insert_text("");
                                    ui.close_menu();
                                }
                                if ui.button("📋 Copy         Ctrl+C").clicked() {
                                    let (s, e) = (self.cursor_pos.min(self.selection_anchor),
                                                  self.cursor_pos.max(self.selection_anchor));
                                    let txt = self.source[
                                        char_to_byte_index(&self.source, s)
                                        ..char_to_byte_index(&self.source, e)].to_string();
                                    ui.output_mut(|o| o.copied_text = txt);
                                    ui.close_menu();
                                }
                                if ui.button("📌 Paste        Ctrl+V").clicked() { ui.close_menu(); }
                                if ui.button("Select All    Ctrl+A").clicked() {
                                    self.cursor_pos = self.source.chars().count();
                                    self.selection_anchor = 0;
                                    ui.close_menu();
                                }
                                if let Some((s, e)) = sel_for_comment {
                                    ui.separator();
                                    if ui.button("\u{1F4AC} Add comment").clicked() {
                                        let bs = char_to_byte_index(&self.source, s);
                                        let be = char_to_byte_index(&self.source, e);
                                        if bs < be && be <= self.source.len() {
                                            let anchor = self.source[bs..be].to_string();
                                            self.open_comment_dialog(anchor);
                                        }
                                        ui.close_menu();
                                    }
                                }
                                if has_sel {
                                    ui.separator();
                                    ui.label(egui::RichText::new("Format").small().weak());
                                    ui.menu_button("📝 Format \u{25b6}", |ui| {
                                        if ui.button("Bold           Ctrl+B").clicked() { self.wrap_text("**","**"); ui.close_menu(); }
                                        if ui.button("Italic         Ctrl+I").clicked() { self.wrap_text("*","*"); ui.close_menu(); }
                                        if ui.button("Underline      Ctrl+U").clicked() { self.wrap_text("<u>","</u>"); ui.close_menu(); }
                                        if ui.button("Strikethrough").clicked() { self.wrap_text("~~","~~"); ui.close_menu(); }
                                        if ui.button("Inline Code").clicked() { self.wrap_text("`","`"); ui.close_menu(); }
                                        if ui.button("Superscript").clicked() { self.wrap_text("<sup>","</sup>"); ui.close_menu(); }
                                        if ui.button("Subscript").clicked() { self.wrap_text("<sub>","</sub>"); ui.close_menu(); }
                                    });
                                    ui.menu_button("🎨 Typography \u{25b6}", |ui| {
                                        ui.menu_button("Font Size \u{25b6}", |ui| {
                                            for sz in [10u8, 11, 12, 14, 16, 18, 20, 24, 28, 36] {
                                                if ui.button(format!("{} pt", sz)).clicked() {
                                                    self.wrap_value_span("span", "font-size", &format!("{}pt", sz));
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                        ui.menu_button("Text Color \u{25b6}", |ui| {
                                            for (name, hex) in [
                                                ("Black","#000000"),("Dark grey","#444444"),("Grey","#888888"),
                                                ("Red","#e74c3c"),("Orange","#e67e22"),("Yellow","#f1c40f"),
                                                ("Green","#27ae60"),("Teal","#16a085"),("Blue","#2980b9"),
                                                ("Purple","#8e44ad"),
                                            ] {
                                                if ui.button(name).clicked() {
                                                    self.wrap_value_span("span", "color", hex);
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                        ui.menu_button("Highlight \u{25b6}", |ui| {
                                            for (name, hex) in [
                                                ("Yellow","#ffff00"),("Green","#90ee90"),("Cyan","#add8e6"),
                                                ("Pink","#ffb6c1"),("Orange","#ffa07a"),("Purple","#da70d6"),
                                            ] {
                                                if ui.button(name).clicked() {
                                                    self.wrap_value_span("mark", "background", hex);
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    });
                                    if ui.button("🔗 Hyperlink...   Ctrl+K").clicked() {
                                        let (s, e) = (self.cursor_pos.min(self.selection_anchor),
                                                      self.cursor_pos.max(self.selection_anchor));
                                        let sel_txt = self.source[
                                            char_to_byte_index(&self.source, s)
                                            ..char_to_byte_index(&self.source, e)].to_string();
                                        self.link_dialog = LinkDialog {
                                            visible: true, text: sel_txt,
                                            url: String::new(), is_image: false,
                                        };
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                ui.label(egui::RichText::new("Insert").small().weak());
                                ui.menu_button("+ Insert \u{25b6}", |ui| {
                                    if ui.button("🔗 Hyperlink...     Ctrl+K").clicked() {
                                        self.open_link_dialog(false);
                                        ui.close_menu();
                                    }
                                    if ui.button("🧮 Equation       Ctrl+E").clicked() { self.insert_text("$$\n\\sum_{i=0}^{n} x_i\n$$\n"); ui.close_menu(); }
                                    if ui.button("🖼 Image...").clicked() {
                                        self.open_link_dialog(true);
                                        ui.close_menu();
                                    }
                                    if ui.button("📊 Table").clicked() { self.insert_text("| Col 1 | Col 2 | Col 3 |\n|--------|--------|--------|\n|  |  |  |\n"); ui.close_menu(); }
                                    if ui.button("▭ Frame").clicked() { self.insert_text("<div class=\"frame\">\n\n\n\n</div>\n"); ui.close_menu(); }
                                    if ui.button("━ Horizontal Rule").clicked() { self.insert_text("---\n"); ui.close_menu(); }
                                });
                                ui.separator();
                                ui.label(egui::RichText::new("Paragraph").small().weak());
                                ui.menu_button("¶ Paragraph \u{25b6}", |ui| {
                                    for (label, prefix) in [("Heading 1","# "),("Heading 2","## "),("Heading 3","### "),
                                                             ("Heading 4","#### "),("Heading 5","##### "),("Heading 6","###### ")] {
                                        if ui.button(label).clicked() { self.insert_text(prefix); ui.close_menu(); }
                                    }
                                    ui.separator();
                                    if ui.button("• Bullet List").clicked() { self.insert_text("- "); ui.close_menu(); }
                                    if ui.button("1. Numbered List").clicked() { self.insert_text("1. "); ui.close_menu(); }
                                    if ui.button("❝ Blockquote").clicked() { self.insert_text("> "); ui.close_menu(); }
                                    if ui.button("``` Code Block").clicked() { self.insert_text("```\n\n```\n"); ui.close_menu(); }
                                    ui.separator();
                                    if ui.button("Align Left").clicked() { self.wrap_block_align("left"); ui.close_menu(); }
                                    if ui.button("Center").clicked() { self.wrap_block_align("center"); ui.close_menu(); }
                                    if ui.button("Align Right").clicked() { self.wrap_block_align("right"); ui.close_menu(); }
                                    if ui.button("Justify").clicked() { self.wrap_block_align("justify"); ui.close_menu(); }
                                });
                            }); // context_menu
                        }
                    } // match block.kind

                    // ── Reorder grip in the left margin (drag to move the block) ──
                    let blk_y1 = ui.cursor().top();
                    block_extents.push((blk_y0, blk_y1));
                    let cl = ui.min_rect().left();
                    // Plan B: record where this block sits, for document hit-testing.
                    self.block_hits.push(BlockHit {
                        idx: block_idx,
                        rect: egui::Rect::from_min_max(
                            egui::pos2(cl, blk_y0), egui::pos2(cl + content_w, blk_y1)),
                        range: block.source_range.clone(),
                        size: fs,
                    });
                    let grip_rect = egui::Rect::from_min_size(
                        egui::pos2(cl - 18.0, blk_y0 + 1.0),
                        egui::vec2(13.0, (blk_y1 - blk_y0 - 2.0).max(13.0)),
                    );
                    let grip = ui.interact(
                        grip_rect, egui::Id::new(("block-grip", block_idx)), egui::Sense::drag());
                    let block_hovered = ui.rect_contains_pointer(egui::Rect::from_min_max(
                        egui::pos2(cl - 20.0, blk_y0), egui::pos2(ui.min_rect().right(), blk_y1)));
                    if grip.hovered() || grip.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if block_hovered || grip.dragged() {
                        let c = if grip.hovered() || grip.dragged() { theme::ACCENT } else { theme::TEXT_MUTED };
                        let (cx, cy) = (grip_rect.center().x, grip_rect.center().y);
                        let p = ui.painter();
                        for dy in [-7.0_f32, 0.0, 7.0] {
                            for dx in [-2.5_f32, 2.5] {
                                p.circle_filled(egui::pos2(cx + dx, cy + dy), 1.4, c);
                            }
                        }
                    }
                    if grip.dragged() {
                        if let Some(pos) = grip.interact_pointer_pos() {
                            drag_live = Some((block_idx, pos.y));
                            // Remember the live position: on the release frame
                            // interact_pointer_pos() is None, so we need a fallback.
                            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("blk-drag-y"), pos.y));
                        }
                    }
                    if grip.drag_stopped() {
                        let py = grip.interact_pointer_pos().map(|p| p.y)
                            .or_else(|| ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("blk-drag-y"))))
                            .or_else(|| ui.ctx().pointer_latest_pos().map(|p| p.y));
                        if let Some(py) = py { drag_drop = Some((block_idx, py)); }
                        ui.ctx().data_mut(|d| d.remove::<f32>(egui::Id::new("blk-drag-y")));
                    }
                } // for blocks

                // ── Trailing "new paragraph" region (always present) ──────────────
                // Keeps the rendered page typable with zero blocks (a brand-new empty
                // document) or at the very end (adding a paragraph) without detouring
                // through the Source view (ADR-002 1bis). The first keystroke is
                // materialized into the source as a real paragraph and focus is handed
                // to that block, so writing flows naturally.
                {
                    ui.add_space(2.0);
                    let append_id = egui::Id::new("wysiwyg_append");
                    let append_resp = egui::TextEdit::multiline(&mut self.append_buf)
                        .id(append_id)
                        .font(egui::FontId::proportional(fs))
                        .desired_width(content_w)
                        .desired_rows(1)
                        .frame(false)
                        .hint_text(if blocks.is_empty() { "Start writing..." } else { "" })
                        .show(ui)
                        .response;
                    if append_resp.changed() && !self.append_buf.is_empty() {
                        append_request = Some(std::mem::take(&mut self.append_buf));
                    }
                }

                // ── Resolve the reorder drag: drop indicator while dragging, apply on release ──
                let target_for = |py: f32| -> usize {
                    for (i, (y0, y1)) in block_extents.iter().enumerate() {
                        if py < (y0 + y1) * 0.5 { return i; }
                    }
                    block_extents.len()
                };
                if let Some((_from, py)) = drag_live {
                    let to = target_for(py);
                    let y = block_extents.get(to).map(|(y0, _)| *y0 - 3.0)
                        .or_else(|| block_extents.last().map(|(_, y1)| *y1 + 3.0))
                        .unwrap_or(py);
                    ui.painter().hline(
                        ui.min_rect().left()..=ui.min_rect().right(), y,
                        egui::Stroke::new(2.5, theme::ACCENT));
                }
                if let Some((from, py)) = drag_drop {
                    let to = target_for(py);
                    // Skip no-op drops (back onto itself).
                    if to != from && to != from + 1 {
                        block_op = Some(BlockOp::Move { from, to });
                    }
                }

                // Deselect the image when a plain click lands outside any image.
                if ui.input(|i| i.pointer.primary_clicked()) && !image_interacted
                    && self.selected_image.is_some()
                {
                    self.selected_image = None;
                }

                // Plan B steps 3 + 7c: paint the document selection PRECISELY - the
                // exact glyph range in the partial first/last blocks, whole middle
                // blocks - as a translucent tint so the text still reads through. Each
                // touched block's galley is re-laid the same way it was rendered (left
                // origin; aligned blocks tint by row span, close enough).
                if let Some(sel) = self.doc_selection {
                    let col = egui::Color32::from_rgba_unmultiplied(90, 140, 235, 70);
                    for h in &self.block_hits {
                        if !sel.touches_block(h.idx) { continue; }
                        let se = h.range.end.min(self.source.len());
                        let ss = h.range.start.min(se);
                        let mb = wysiwyg_map::map_block(&self.source[ss..se]);
                        let (a_b, b_b) = match sel.range_in_block(h.idx, mb.visible.len()) {
                            Some(r) => r,
                            None => continue,
                        };
                        let a_c = byte_to_char_index(&mb.visible, a_b);
                        let b_c = byte_to_char_index(&mb.visible, b_b);
                        let mut job = wysiwyg_map::render_buffer_job(
                            &mb.visible, &mb, h.size, ui.visuals());
                        job.wrap.max_width = h.rect.width().max(1.0);
                        let galley = ui.fonts(|f| f.layout_job(job));
                        for r in selection_row_rects(&galley, a_c, b_c) {
                            ui.painter().rect_filled(r.translate(h.rect.min.to_vec2()), 1.0, col);
                        }
                    }
                }

                // ── Recompute pagination from THIS frame's measured block heights ──
                // (applied next frame: paints the right number of sheets and inserts
                // the page-break spacers). Heights are spacer-independent (y1 - y0),
                // so this converges immediately and stays stable.
                {
                    let content_h = (PAGE_H - 2.0 * MARGIN_Y).max(1.0);
                    let mut breaks: Vec<usize> = Vec::new();
                    let mut acc = 0.0_f32;
                    for (i, (y0, y1)) in block_extents.iter().enumerate() {
                        let h = (y1 - y0).max(0.0);
                        if i > 0 && acc + h > content_h {
                            // Keep-with-next: do not orphan a heading at the bottom of
                            // a page; break BEFORE it so it travels to the next page
                            // with this block.
                            let prev_heading = blocks.get(i - 1)
                                .map_or(false, |b| matches!(b.kind, editor::BlockKind::Heading(_)));
                            if prev_heading && breaks.last() != Some(&(i - 1)) {
                                let hp = (block_extents[i - 1].1 - block_extents[i - 1].0).max(0.0);
                                breaks.push(i - 1);
                                acc = hp + h + 12.0;
                            } else {
                                breaks.push(i);
                                acc = h + 6.0;
                            }
                        } else {
                            acc += h + 6.0; // approximate inter-block spacing
                        }
                    }
                    self.page_breaks = breaks;
                    self.page_count  = self.page_breaks.len() + 1;

                    // Pad the content down to the bottom of the last sheet so the
                    // ScrollArea covers every page and the last sheet is not clipped.
                    let last_bottom = origin_y + self.page_count as f32 * PAGE_H
                        + self.page_count.saturating_sub(1) as f32 * PAGE_GAP;
                    let cur = ui.cursor().top();
                    if last_bottom > cur { ui.add_space(last_bottom - cur); }
                }

            }); // page_frame
            }); // with_layout
            ui.add_space(DESKTOP_PAD);
        }); // ScrollArea

        // Apply pending text edit (done after ScrollArea to keep source_range valid)
        if let Some((range, new_text)) = pending_change {
            let safe_end = range.end.min(self.source.len());
            self.source.replace_range(range.start..safe_end, &new_text);
            self.modified        = true;
            self.segments_dirty  = true;
        }

        // Apply a pending block split/merge (Enter / Backspace at an edge).
        if let Some(op) = block_op {
            self.apply_block_op(op);
        }

        // Materialize a keystroke typed in the trailing append region into a new
        // paragraph, then hand focus (and the caret) to that real block so further
        // typing flows there instead of staying in the transient append buffer.
        if let Some(typed) = append_request {
            let trimmed = self.source
                .trim_end_matches(|c: char| c == '\n' || c == '\r')
                .to_string();
            self.source = if trimmed.is_empty() { typed } else { format!("{trimmed}\n\n{typed}") };
            self.modified       = true;
            self.segments_dirty = true;
            let nb = editor::parse_document(&self.source).len();
            if nb > 0 {
                let new_id = egui::Id::new(("wysiwyg_block", nb - 1));
                self.region_focus_req = Some((new_id, CaretAim::End));
                ui.ctx().memory_mut(|m| m.request_focus(new_id));
            }
        }

        // Open equation editor if an equation was clicked
        if let Some(req) = open_eq {
            match req {
                OpenEqReq::Display { latex, index } => {
                    self.eq_editor = EquationEditor {
                        visible: true,
                        inline_orig_latex: String::new(),
                inline_run_idx: 0,
                        latex,
                        index,
                        is_inline: false,
                        inline_block_range: 0..0,
                        inline_delim_open:  "$".into(),
                        inline_delim_close: "$".into(),
                    };
                }
                OpenEqReq::Inline { latex, block_range, delim_open, delim_close, run_idx } => {
                    self.eq_editor = EquationEditor {
                        visible: true,
                        inline_orig_latex: latex.clone(),
                        inline_run_idx: run_idx,
                        latex,
                        index: 0,
                        is_inline: true,
                        inline_block_range: block_range,
                        inline_delim_open:  delim_open,
                        inline_delim_close: delim_close,
                    };
                }
            }
        }

        // Inline equation clicked inside a wrapping region → open the LaTeX
        // editor on exactly its `$...$` (or `\(...\)`) source span.
        if !self.eq_editor.visible {
            if let Some(r) = self.pending_inline_eq.take() {
                let safe = r.end.min(self.source.len());
                let raw  = self.source[r.start..safe].to_string();
                let (open_d, close_d, inner) = if raw.starts_with("\\(") {
                    ("\\(".to_string(), "\\)".to_string(),
                     raw.trim_start_matches("\\(").trim_end_matches("\\)").trim().to_string())
                } else {
                    ("$".to_string(), "$".to_string(),
                     raw.trim_matches('$').trim().to_string())
                };
                self.eq_editor = EquationEditor {
                    visible: true,
                    latex: inner.clone(),
                    index: 0,
                    is_inline: true,
                    inline_block_range: r,
                    inline_delim_open: open_d,
                    inline_delim_close: close_d,
                    inline_orig_latex: inner,
                    inline_run_idx: 0,
                };
            }
        } else {
            self.pending_inline_eq = None;
        }
    }


}

impl MdApp {
    /// Reusable true-WYSIWYG rich-text region (ADR-002): renders `block_src` as
    /// MARKUP-FREE editable text, syncs edits back to the source preserving
    /// markup, and tracks the caret in source space (so the toolbar formats the
    /// right range). Reused for paragraphs/headings now, and (nested) for
    /// blockquotes / table cells / frames next. Returns the TextEdit response.
    /// Cached `map_block`: avoids re-parsing a region's source every frame.
    /// Keyed by a hash of the source and verified against the stored string
    /// (collision-safe); the cache is content-keyed so edits never read stale
    /// data, and bounded so it cannot grow without limit.
    fn mapped_block(&mut self, src: &str) -> wysiwyg_map::MappedBlock {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        src.hash(&mut h);
        let key = h.finish();
        if let Some((stored, mb)) = self.map_cache.get(&key) {
            if stored == src {
                return mb.clone();
            }
        }
        let mb = wysiwyg_map::map_block(src);
        if self.map_cache.len() > 512 {
            self.map_cache.clear();
        }
        self.map_cache.insert(key, (src.to_string(), mb.clone()));
        mb
    }

    fn render_rich_text_region(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        block_src: &str,
        src_range: &std::ops::Range<usize>,
        size: f32,
        blocks: &[editor::DocumentBlock],
        pending_change: &mut Option<(std::ops::Range<usize>, String)>,
    ) -> egui::Response {
        let mut noop = None;
        self.render_rich_text_region_nav(
            ui, id, block_src, src_range, size, blocks, pending_change,
            None, None, false, &mut noop, f32::INFINITY, egui::Align::Min,
        )
    }

    /// Aligned variant for centred / right-aligned content (`<div text-align>`,
    /// table columns).
    ///
    /// egui 0.29's `TextEdit` can only left-align (no per-line `halign`, and no
    /// `horizontal_align` builder until a later egui). So we split by focus:
    /// - NOT focused -> paint the galley directly, which DOES honour `job.halign`
    ///   (epaint centres / right-aligns every row around x=0); painting it at the
    ///   centre / right of the region gives true Word-style per-line alignment.
    ///   A click maps to the character under the cursor and focuses the region.
    /// - focused -> an editable `TextEdit`, block-shifted by a leading pad so the
    ///   text does not jump to the left edge while editing.
    ///
    /// `Align::Min` is just the normal left rendering.
    fn render_rich_text_region_aligned(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        block_src: &str,
        src_range: &std::ops::Range<usize>,
        size: f32,
        blocks: &[editor::DocumentBlock],
        pending_change: &mut Option<(std::ops::Range<usize>, String)>,
        align: egui::Align,
    ) -> egui::Response {
        if align == egui::Align::Min {
            return self.render_rich_text_region(ui, id, block_src, src_range, size, blocks, pending_change);
        }
        let avail = ui.available_width();
        let is_focused = ui.memory(|m| m.focused()) == Some(id);

        if is_focused {
            // Editing: keep the block visually shifted so clicking in does not snap
            // the text to the left margin. content_w is the widest laid-out line.
            let content_w = {
                let mb = self.mapped_block(block_src);
                let mut job = wysiwyg_map::render_buffer_job(&mb.visible, &mb, size, ui.visuals());
                job.wrap.max_width = avail;
                ui.fonts(|f| f.layout_job(job)).rect.width()
            };
            let pad = match align {
                egui::Align::Center => ((avail - content_w) * 0.5).max(0.0),
                egui::Align::Max => (avail - content_w).max(0.0),
                _ => 0.0,
            };
            let mut noop = None;
            return ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if pad > 1.0 { ui.add_space(pad); }
                self.render_rich_text_region_nav(
                    ui, id, block_src, src_range, size, blocks, pending_change,
                    None, None, false, &mut noop, (content_w + 3.0).min(avail), egui::Align::Min,
                )
            }).inner;
        }

        // Display: paint the galley with TRUE per-line alignment.
        let mb = self.mapped_block(block_src);
        let mut job = wysiwyg_map::render_buffer_job(&mb.visible, &mb, size, ui.visuals());
        job.wrap.max_width = avail;
        job.halign = align;
        let galley = ui.fonts(|f| f.layout_job(job));
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(avail, galley.size().y.max(size)), egui::Sense::click());
        // halign lays rows around x=0: Center -> [-w/2, w/2], Max -> [-w, 0]. Paint
        // the local origin at the centre / right edge so the rows land in the page.
        let origin = egui::pos2(
            match align {
                egui::Align::Center => rect.center().x,
                egui::Align::Max => rect.right(),
                _ => rect.left(),
            },
            rect.top(),
        );
        if resp.clicked() {
            let local = resp.interact_pointer_pos().unwrap_or(origin) - origin;
            let idx = galley.cursor_from_pos(local).ccursor.index;
            self.region_focus_req = Some((id, CaretAim::Index(idx)));
            ui.memory_mut(|m| m.request_focus(id));
        }
        let color = ui.visuals().override_text_color.unwrap_or_else(|| ui.visuals().text_color());
        ui.painter().galley(origin, galley, color);
        resp
    }

    /// Like `render_rich_text_region`, plus cross-block caret navigation: when the
    /// caret is at an edge and an arrow key cannot move it further, focus jumps to
    /// the `prev_id` / `next_id` neighbour region (ADR-002 step 7). Neighbours are
    /// `None` for nested regions (list items, table cells, frames) where intra-egui
    /// navigation already suffices.
    #[allow(clippy::too_many_arguments)]
    fn render_rich_text_region_nav(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        block_src: &str,
        src_range: &std::ops::Range<usize>,
        size: f32,
        blocks: &[editor::DocumentBlock],
        pending_change: &mut Option<(std::ops::Range<usize>, String)>,
        prev_id: Option<egui::Id>,
        next_id: Option<egui::Id>,
        block_split: bool,
        block_op: &mut Option<BlockOp>,
        desired_width: f32,
        halign: egui::Align,
    ) -> egui::Response {
        let mb = self.mapped_block(block_src);

        // Plan B step 7b (the keystone): while a cross-block drag is active, paint
        // every text region as a READ-ONLY galley instead of an interactive TextEdit.
        // A TextEdit that was the drag origin keeps re-grabbing focus (egui requests
        // focus on drag_started) and paints its own single-block native selection on
        // top of the document selection; rendering it inert removes both. The galley
        // is laid out exactly as the editable path lays it out, and the same vertical
        // space is allocated so pagination does not jump. Gated on doc_dragging, which
        // is false in all normal editing, so the default path is untouched.
        if self.doc_dragging {
            let avail = if desired_width.is_finite() { desired_width } else { ui.available_width() };
            let mut job = wysiwyg_map::render_buffer_job(&mb.visible, &mb, size, ui.visuals());
            job.wrap.max_width = avail;
            job.halign = halign;
            let galley = ui.fonts(|f| f.layout_job(job));
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(avail, galley.size().y.max(size)), egui::Sense::hover());
            let origin = egui::pos2(
                match halign {
                    egui::Align::Center => rect.center().x,
                    egui::Align::Max => rect.right(),
                    _ => rect.left(),
                },
                rect.top(),
            );
            let color = ui.visuals().override_text_color
                .unwrap_or_else(|| ui.visuals().text_color());
            ui.painter().galley(origin, galley, color);
            return resp;
        }

        // A pending cross-block jump targeting this region: focus it and place the
        // caret at the requested position before it lays out this frame.
        let focus_aim = match self.region_focus_req {
            Some((tid, aim)) if tid == id => { self.region_focus_req = None; Some(aim) }
            _ => None,
        };

        // Buffer: while THIS region is focused, keep egui's own continuously-edited
        // buffer (region_live) so fast typing isn't reset by per-frame re-derivation
        // from the (one-frame-lagging) source. Seed it from the visible text on first
        // focus or a cross-block jump; otherwise mirror the source.
        let is_focused = ui.memory(|m| m.focused()) == Some(id);
        let mut buf = if is_focused && focus_aim.is_none() {
            match &self.region_live {
                Some((lid, s)) if *lid == id => s.clone(),
                _ => mb.visible.clone(),
            }
        } else {
            mb.visible.clone()
        };
        let rows = buf.lines().count().max(1);

        // Block-structure keys (Enter to split, Backspace at start to merge) are
        // consumed BEFORE the TextEdit sees them, so they don't insert/delete text.
        let mut do_split = false;
        let mut do_merge = false;
        if block_split && ui.memory(|m| m.focused()) == Some(id) {
            let at_start = self.region_caret_prev == Some((id, 0));
            let caret_chars = self.region_caret_prev.filter(|(pid, _)| *pid == id).map(|(_, i)| i);
            let nchars_prev = buf.chars().count();
            ui.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. }
                        if !modifiers.shift => {
                        // Split only when there is text after the caret (avoids
                        // creating an unrepresentable empty Markdown paragraph).
                        if caret_chars.map_or(false, |c| c < nchars_prev) { do_split = true; return false; }
                        true
                    }
                    egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. }
                        if at_start && prev_id.is_some() => { do_merge = true; false }
                    _ => true,
                });
            });
        }

        let out = egui::TextEdit::multiline(&mut buf)
            .id(id)
            .font(egui::FontId::proportional(size))
            .desired_width(desired_width)
            .desired_rows(rows)
            .frame(false)
            .layouter(&mut |ui, string, wrap_width| {
                let mut job = wysiwyg_map::render_buffer_job(string, &mb, size, ui.visuals());
                // Wrap at the region's available width (egui passes it here); without
                // this the galley is infinitely wide and text never wraps / overflows
                // the page margins.
                job.wrap.max_width = wrap_width;
                // Per-line centre / right alignment is handled by the painted-galley
                // path in render_rich_text_region_aligned; here halign stays Left.
                job.halign = halign;
                ui.fonts(|f| f.layout_job(job))
            })
            .show(ui);

        if let Some(aim) = focus_aim {
            out.response.request_focus();
            let n = buf.chars().count();
            let idx = match aim {
                CaretAim::Start => 0,
                CaretAim::End => n,
                CaretAim::Index(i) => i.min(n),
            };
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx))));
            st.store(ui.ctx(), id);
        }

        // Sync the visible edit back into the source (deferred to keep ranges valid).
        if out.response.changed() && buf != mb.visible && pending_change.is_none() {
            *pending_change = Some((src_range.clone(), wysiwyg_map::sync_edit(block_src, &mb, &buf)));
        }
        // Keep the live buffer in lock-step with egui while focused. On blur, FLUSH
        // any in-flight visible edit to the source before dropping it (bug #2): a
        // per-frame keystroke that couldn't sync (because `pending_change` was
        // already taken that frame) lives only in `region_live`; dropping it blindly
        // when a toolbar/dialog steals focus loses the edit and the pre-edit source
        // reappears. We only drop the live buffer once it is actually in the source,
        // otherwise we keep it and retry next frame.
        if out.response.has_focus() {
            self.region_live = Some((id, buf.clone()));
        } else if let Some((lid, live)) = self.region_live.clone() {
            if lid == id {
                if live == mb.visible {
                    self.region_live = None; // already synced - safe to drop
                } else if pending_change.is_none() {
                    *pending_change =
                        Some((src_range.clone(), wysiwyg_map::sync_edit(block_src, &mb, &live)));
                    self.region_live = None; // flushed this frame
                }
                // else: couldn't flush (pending_change taken) - keep region_live and
                // flush on a later frame so the edit is never lost.
            }
        }
        // Caret → SOURCE positions via the index map.
        if let Some(ref cr) = out.cursor_range {
            let vis_b_p = char_to_byte_index(&buf, cr.primary.ccursor.index);
            let vis_b_a = char_to_byte_index(&buf, cr.secondary.ccursor.index);
            let src_b_p = src_range.start + mb.source_offset(vis_b_p);
            let src_b_a = src_range.start + mb.source_offset(vis_b_a);
            self.cursor_pos = byte_to_char_index(&self.source, src_b_p);
            self.selection_anchor = byte_to_char_index(&self.source, src_b_a);
            if self.cursor_pos != self.selection_anchor {
                // Remember the live selection so a toolbar format that steals focus
                // (collapsing the selection) can still wrap the intended text.
                self.last_sel = Some((
                    self.cursor_pos.min(self.selection_anchor),
                    self.cursor_pos.max(self.selection_anchor),
                ));
            }
            self.wysiwyg_fmt = detect_format_at(src_b_p, &self.source, blocks);

            // Click on an inline equation → request the LaTeX editor (resolved
            // after the loop). The equation is an atomic span in the galley.
            if out.response.clicked() {
                for sp in &mb.spans {
                    if sp.kind == wysiwyg_map::VisKind::Equation
                        && vis_b_p >= sp.vis.start && vis_b_p < sp.vis.end
                    {
                        self.pending_inline_eq =
                            Some((src_range.start + sp.src.start)..(src_range.start + sp.src.end));
                        break;
                    }
                }
            }

            // Record requested block split/merge (applied after the block loop).
            if do_split {
                *block_op = Some(BlockOp::Split { at: src_b_p });
            } else if do_merge {
                *block_op = Some(BlockOp::Merge { block_start: src_range.start });
            }

            // Cross-block caret: if an arrow key fired but the caret did not move
            // (egui clamped it at an edge), hand focus to the neighbour region.
            if out.response.has_focus() {
                let idx = cr.primary.ccursor.index;
                if prev_id.is_some() || next_id.is_some() {
                    let (up, down, left, right) = ui.input(|i| (
                        i.key_pressed(egui::Key::ArrowUp),
                        i.key_pressed(egui::Key::ArrowDown),
                        i.key_pressed(egui::Key::ArrowLeft),
                        i.key_pressed(egui::Key::ArrowRight),
                    ));
                    let row   = cr.primary.rcursor.row;
                    let nrows = out.galley.rows.len();
                    let nchars = buf.chars().count();
                    let prev_idx = self.region_caret_prev
                        .filter(|(pid, _)| *pid == id).map(|(_, i)| i);
                    let unmoved = prev_idx == Some(idx);
                    if (up || left) && unmoved && (row == 0 || idx == 0) {
                        if let Some(pid) = prev_id { self.region_focus_req = Some((pid, CaretAim::End)); }
                    } else if (down || right) && unmoved && (row + 1 >= nrows || idx == nchars) {
                        if let Some(nid) = next_id { self.region_focus_req = Some((nid, CaretAim::Start)); }
                    }
                }
                // Always track the caret so edge-key detection works even for a
                // lone block with no neighbours (e.g. Enter-to-split).
                self.region_caret_prev = Some((id, idx));
            }
        }

        // ── Spelling squiggles: a red wavy underline under each misspelled word
        //    that falls inside this region. Drawn only when the galley text equals
        //    the source-derived visible text (buf == mb.visible), so the byte/char
        //    offsets line up exactly with the laid-out glyphs. ──
        if self.spell_enabled && !self.spell_issues.is_empty() && buf == mb.visible {
            let red = egui::Color32::from_rgb(220, 50, 47);
            for issue in &self.spell_issues {
                if issue.start < src_range.start || issue.end > src_range.end {
                    continue;
                }
                let vb0 = mb.visible_offset(issue.start - src_range.start);
                let vb1 = mb.visible_offset(issue.end - src_range.start);
                if vb1 <= vb0 || vb1 > buf.len()
                    || !buf.is_char_boundary(vb0) || !buf.is_char_boundary(vb1)
                {
                    continue;
                }
                let c0 = buf[..vb0].chars().count();
                let c1 = buf[..vb1].chars().count();
                let r0 = out.galley.pos_from_ccursor(egui::text::CCursor::new(c0));
                let r1 = out.galley.pos_from_ccursor(egui::text::CCursor::new(c1));
                // Only the common single-row case (a wrapped word just isn't underlined).
                if (r0.top() - r1.top()).abs() < 1.0 {
                    let y = out.galley_pos.y + r0.bottom() - 1.0;
                    let x0 = out.galley_pos.x + r0.left();
                    let x1 = out.galley_pos.x + r1.left();
                    draw_wavy_underline(ui.painter(), x0, x1, y, red);
                }
            }
        }

        // ── Review anchor frame: frames the passage a Review item refers to.
        //    Hovering the item in the panel draws a subtle frame; clicking it
        //    draws a persistent (marked) frame. Only when offsets are exact. ──
        if buf == mb.visible && (self.review_hl.is_some() || self.review_mark.is_some()) {
            for (anchor_opt, strong) in [
                (self.review_hl.as_ref(), false),
                (self.review_mark.as_ref(), true),
            ] {
                let Some(text) = anchor_opt else { continue };
                if text.is_empty() {
                    continue;
                }
                let Some(astart) = self.source.find(text.as_str()) else { continue };
                let aend = astart + text.len();
                if aend <= src_range.start || astart >= src_range.end {
                    continue;
                }
                let ls = astart.saturating_sub(src_range.start);
                let le = (aend - src_range.start).min(block_src.len());
                let vb0 = mb.visible_offset(ls);
                let vb1 = mb.visible_offset(le);
                if vb1 <= vb0 || vb1 > buf.len()
                    || !buf.is_char_boundary(vb0) || !buf.is_char_boundary(vb1)
                {
                    continue;
                }
                let c0 = buf[..vb0].chars().count();
                let c1 = buf[..vb1].chars().count();
                let (fill, stroke) = if strong {
                    (
                        egui::Color32::from_rgba_unmultiplied(201, 146, 10, 42),
                        egui::Stroke::new(1.5, crate::theme::ACCENT),
                    )
                } else {
                    (
                        egui::Color32::from_rgba_unmultiplied(201, 146, 10, 22),
                        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(201, 146, 10, 130)),
                    )
                };
                for rect in anchor_row_rects(&out.galley, out.galley_pos, c0, c1) {
                    ui.painter().rect(rect.expand2(egui::vec2(1.0, 0.5)), 2.0, fill, stroke);
                }
            }
        }

        // ── Hyperlink affordances: hovering a link run shows its title/URL as a
        //    tooltip and a pointing-hand cursor; Ctrl+Click opens it. Plain click
        //    keeps editing (the caret lands inside the link text). The markup
        //    ([text](url "title")) lives only in the source - never shown. ──
        if let Some(hover) = out.response.hover_pos() {
            let vis_idx = out.galley.cursor_from_pos(hover - out.galley_pos).ccursor.index;
            let vis_b = char_to_byte_index(&buf, vis_idx);
            if let Some(lm) = mb.link_at(vis_b) {
                let tip = if lm.title.is_empty() {
                    lm.url.clone()
                } else {
                    format!("{}\n{}", lm.title, lm.url)
                };
                egui::show_tooltip_at_pointer(ui.ctx(), out.response.layer_id, id.with("linktip"), |ui| {
                    ui.label(tip);
                });
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if ui.input(|i| i.pointer.primary_clicked() && i.modifiers.ctrl) {
                    let _ = open::that(lm.url.clone());
                }
            }
        }
        out.response
    }

    /// Apply a block split (Enter) or merge (Backspace at start), then focus the
    /// resulting block with the caret at the natural junction.
    /// The selection covering the whole document (block 0 byte 0 .. last block's
    /// visible end), or None if empty. Used by Ctrl+A (plan B step 3).
    fn whole_doc_selection(&self) -> Option<crate::doc_select::DocSelection> {
        use crate::doc_select::{DocPos, DocSelection};
        let last = self.blocks.len().checked_sub(1)?;
        let lb = &self.blocks[last];
        let se = lb.source_range.end.min(self.source.len());
        let ss = lb.source_range.start.min(se);
        let vis_len = wysiwyg_map::map_block(&self.source[ss..se]).visible.len();
        Some(DocSelection { anchor: DocPos::new(0, 0), head: DocPos::new(last, vis_len) })
    }

    /// Plan B step 5: map a SCREEN position to a document position (block index +
    /// byte offset in that block's VISIBLE text). Picks the block via the per-frame
    /// `block_hits` cache, then lays out that block's visible text exactly as the
    /// renderer does (`render_buffer_job`, wrapped to the block width = `ui.set_width`
    /// content width) and asks the galley which character sits under the cursor -
    /// the same mapping the aligned-region click already uses. Pure given the cached
    /// hits; the keystone of click / drag hit-testing (consumed by B-6/B-7).
    #[allow(dead_code)]
    fn docpos_at(&self, ui: &egui::Ui, pos: egui::Pos2) -> Option<crate::doc_select::DocPos> {
        use crate::doc_select::DocPos;
        let bi = block_at_y(&self.block_hits, pos.y)?;
        let h = self.block_hits.iter().find(|h| h.idx == bi)?;
        let se = h.range.end.min(self.source.len());
        let ss = h.range.start.min(se);
        let mb = wysiwyg_map::map_block(&self.source[ss..se]);
        let mut job = wysiwyg_map::render_buffer_job(&mb.visible, &mb, h.size, ui.visuals());
        job.wrap.max_width = h.rect.width().max(1.0);
        let galley = ui.fonts(|f| f.layout_job(job));
        let local = pos - h.rect.min;
        let byte = char_to_byte_index(&mb.visible, galley.cursor_from_pos(local).ccursor.index);
        Some(DocPos::new(bi, byte))
    }

    /// Plan B step 8: the VISIBLE (rendered, markup-free) text of the current
    /// document selection, across blocks - what Ctrl+C copies. Each crossed block
    /// contributes its `range_in_block` slice of its visible text; blocks are joined
    /// by a blank line so pasted paragraphs stay separated. `None` for no selection
    /// or a collapsed caret.
    fn doc_selection_text(&self) -> Option<String> {
        let sel = self.doc_selection?;
        if sel.is_caret() {
            return None;
        }
        let (s, e) = sel.ordered();
        let mut parts: Vec<String> = Vec::new();
        for bi in s.block..=e.block {
            let blk = self.blocks.get(bi)?;
            let be_src = blk.source_range.end.min(self.source.len());
            let bs_src = blk.source_range.start.min(be_src);
            let vis = wysiwyg_map::map_block(&self.source[bs_src..be_src]).visible;
            if let Some((mut a, mut b)) = sel.range_in_block(bi, vis.len()) {
                // Defensive: the offsets came from this same map, but never slice
                // inside a multi-byte char.
                while a > 0 && !vis.is_char_boundary(a) { a -= 1; }
                b = b.min(vis.len());
                while b < vis.len() && !vis.is_char_boundary(b) { b += 1; }
                let a = a.min(b);
                parts.push(vis[a..b].to_string());
            }
        }
        Some(parts.join("\n\n"))
    }

    fn apply_block_op(&mut self, op: BlockOp) {
        match op {
            BlockOp::Split { at } => {
                let mut p = at.min(self.source.len());
                while p > 0 && !self.source.is_char_boundary(p) { p -= 1; }
                self.source.insert_str(p, "\n\n");
                self.modified = true;
                self.segments_dirty = true;
                let target = p + 2;
                let blocks = editor::parse_document(&self.source);
                if let Some((bi, _)) = blocks.iter().enumerate().find(|(_, b)| {
                    b.source_range.start <= target && target < b.source_range.end.max(b.source_range.start + 1)
                }) {
                    self.region_focus_req =
                        Some((egui::Id::new(("wysiwyg_block", bi)), CaretAim::Start));
                }
            }
            BlockOp::Merge { block_start } => {
                let bs = block_start.min(self.source.len());
                let gap_start = self.source[..bs]
                    .trim_end_matches(|c: char| c == '\n' || c == '\r' || c == ' ' || c == '\t')
                    .len();
                if gap_start == 0 || gap_start >= bs { return; }
                // Junction caret = visible length of the previous block's content.
                let pre = editor::parse_document(&self.source);
                let junction = pre.iter()
                    .rfind(|b| b.source_range.start < gap_start)
                    .map(|b| {
                        let end = gap_start.min(self.source.len());
                        let s = &self.source[b.source_range.start..end];
                        wysiwyg_map::map_block(s).visible.chars().count()
                    })
                    .unwrap_or(0);
                self.source.replace_range(gap_start..bs, "");
                self.modified = true;
                self.segments_dirty = true;
                let blocks = editor::parse_document(&self.source);
                if let Some((bi, _)) = blocks.iter().enumerate()
                    .rfind(|(_, b)| b.source_range.start <= gap_start.saturating_sub(1))
                {
                    self.region_focus_req =
                        Some((egui::Id::new(("wysiwyg_block", bi)), CaretAim::Index(junction)));
                }
            }
            BlockOp::Move { from, to } => {
                if let Some(s) = reorder_blocks_source(&self.source, from, to) {
                    self.source = s;
                    self.modified = true;
                    self.segments_dirty = true;
                }
            }
        }
    }
}

impl MdApp {
    /// Draws a LibreOffice-style graduated ruler above the page view.
    ///
    /// * `available_w` - full panel width (pixels)
    /// * `page_w`      - A4 page width used by the editor (pixels, may be capped)
    /// * `margin_x`    - left/right page margin (pixels)
    fn show_ruler(&mut self, ui: &mut egui::Ui, available_w: f32, page_w: f32) {
        const RULER_H: f32 = 22.0;
        // A4 is 210 mm wide; at 96 DPI that maps to ~794 px → 1 mm ≈ 3.78 px.
        let px_per_mm = page_w / 210.0_f32;

        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available_w, RULER_H),
            egui::Sense::hover(),
        );

        // Page occupies [page_left .. page_right] in screen x.
        let page_left  = rect.left() + (available_w - page_w) * 0.5;
        let page_right = page_left + page_w;

        // ── Draggable margin handles - update the page margins like Word/LO ──
        let min_content = 120.0_f32;
        let handle_size = egui::vec2(12.0, RULER_H);
        let lh_x = page_left + self.margin_left;
        let rh_x = page_right - self.margin_right;
        let lh = ui.interact(
            egui::Rect::from_center_size(egui::pos2(lh_x, rect.center().y), handle_size),
            ui.id().with("ruler_margin_l"), egui::Sense::drag());
        let rh = ui.interact(
            egui::Rect::from_center_size(egui::pos2(rh_x, rect.center().y), handle_size),
            ui.id().with("ruler_margin_r"), egui::Sense::drag());
        if lh.dragged() {
            self.margin_left = (self.margin_left + lh.drag_delta().x)
                .clamp(0.0, page_w - self.margin_right - min_content);
        }
        if rh.dragged() {
            self.margin_right = (self.margin_right - rh.drag_delta().x)
                .clamp(0.0, page_w - self.margin_left - min_content);
        }
        if lh.hovered() || lh.dragged() || rh.hovered() || rh.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        let painter = ui.painter_at(rect);

        // ── Background zones ──────────────────────────────────────────────
        let margin_color   = egui::Color32::from_rgb(210, 210, 218);
        let content_color  = egui::Color32::WHITE;
        let border_color   = egui::Color32::from_rgb(170, 170, 180);
        let tick_color     = egui::Color32::from_rgb(110, 110, 125);
        let label_color    = egui::Color32::from_rgb(75, 75, 90);
        let margin_line_color = egui::Color32::from_rgb(70, 130, 200);
        let handle_color   = egui::Color32::from_rgb(70, 130, 200);

        // Content boundaries from the live (possibly just-dragged) margins.
        let content_left  = page_left + self.margin_left;
        let content_right = page_right - self.margin_right;

        // Left margin zone (outside panel left → page left, if any)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top()), egui::pos2(page_left, rect.bottom())),
            0.0, margin_color,
        );
        // Left margin zone (page left → content left)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(page_left, rect.top()), egui::pos2(content_left, rect.bottom())),
            0.0, margin_color,
        );
        // Content zone (white)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(content_left, rect.top()), egui::pos2(content_right, rect.bottom())),
            0.0, content_color,
        );
        // Right margin zone (content right → page right)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(content_right, rect.top()), egui::pos2(page_right, rect.bottom())),
            0.0, margin_color,
        );
        // Right overhang (page right → panel right)
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(page_right, rect.top()), egui::pos2(rect.right(), rect.bottom())),
            0.0, margin_color,
        );

        // ── Tick marks over the page width (0 ... 210 mm) ──────────────────
        for mm in 0u32..=210 {
            let x = page_left + mm as f32 * px_per_mm;
            if x < rect.left() - 1.0 || x > rect.right() + 1.0 { continue; }

            let (tick_h, draw_label) = if mm % 20 == 0 {
                (12.0, true)   // every 2 cm - tallest + label
            } else if mm % 10 == 0 {
                (9.0, false)   // every 1 cm
            } else if mm % 5 == 0 {
                (6.0, false)   // every 5 mm
            } else {
                (3.5, false)   // every 1 mm
            };

            let y_top = rect.bottom() - tick_h;
            painter.line_segment(
                [egui::pos2(x, y_top), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, tick_color),
            );

            if draw_label && mm > 0 && mm < 210 {
                painter.text(
                    egui::pos2(x, rect.top() + 1.0),
                    egui::Align2::CENTER_TOP,
                    &format!("{}", mm / 10),   // cm number
                    egui::FontId::proportional(9.0),
                    label_color,
                );
            }
        }

        // ── Blue margin boundary lines + draggable handle markers ─────────
        for &bx in &[content_left, content_right] {
            painter.line_segment(
                [egui::pos2(bx, rect.top()), egui::pos2(bx, rect.bottom())],
                egui::Stroke::new(1.5, margin_line_color),
            );
            // Downward triangle marker so the boundary reads as draggable.
            let cy = rect.top() + 4.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(bx - 5.0, rect.top()),
                    egui::pos2(bx + 5.0, rect.top()),
                    egui::pos2(bx, cy + 4.0),
                ],
                handle_color,
                egui::Stroke::NONE,
            ));
        }

        // ── Bottom separator ──────────────────────────────────────────────
        painter.line_segment(
            [egui::pos2(rect.left(), rect.bottom()), egui::pos2(rect.right(), rect.bottom())],
            egui::Stroke::new(1.0, border_color),
        );
    }
}

// ── Cross-block caret / block editing targets (ADR-002 step 7) ────────────────

/// Where to place the caret when focus lands on a region next frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CaretAim {
    Start,
    End,
    Index(usize),
}

/// Reorder document blocks in the Markdown source: move the block at index
/// `from` so it lands at block index `to` (original ordering). Returns the new
/// source, or `None` if the move is a no-op / out of range. Pure + testable.
pub(crate) fn reorder_blocks_source(source: &str, from: usize, to: usize) -> Option<String> {
    let blocks = editor::parse_document(source);
    if from >= blocks.len() || from == to { return None; }
    let mut parts: Vec<String> = blocks.iter().map(|b| {
        let e = b.source_range.end.min(source.len());
        source[b.source_range.start..e].trim_end().to_string()
    }).collect();
    let moved = parts.remove(from);
    let insert_at = (if to > from { to - 1 } else { to }).min(parts.len());
    parts.insert(insert_at, moved);
    let mut s = parts.join("\n\n");
    s.push('\n');
    Some(s)
}

/// A structural block edit requested from a focused region, applied after the
/// block loop (so source ranges stay valid during rendering).
#[derive(Clone, Copy, Debug)]
pub(crate) enum BlockOp {
    /// Split the block: insert a paragraph break at this source byte offset.
    Split { at: usize },
    /// Merge with the previous block: drop the whitespace gap before this offset.
    Merge { block_start: usize },
    /// Reorder: move the block at `from` (block index) so it lands at position
    /// `to` (block index in the ORIGINAL ordering; the insertion is adjusted for
    /// the removal). Used by drag-to-reorder.
    Move { from: usize, to: usize },
}

/// Per-frame cache of where each rendered block sits on screen and which source
/// bytes it spans, for document-level hit-testing (plan B). Populated during the
/// segmented render; read by the document selection in later increments.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct BlockHit {
    pub idx: usize,
    pub rect: egui::Rect,
    pub range: std::ops::Range<usize>,
    pub size: f32,
}

/// The block index (`BlockHit::idx`) whose vertical span contains `y`, or the
/// nearest block if `y` falls in an inter-block gap / margin. Pure given the cached
/// hits; the keystone of click / drag hit-testing.
#[allow(dead_code)]
pub(crate) fn block_at_y(hits: &[BlockHit], y: f32) -> Option<usize> {
    if hits.is_empty() {
        return None;
    }
    for h in hits {
        if y >= h.rect.top() && y <= h.rect.bottom() {
            return Some(h.idx);
        }
    }
    hits.iter()
        .min_by(|a, b| {
            let da = (y - a.rect.center().y).abs();
            let db = (y - b.rect.center().y).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|h| h.idx)
}

/// The rectangles (relative to the galley origin) covering the glyph range
/// `[char_start, char_end)` across a galley's wrapped rows - used to paint a precise
/// text selection inside a block (plan B step 7c). One rect per touched row; empty
/// for an empty range. Coordinates are galley-local, so the caller offsets by the
/// block's on-screen origin.
#[allow(dead_code)]
pub(crate) fn selection_row_rects(
    galley: &egui::Galley, char_start: usize, char_end: usize,
) -> Vec<egui::Rect> {
    let mut rects = Vec::new();
    if char_start >= char_end {
        return rects;
    }
    let mut idx = 0usize;
    for row in &galley.rows {
        let g = &row.glyphs;
        let rc = g.len();
        let (rs, re) = (idx, idx + rc);
        let a = char_start.max(rs);
        let b = char_end.min(re);
        if a < b {
            let x_at = |c: usize| -> f32 {
                if c < rc { g[c].pos.x }
                else if let Some(last) = g.last() { last.pos.x + last.size().x }
                else { 0.0 }
            };
            rects.push(egui::Rect::from_min_max(
                egui::pos2(x_at(a - rs), row.min_y()),
                egui::pos2(x_at(b - rs), row.max_y()),
            ));
        }
        idx = re + if row.ends_with_newline { 1 } else { 0 };
    }
    rects
}

#[cfg(test)]
mod block_hit_tests {
    use super::{block_at_y, BlockHit};
    use eframe::egui;

    fn hit(idx: usize, top: f32, bottom: f32) -> BlockHit {
        BlockHit {
            idx,
            rect: egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(100.0, bottom)),
            range: 0..0,
            size: 16.0,
        }
    }

    #[test]
    fn inside_a_block() {
        let hits = vec![hit(0, 0.0, 20.0), hit(1, 30.0, 50.0)];
        assert_eq!(block_at_y(&hits, 10.0), Some(0));
        assert_eq!(block_at_y(&hits, 40.0), Some(1));
    }

    #[test]
    fn gap_picks_nearest() {
        let hits = vec![hit(0, 0.0, 20.0), hit(1, 60.0, 80.0)];
        assert_eq!(block_at_y(&hits, 24.0), Some(0));
        assert_eq!(block_at_y(&hits, 56.0), Some(1));
    }

    #[test]
    fn above_first_below_last() {
        let hits = vec![hit(0, 10.0, 20.0), hit(2, 40.0, 50.0)];
        assert_eq!(block_at_y(&hits, -5.0), Some(0));
        assert_eq!(block_at_y(&hits, 999.0), Some(2));
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(block_at_y(&[], 5.0), None);
    }
}

// ── Standalone image blocks (ADR-002 step 6) ──────────────────────────────────

/// Horizontal placement of a figure. `None` keeps Markdown's default flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ImgAlign {
    None,
    Left,
    Center,
    Right,
}

/// A block whose entire content is a single image. Rendered as a real image in
/// the WYSIWYG view. `width`/`align` carry geometry that Markdown cannot express
/// (round-tripped through HTML `<img>`/`<figure>`).
pub(crate) struct StandaloneImage {
    pub alt: String,
    pub url: String,
    pub width: Option<u32>,
    pub align: ImgAlign,
}

/// Read an HTML attribute value (`name="..."` or `name='...'`), case-insensitive
/// on the name. Returns the raw inner string.
/// Screen rects (one per wrapped row) covering visible chars `[c0, c1]` in `galley`.
fn anchor_row_rects(galley: &egui::Galley, galley_pos: egui::Pos2, c0: usize, c1: usize) -> Vec<egui::Rect> {
    let mut out = Vec::new();
    if c1 <= c0 {
        return out;
    }
    let mut cur: Option<egui::Rect> = None;
    for c in c0..=c1 {
        let r = galley
            .pos_from_ccursor(egui::text::CCursor::new(c))
            .translate(galley_pos.to_vec2());
        match cur {
            Some(acc) if (acc.top() - r.top()).abs() < 1.0 => cur = Some(acc.union(r)),
            Some(acc) => {
                out.push(acc);
                cur = Some(r);
            }
            None => cur = Some(r),
        }
    }
    if let Some(acc) = cur {
        out.push(acc);
    }
    out
}

/// Paint a red wavy underline (spell-check style) from `x0` to `x1` at baseline `y`.
pub(crate) fn draw_wavy_underline(painter: &egui::Painter, x0: f32, x1: f32, y: f32, color: egui::Color32) {
    if x1 <= x0 + 1.0 {
        return;
    }
    let amp = 1.3;
    let wl = 3.5;
    let mut pts = Vec::new();
    let mut x = x0;
    let mut up = true;
    while x < x1 {
        pts.push(egui::pos2(x, if up { y - amp } else { y + amp }));
        up = !up;
        x += wl;
    }
    pts.push(egui::pos2(x1, if up { y - amp } else { y + amp }));
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, color)));
}

fn html_attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    loop {
        let idx = low[from..].find(name)? + from;
        let after = &tag[idx + name.len()..];
        let after_t = after.trim_start();
        if let Some(rest) = after_t.strip_prefix('=') {
            let rest = rest.trim_start();
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &rest[1..];
                let end = inner.find(quote)?;
                return Some(inner[..end].to_string());
            }
        }
        from = idx + name.len();
    }
}

/// Detect a block whose trimmed source is exactly one image: either a Markdown
/// `![alt](url)` or an HTML `<img>` (optionally wrapped in a `<figure>` carrying
/// alignment). Returns alt/url/width/align, else `None`. Mixed text+image
/// paragraphs stay on the normal rich-text path.
pub(crate) fn parse_standalone_image(src: &str) -> Option<StandaloneImage> {
    let t = src.trim();

    // ── HTML form: <figure ...><img ...></figure> or a bare <img ...> ──
    if t.starts_with('<') && t.to_ascii_lowercase().contains("<img") {
        let low = t.to_ascii_lowercase();
        let img_start = low.find("<img")?;
        let img_end = t[img_start..].find('>')? + img_start;
        let img_tag = &t[img_start..=img_end];
        let url = html_attr(img_tag, "src")?;
        if url.trim().is_empty() {
            return None;
        }
        let alt = html_attr(img_tag, "alt").unwrap_or_default();
        let width = html_attr(img_tag, "width").and_then(|w| {
            w.trim().trim_end_matches("px").trim().parse::<u32>().ok()
        });
        // Alignment from float (on img) or text-align (on the wrapping figure).
        let align = if low.contains("float:") && low.contains("left") {
            ImgAlign::Left
        } else if low.contains("float:") && low.contains("right") {
            ImgAlign::Right
        } else if low.contains("text-align") && low.contains("center") {
            ImgAlign::Center
        } else {
            ImgAlign::None
        };
        return Some(StandaloneImage { alt, url: url.trim().to_string(), width, align });
    }

    // ── Markdown form: ![alt](url) ──
    let rest = t.strip_prefix("![")?;
    let alt_end = rest.find("](")?;
    let alt = &rest[..alt_end];
    let after = &rest[alt_end + 2..];
    // url runs to the matching ')'. Markdown image titles `(url "t")` are dropped.
    let url_part = after.strip_suffix(')')?;
    if url_part.contains(['\n', '(']) {
        return None; // not a clean single image
    }
    let url = url_part.split_whitespace().next().unwrap_or("").trim();
    if url.is_empty() {
        return None;
    }
    Some(StandaloneImage {
        alt: alt.to_string(),
        url: url.to_string(),
        width: None,
        align: ImgAlign::None,
    })
}

/// One piece of a paragraph that mixes text and inline image(s).
pub(crate) enum InlineSeg {
    /// Editable text run - absolute source byte range.
    Text(std::ops::Range<usize>),
    /// Inline image and the absolute source range of its `![alt](url)` markup.
    Image { alt: String, url: String, range: std::ops::Range<usize> },
}

/// Split a paragraph that contains at least one inline Markdown image
/// `![alt](url)` into alternating text / image segments, with absolute source
/// ranges (offset by `base`). Returns `None` when there is no inline image, so
/// the caller can fall back to the normal rich-text path. A block that is ONLY a
/// single image is handled earlier by `parse_standalone_image`, not here.
pub(crate) fn parse_inline_image_segments(block_src: &str, base: usize) -> Option<Vec<InlineSeg>> {
    let bytes = block_src.as_bytes();
    let n = bytes.len();
    let mut segs: Vec<InlineSeg> = Vec::new();
    let mut i = 0usize;
    let mut text_start = 0usize;
    let mut found = false;
    while i < n {
        if bytes[i] == b'!' && i + 1 < n && bytes[i + 1] == b'[' {
            if let Some(br) = block_src[i + 2..].find(']') {
                let alt_end = i + 2 + br;
                if alt_end + 1 < n && bytes[alt_end + 1] == b'(' {
                    if let Some(par) = block_src[alt_end + 2..].find(')') {
                        let url_end = alt_end + 2 + par; // index of ')'
                        let url_raw = &block_src[alt_end + 2..url_end];
                        // Skip nested parens / multiline (not a clean inline image).
                        if !url_raw.contains(['\n', '(']) && !url_raw.trim().is_empty() {
                            if i > text_start {
                                segs.push(InlineSeg::Text((base + text_start)..(base + i)));
                            }
                            segs.push(InlineSeg::Image {
                                alt: block_src[i + 2..alt_end].to_string(),
                                url: url_raw.split_whitespace().next().unwrap_or("").trim().to_string(),
                                range: (base + i)..(base + url_end + 1),
                            });
                            found = true;
                            i = url_end + 1;
                            text_start = i;
                            continue;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    if !found { return None; }
    if text_start < n {
        segs.push(InlineSeg::Text((base + text_start)..(base + n)));
    }
    Some(segs)
}

/// Serialize an image back to source. Plain `![alt](url)` when it has no
/// geometry; otherwise HTML so width/alignment survive (Markdown cannot express
/// them). The emitted HTML is what `parse_standalone_image` reads back.
pub(crate) fn serialize_image(alt: &str, url: &str, width: Option<u32>, align: ImgAlign) -> String {
    if width.is_none() && align == ImgAlign::None {
        return format!("![{}]({})", alt, url);
    }
    let width_attr = width.map(|w| format!(" width=\"{}\"", w)).unwrap_or_default();
    match align {
        ImgAlign::Center => format!(
            "<figure style=\"text-align: center\"><img src=\"{}\" alt=\"{}\"{}></figure>",
            url, alt, width_attr
        ),
        ImgAlign::Left => format!(
            "<img src=\"{}\" alt=\"{}\"{} style=\"float: left\">",
            url, alt, width_attr
        ),
        ImgAlign::Right => format!(
            "<img src=\"{}\" alt=\"{}\"{} style=\"float: right\">",
            url, alt, width_attr
        ),
        ImgAlign::None => format!("<img src=\"{}\" alt=\"{}\"{}>", url, alt, width_attr),
    }
}

/// Turn an image url/path into a URI egui's image loaders accept. Remote and
/// already-schemed URIs pass through; a local path is resolved relative to the
/// current document directory and given a `file://` scheme.
pub(crate) fn image_uri(url: &str, current_file: &Option<std::path::PathBuf>) -> String {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://")
        || lower.starts_with("file://") || lower.starts_with("data:")
    {
        return url.to_string();
    }
    let p = std::path::Path::new(url);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(dir) = current_file.as_ref().and_then(|f| f.parent()) {
        dir.join(p)
    } else {
        p.to_path_buf()
    };
    format!("file://{}", abs.display())
}

// ── GFM tables (ADR-002 step 8) ───────────────────────────────────────────────

/// One table cell: its trimmed text plus the absolute source byte range that
/// text occupies (so it can be edited in place via `render_rich_text_region`).
pub(crate) struct TableCell {
    pub text: String,
    pub range: std::ops::Range<usize>,
}

/// True for a GFM delimiter row like `|---|:--:|` (dashes/colons/pipes only).
fn is_table_delimiter(t: &str) -> bool {
    t.contains('-') && t.chars().all(|c| matches!(c, '-' | ':' | '|' | ' ' | '\t'))
}

/// Split one table row into cells, computing each cell's absolute source range.
/// `lb` is the byte offset of `line` within the full document source.
fn split_table_cells(line: &str, lb: usize) -> Vec<TableCell> {
    let bytes = line.as_bytes();
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    let mut seg_start = 0usize;
    let mut i = 0usize;
    while i < line.len() {
        if bytes[i] == b'|' && (i == 0 || bytes[i - 1] != b'\\') {
            bounds.push((seg_start, i));
            seg_start = i + 1;
        }
        i += 1;
    }
    bounds.push((seg_start, line.len()));
    // Drop the empty edge segments produced by leading/trailing pipes.
    if bounds.first().map_or(false, |&(s, e)| s == e) {
        bounds.remove(0);
    }
    if bounds.len() > 1 && bounds.last().map_or(false, |&(s, e)| s == e) {
        bounds.pop();
    }
    bounds
        .iter()
        .map(|&(s, e)| {
            let seg = &line[s..e];
            let lead = seg.len() - seg.trim_start().len();
            let trail = seg.len() - seg.trim_end().len();
            let cs = s + lead;
            let ce = (e.saturating_sub(trail)).max(cs);
            TableCell { text: line[cs..ce].to_string(), range: (lb + cs)..(lb + ce) }
        })
        .collect()
}

/// Parse a GFM pipe-table block into rows of cells with absolute source ranges.
/// `block_start` is the byte offset of `block_src` in the document. The
/// delimiter row is dropped; row 0 (if any) is the header.
pub(crate) fn parse_table(block_src: &str, block_start: usize) -> Vec<Vec<TableCell>> {
    let mut rows = Vec::new();
    let mut line_start = 0usize;
    for raw in block_src.split_inclusive('\n') {
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        let lb = block_start + line_start;
        line_start += raw.len();
        let trimmed = line.trim();
        if trimmed.is_empty() || is_table_delimiter(trimmed) {
            continue;
        }
        rows.push(split_table_cells(line, lb));
    }
    rows
}

// ── Table structural editing (rows / columns) ────────────────────────────────

/// Column text alignment from a GFM delimiter row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ColAlign { None, Left, Center, Right }

/// A normalized table: `rows[0]` is the header, the rest are body rows; `aligns`
/// holds one entry per column. Built from GFM, edited structurally, serialized
/// back to GFM. Used for add/delete row & column.
pub(crate) struct TableModel {
    pub aligns: Vec<ColAlign>,
    pub rows: Vec<Vec<String>>,
}

fn split_cells_text(line: &str) -> Vec<String> {
    split_table_cells(line, 0).into_iter().map(|c| c.text).collect()
}

fn col_align_of(cell: &str) -> ColAlign {
    let t = cell.trim();
    let l = t.starts_with(':');
    let r = t.ends_with(':');
    match (l, r) {
        (true, true) => ColAlign::Center,
        (false, true) => ColAlign::Right,
        (true, false) => ColAlign::Left,
        _ => ColAlign::None,
    }
}

impl TableModel {
    pub fn cols(&self) -> usize {
        self.rows.iter().map(|r| r.len()).max().unwrap_or(0).max(self.aligns.len())
    }

    /// Parse a GFM pipe-table block. `None` if there is no usable row.
    pub fn parse(block_src: &str) -> Option<TableModel> {
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut aligns: Vec<ColAlign> = Vec::new();
        for raw in block_src.split_inclusive('\n') {
            let line = raw.trim_end_matches('\n').trim_end_matches('\r');
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            if is_table_delimiter(trimmed) {
                aligns = split_cells_text(line).iter().map(|c| col_align_of(c)).collect();
                continue;
            }
            rows.push(split_cells_text(line));
        }
        if rows.is_empty() { return None; }
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if aligns.len() < cols { aligns.resize(cols, ColAlign::None); }
        Some(TableModel { aligns, rows })
    }

    /// Serialize back to GFM (normalized spacing, one space padding per cell).
    pub fn to_source(&self) -> String {
        let cols = self.cols().max(1);
        let mut out = String::new();
        let row_line = |cells: &[String]| {
            let mut s = String::from("|");
            for c in 0..cols {
                s.push(' ');
                s.push_str(cells.get(c).map(|x| x.as_str()).unwrap_or(""));
                s.push_str(" |");
            }
            s.push('\n');
            s
        };
        let header = self.rows.first().cloned().unwrap_or_default();
        out.push_str(&row_line(&header));
        // delimiter
        out.push('|');
        for c in 0..cols {
            let a = self.aligns.get(c).copied().unwrap_or(ColAlign::None);
            let seg = match a {
                ColAlign::None => "---",
                ColAlign::Left => ":--",
                ColAlign::Center => ":-:",
                ColAlign::Right => "--:",
            };
            out.push(' ');
            out.push_str(seg);
            out.push_str(" |");
        }
        out.push('\n');
        for r in self.rows.iter().skip(1) {
            out.push_str(&row_line(r));
        }
        out
    }

    /// Insert a blank row at body index `at` (0 = just below the header).
    pub fn insert_row(&mut self, at: usize) {
        let cols = self.cols().max(1);
        let idx = (at + 1).min(self.rows.len());
        self.rows.insert(idx, vec![String::new(); cols]);
    }

    /// Delete row `idx` (header included); never removes the last header row.
    pub fn delete_row(&mut self, idx: usize) {
        if self.rows.len() > 1 && idx < self.rows.len() {
            self.rows.remove(idx);
        }
    }

    /// Insert a blank column at index `at`.
    pub fn insert_col(&mut self, at: usize) {
        let cols = self.cols();
        let pos = at.min(cols);
        for row in &mut self.rows {
            if row.len() < cols { row.resize(cols, String::new()); }
            row.insert(pos.min(row.len()), String::new());
        }
        if self.aligns.len() < cols { self.aligns.resize(cols, ColAlign::None); }
        self.aligns.insert(pos.min(self.aligns.len()), ColAlign::None);
    }

    /// Delete column `idx`; never removes the last column.
    pub fn delete_col(&mut self, idx: usize) {
        if self.cols() <= 1 { return; }
        for row in &mut self.rows {
            if idx < row.len() { row.remove(idx); }
        }
        if idx < self.aligns.len() { self.aligns.remove(idx); }
    }
}

// ── Frames (bordered container, ADR-002 step 8) ───────────────────────────────

/// Detect a `<div class="frame"> ... </div>` block. Returns the trimmed interior
/// text and its absolute source byte range so the interior edits in place.
/// `block_start` is the byte offset of `block_src` in the document.
pub(crate) fn parse_frame(block_src: &str, block_start: usize) -> Option<(String, std::ops::Range<usize>)> {
    let low = block_src.to_ascii_lowercase();
    if !low.trim_start().starts_with("<div") {
        return None;
    }
    if !(low.contains("class=\"frame\"") || low.contains("class='frame'")) {
        return None;
    }
    let open_end = block_src.find('>')?;
    let close = low.rfind("</div")?;
    if close <= open_end {
        return None;
    }
    let raw = &block_src[open_end + 1..close];
    let lead = raw.len() - raw.trim_start().len();
    let interior = raw.trim().to_string();
    let i_start = block_start + open_end + 1 + lead;
    let i_end = i_start + interior.len();
    Some((interior, i_start..i_end))
}

/// Detect a `<div style="text-align: X"> ... </div>` alignment block (not a frame).
/// Returns the alignment keyword, the trimmed interior, and its absolute range.
pub(crate) fn parse_aligned_div(block_src: &str, block_start: usize)
    -> Option<(String, String, std::ops::Range<usize>)>
{
    let low = block_src.to_ascii_lowercase();
    if !low.trim_start().starts_with("<div")
        || low.contains("class=\"frame\"") || low.contains("class='frame'")
    {
        return None;
    }
    let ta = low.find("text-align")?;
    let after = &low[ta + "text-align".len()..];
    let colon = after.find(':')?;
    let val: String = after[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if val.is_empty() {
        return None;
    }
    let open_end = block_src.find('>')?;
    let close = low.rfind("</div")?;
    if close <= open_end {
        return None;
    }
    let raw = &block_src[open_end + 1..close];
    let lead = raw.len() - raw.trim_start().len();
    let interior = raw.trim().to_string();
    let i_start = block_start + open_end + 1 + lead;
    let i_end = i_start + interior.len();
    Some((val, interior, i_start..i_end))
}

/// Visual style extracted from a generic `<div style="...">` habillage block.
pub(crate) struct DivStyle {
    pub fill: Option<egui::Color32>,
    pub stroke: Option<egui::Stroke>,
    pub text_color: Option<egui::Color32>,
    pub align: egui::Align,
}

/// Parse a generic styled `<div style="...">...</div>` block (background, border,
/// text colour, alignment). Returns the style, the trimmed interior, and the
/// interior's absolute byte range. Returns None for non-div blocks, the dedicated
/// `class="frame"` boxes, and divs with no visual styling (those fall through to
/// the alignment / default arms, which keep them editable too).
pub(crate) fn parse_styled_div(block_src: &str, block_start: usize)
    -> Option<(DivStyle, String, std::ops::Range<usize>)>
{
    let low = block_src.to_ascii_lowercase();
    if !low.trim_start().starts_with("<div") {
        return None;
    }
    if low.contains("class=\"frame\"") || low.contains("class='frame'") {
        return None;
    }
    let open_end = block_src.find('>')?;
    let close = low.rfind("</div")?;
    if close <= open_end {
        return None;
    }
    let style = extract_tag_attr(&low[..open_end], "style").unwrap_or_default();

    let fill = css_value(&style, "background-color")
        .or_else(|| css_value(&style, "background"))
        .and_then(|v| parse_css_color(&v));
    let text_color = css_value(&style, "color").and_then(|v| parse_css_color(&v));
    let stroke = css_value(&style, "border")
        .and_then(|v| v.split_whitespace().find_map(parse_css_color))
        .or_else(|| css_value(&style, "border-color").and_then(|v| parse_css_color(&v)))
        .map(|c| egui::Stroke::new(1.5, c));

    // Only claim the block when it carries a visual style; plain or align-only
    // divs stay with the alignment / default arms.
    if fill.is_none() && stroke.is_none() && text_color.is_none() {
        return None;
    }

    let align = match css_value(&style, "text-align").as_deref() {
        Some(v) if v.starts_with("center") => egui::Align::Center,
        Some(v) if v.starts_with("right") => egui::Align::Max,
        _ => egui::Align::Min,
    };

    let raw = &block_src[open_end + 1..close];
    let lead = raw.len() - raw.trim_start().len();
    let interior = raw.trim().to_string();
    let i_start = block_start + open_end + 1 + lead;
    let i_end = i_start + interior.len();
    Some((DivStyle { fill, stroke, text_color, align }, interior, i_start..i_end))
}

/// Extract `attr="value"` (or single-quoted) from an opening-tag string.
fn extract_tag_attr(tag: &str, attr: &str) -> Option<String> {
    for q in ['"', '\''] {
        let needle = format!("{}={}", attr, q);
        if let Some(p) = tag.find(&needle) {
            let rest = &tag[p + needle.len()..];
            if let Some(end) = rest.find(q) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

/// Look up a CSS property value in a (lower-cased) inline style string. The match
/// must start at a property boundary so `color` does not match inside
/// `background-color` and `border` does not match inside `border-color`.
fn css_value(style: &str, prop: &str) -> Option<String> {
    let mut from = 0usize;
    while let Some(rel) = style[from..].find(prop) {
        let p = from + rel;
        let before_ok = p == 0
            || matches!(style.as_bytes()[p - 1], b';' | b' ' | b'\t' | b'\n' | b'"' | b'\'');
        if before_ok {
            if let Some(rest) = style[p + prop.len()..].trim_start().strip_prefix(':') {
                let v = rest.split(';').next().unwrap_or("").trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
        from = p + prop.len();
    }
    None
}

/// Parse a CSS colour: `#rgb`, `#rrggbb`, or a small set of named colours common
/// in document habillage. Returns None for anything else (e.g. `transparent`).
fn parse_css_color(s: &str) -> Option<egui::Color32> {
    let s = s.trim().trim_end_matches(';').trim();
    if let Some(hex) = s.strip_prefix('#') {
        let h = hex.as_bytes();
        let hx = |a: u8, b: u8| u8::from_str_radix(&format!("{}{}", a as char, b as char), 16).ok();
        return match h.len() {
            6 => Some(egui::Color32::from_rgb(hx(h[0], h[1])?, hx(h[2], h[3])?, hx(h[4], h[5])?)),
            3 => Some(egui::Color32::from_rgb(hx(h[0], h[0])?, hx(h[1], h[1])?, hx(h[2], h[2])?)),
            _ => None,
        };
    }
    Some(match s {
        "red" => egui::Color32::from_rgb(0xD0, 0x30, 0x30),
        "green" => egui::Color32::from_rgb(0x2D, 0x7A, 0x45),
        "blue" => egui::Color32::from_rgb(0x30, 0x60, 0xC0),
        "orange" => egui::Color32::from_rgb(0xE0, 0x80, 0x10),
        "yellow" => egui::Color32::from_rgb(0xE8, 0xC0, 0x20),
        "purple" => egui::Color32::from_rgb(0x80, 0x40, 0xA0),
        "gray" | "grey" => egui::Color32::GRAY,
        "lightgray" | "lightgrey" => egui::Color32::from_rgb(0xE4, 0xE4, 0xE4),
        "black" => egui::Color32::from_rgb(0x10, 0x10, 0x10),
        "white" => egui::Color32::WHITE,
        "lightblue" => egui::Color32::from_rgb(0xCC, 0xE0, 0xF5),
        "lightyellow" => egui::Color32::from_rgb(0xFF, 0xF8, 0xD0),
        "lightgreen" => egui::Color32::from_rgb(0xD8, 0xF0, 0xD8),
        "aliceblue" => egui::Color32::from_rgb(0xF0, 0xF8, 0xFF),
        _ => return None,
    })
}

#[cfg(test)]
mod styled_div_tests {
    use super::parse_styled_div;

    #[test]
    fn colored_callout_box_is_parsed() {
        let src = "<div style=\"background-color:#eef; border:1px solid #88a; color:#003\">\nNote: be careful.\n</div>";
        let (style, interior, _range) = parse_styled_div(src, 0).expect("styled div");
        assert!(style.fill.is_some(), "background not parsed");
        assert!(style.stroke.is_some(), "border not parsed");
        assert!(style.text_color.is_some(), "text colour not parsed");
        assert_eq!(interior, "Note: be careful.");
    }

    #[test]
    fn plain_or_frame_div_is_not_claimed() {
        assert!(parse_styled_div("<div>plain</div>", 0).is_none());
        assert!(parse_styled_div("<div class=\"frame\">x</div>", 0).is_none());
        // Align-only div is left to the alignment arm.
        assert!(parse_styled_div("<div style=\"text-align:center\">x</div>", 0).is_none());
    }
}

#[cfg(test)]
mod image_tests {
    use super::{parse_standalone_image, image_uri, serialize_image, ImgAlign,
                parse_inline_image_segments, InlineSeg};

    #[test]
    fn inline_image_segments_split_text_and_image() {
        let src = "Before ![logo](a.png) after.";
        let segs = parse_inline_image_segments(src, 0).expect("has inline image");
        assert_eq!(segs.len(), 3, "text / image / text");
        match &segs[0] { InlineSeg::Text(r) => assert_eq!(&src[r.clone()], "Before "), _ => panic!() }
        match &segs[1] {
            InlineSeg::Image { alt, url, range } => {
                assert_eq!(alt, "logo");
                assert_eq!(url, "a.png");
                assert_eq!(&src[range.clone()], "![logo](a.png)");
            }
            _ => panic!(),
        }
        match &segs[2] { InlineSeg::Text(r) => assert_eq!(&src[r.clone()], " after."), _ => panic!() }
        // Plain text has no inline image.
        assert!(parse_inline_image_segments("just text", 0).is_none());
        // Base offset is applied to ranges.
        let segs2 = parse_inline_image_segments("x ![a](u)", 100).unwrap();
        match &segs2[1] { InlineSeg::Image { range, .. } => assert_eq!(range.start, 102), _ => panic!() }
    }
    use std::path::PathBuf;

    #[test]
    fn parses_plain_image() {
        let s = parse_standalone_image("![cat](photos/cat.png)").unwrap();
        assert_eq!(s.alt, "cat");
        assert_eq!(s.url, "photos/cat.png");
        assert_eq!(s.width, None);
        assert_eq!(s.align, ImgAlign::None);
    }

    #[test]
    fn parses_html_img_width_and_float() {
        let s = parse_standalone_image(
            "<img src=\"f.png\" alt=\"a fig\" width=\"320\" style=\"float: left\">",
        ).unwrap();
        assert_eq!(s.url, "f.png");
        assert_eq!(s.alt, "a fig");
        assert_eq!(s.width, Some(320));
        assert_eq!(s.align, ImgAlign::Left);
    }

    #[test]
    fn parses_figure_centered() {
        let s = parse_standalone_image(
            "<figure style=\"text-align: center\"><img src=\"f.png\" alt=\"\" width=\"200\"></figure>",
        ).unwrap();
        assert_eq!(s.width, Some(200));
        assert_eq!(s.align, ImgAlign::Center);
    }

    #[test]
    fn serialize_plain_when_no_geometry() {
        assert_eq!(serialize_image("a", "b.png", None, ImgAlign::None), "![a](b.png)");
    }

    #[test]
    fn serialize_then_parse_roundtrips_geometry() {
        for (w, a) in [
            (Some(300u32), ImgAlign::Center),
            (Some(120), ImgAlign::Left),
            (Some(480), ImgAlign::Right),
            (Some(256), ImgAlign::None),
        ] {
            let src = serialize_image("cap", "img/x.png", w, a);
            let p = parse_standalone_image(&src).unwrap();
            assert_eq!(p.url, "img/x.png");
            assert_eq!(p.alt, "cap");
            assert_eq!(p.width, w, "width roundtrip for {:?}", a);
            assert_eq!(p.align, a, "align roundtrip");
        }
    }

    #[test]
    fn parses_with_surrounding_whitespace_and_empty_alt() {
        let s = parse_standalone_image("   ![](x.jpg)  ").unwrap();
        assert_eq!(s.alt, "");
        assert_eq!(s.url, "x.jpg");
    }

    #[test]
    fn drops_markdown_title() {
        let s = parse_standalone_image("![a](b.png \"a title\")").unwrap();
        assert_eq!(s.url, "b.png");
    }

    #[test]
    fn rejects_mixed_and_multi() {
        assert!(parse_standalone_image("text ![a](b.png)").is_none());
        assert!(parse_standalone_image("![a](b.png) trailing").is_none());
        assert!(parse_standalone_image("![a](b.png)\n![c](d.png)").is_none());
        assert!(parse_standalone_image("not an image").is_none());
        assert!(parse_standalone_image("![a]()").is_none());
    }

    #[test]
    fn uri_passthrough_for_schemes() {
        assert_eq!(image_uri("https://x/y.png", &None), "https://x/y.png");
        assert_eq!(image_uri("file:///z.png", &None), "file:///z.png");
        assert!(image_uri("data:image/png;base64,AAAA", &None).starts_with("data:"));
    }

    #[test]
    fn uri_resolves_relative_to_doc_dir() {
        let doc = Some(PathBuf::from("docs").join("paper.md"));
        let uri = image_uri("fig.png", &doc);
        assert!(uri.starts_with("file://"));
        assert!(uri.ends_with("fig.png"));
        assert!(uri.contains("docs"));
    }
}

#[cfg(test)]
mod reorder_tests {
    use super::reorder_blocks_source;

    #[test]
    fn moves_block_down_and_up() {
        let src = "# A\n\nB para\n\nC para\n";
        // Move block 0 (heading) to position 2 (after C) -> A goes last.
        let down = reorder_blocks_source(src, 0, 3).unwrap();
        let order: Vec<&str> = down.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(order, vec!["B para", "C para", "# A"], "down move wrong: {down:?}");
        // Move block 2 (C) up to position 0.
        let up = reorder_blocks_source(src, 2, 0).unwrap();
        let order: Vec<&str> = up.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(order, vec!["C para", "# A", "B para"], "up move wrong: {up:?}");
    }

    #[test]
    fn noop_and_out_of_range_return_none() {
        let src = "# A\n\nB\n";
        assert!(reorder_blocks_source(src, 1, 1).is_none(), "same index = no-op");
        assert!(reorder_blocks_source(src, 9, 0).is_none(), "out of range");
    }
}

#[cfg(test)]
mod table_tests {
    use super::parse_table;

    #[test]
    fn parses_header_and_body_with_absolute_ranges() {
        let src = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let rows = parse_table(src, 0);
        assert_eq!(rows.len(), 2, "delimiter row dropped, header + 1 body row");
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[0][0].text, "A");
        assert_eq!(rows[0][1].text, "B");
        assert_eq!(rows[1][0].text, "1");
        assert_eq!(rows[1][1].text, "2");
        // Ranges must point at the cell content in the original source.
        for row in &rows {
            for cell in row {
                assert_eq!(&src[cell.range.clone()], cell.text);
            }
        }
    }

    #[test]
    fn handles_no_edge_pipes_and_empty_cells() {
        let src = "a | b | c\n--|--|--\nx |  | z\n";
        let rows = parse_table(src, 0);
        assert_eq!(rows[0].iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(rows[1][1].text, "", "middle empty cell preserved");
        for row in &rows {
            for cell in row {
                assert_eq!(&src[cell.range.clone()], cell.text);
            }
        }
    }

    #[test]
    fn ranges_absolute_with_offset() {
        let prefix = "intro\n\n";
        let table = "| H |\n|---|\n| v |\n";
        let full = format!("{}{}", prefix, table);
        let rows = parse_table(table, prefix.len());
        assert_eq!(&full[rows[0][0].range.clone()], "H");
        assert_eq!(&full[rows[1][0].range.clone()], "v");
    }
}

#[cfg(test)]
mod table_model_tests {
    use super::{TableModel, ColAlign};

    fn model() -> TableModel {
        TableModel::parse("| A | B |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |\n").unwrap()
    }

    #[test]
    fn parses_aligns_and_rows() {
        let m = model();
        assert_eq!(m.cols(), 2);
        assert_eq!(m.aligns, vec![ColAlign::Left, ColAlign::Right]);
        assert_eq!(m.rows.len(), 3); // header + 2 body
        assert_eq!(m.rows[0], vec!["A", "B"]);
        assert_eq!(m.rows[2], vec!["3", "4"]);
    }

    #[test]
    fn roundtrips_through_source() {
        let m = model();
        let m2 = TableModel::parse(&m.to_source()).unwrap();
        assert_eq!(m2.rows, m.rows);
        assert_eq!(m2.aligns, m.aligns);
    }

    #[test]
    fn insert_and_delete_row() {
        let mut m = model();
        m.insert_row(0); // below header
        assert_eq!(m.rows.len(), 4);
        assert_eq!(m.rows[1], vec!["", ""]);
        m.delete_row(1);
        assert_eq!(m.rows.len(), 3);
        // never deletes the last remaining row
        let mut tiny = TableModel::parse("| X |\n|---|\n").unwrap();
        tiny.delete_row(0);
        assert_eq!(tiny.rows.len(), 1);
    }

    #[test]
    fn insert_and_delete_col() {
        let mut m = model();
        m.insert_col(1);
        assert_eq!(m.cols(), 3);
        assert_eq!(m.rows[0], vec!["A", "", "B"]);
        assert_eq!(m.aligns.len(), 3);
        m.delete_col(1);
        assert_eq!(m.rows[0], vec!["A", "B"]);
        // never deletes the last column
        let mut one = TableModel::parse("| X |\n|---|\n| y |\n").unwrap();
        one.delete_col(0);
        assert_eq!(one.cols(), 1);
    }
}

#[cfg(test)]
mod caret_nav_tests {
    //! Headless interaction tests for cross-block caret navigation (ADR-002 §7).
    //! Drives the real editor UI via egui's `Context::run` with synthetic key
    //! events and reads focus from `Memory` - no display, no extra dependency.
    use crate::MdApp;
    use eframe::egui;
    use mdall_core::editor;

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    struct Harness {
        ctx: egui::Context,
        app: MdApp,
    }

    impl Harness {
        fn new(source: &str) -> Self {
            let ctx = egui::Context::default();
            // Register the Name font families the layouter references, so headless
            // layout does not hit an unknown family (system Cambria is absent here).
            let mut fonts = egui::FontDefinitions::default();
            let def = fonts.families
                .get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
            fonts.families.insert(egui::FontFamily::Name("CambriaBold".into()), def.clone());
            fonts.families.insert(egui::FontFamily::Name("CambriaItalic".into()), def);
            ctx.set_fonts(fonts);

            let mut app = MdApp::default();
            app.source = source.to_string();
            app.blocks = editor::parse_document(source);
            Self { ctx, app }
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let Harness { ctx, app } = self;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))),
                events,
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.show_wysiwyg_editor(ui);
                });
            });
        }

        fn focus(&mut self, id: egui::Id) {
            self.ctx.memory_mut(|m| m.request_focus(id));
        }

        fn focused(&self) -> Option<egui::Id> {
            self.ctx.memory(|m| m.focused())
        }

        fn set_caret(&mut self, id: egui::Id, idx: usize) {
            let mut st = egui::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx))));
            st.store(&self.ctx, id);
        }

        /// The cached on-screen rect of block `i` after one frame (plan B hit cache).
        fn block_rect(&mut self, i: usize) -> Option<egui::Rect> {
            self.frame(vec![]);
            self.app.block_hits.iter().find(|h| h.idx == i).map(|h| h.rect)
        }

        /// Run a frame and map a screen point to a `DocPos` via `MdApp::docpos_at`
        /// (queried inside the closure, where a live `ui` exists). Plan B step 5.
        fn docpos_at(&mut self, p: egui::Pos2) -> Option<crate::doc_select::DocPos> {
            let Harness { ctx, app } = self;
            let mut out = None;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.show_wysiwyg_editor(ui);
                    out = app.docpos_at(ui, p);
                });
            });
            out
        }

        /// Run a frame and return what was placed on the clipboard (plan B step 8).
        fn frame_copy(&mut self, events: Vec<egui::Event>) -> String {
            let Harness { ctx, app } = self;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))),
                events,
                ..Default::default()
            };
            let out = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| { app.show_wysiwyg_editor(ui); });
            });
            out.platform_output.copied_text
        }
    }

    fn bid(i: usize) -> egui::Id { egui::Id::new(("wysiwyg_block", i)) }

    /// ArrowRight at the end of a paragraph hands focus to the next paragraph.
    #[test]
    fn arrow_right_at_end_moves_to_next_block() {
        let mut h = Harness::new("First.\n\nSecond.");
        let (id0, id1) = (bid(0), bid(1));
        h.frame(vec![]);             // regions exist
        h.focus(id0);
        h.frame(vec![]);
        h.set_caret(id0, "First.".chars().count()); // caret at end
        h.frame(vec![]);             // prev-caret recorded at the edge
        h.frame(vec![key(egui::Key::ArrowRight)]);
        h.frame(vec![]);             // focus jump applied
        assert_eq!(h.focused(), Some(id1));
    }

    /// ArrowLeft at the start of a paragraph hands focus to the previous one.
    #[test]
    fn arrow_left_at_start_moves_to_prev_block() {
        let mut h = Harness::new("First.\n\nSecond.");
        let (id0, id1) = (bid(0), bid(1));
        h.frame(vec![]);
        h.focus(id1);
        h.frame(vec![]);
        h.set_caret(id1, 0);
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::ArrowLeft)]);
        h.frame(vec![]);
        assert_eq!(h.focused(), Some(id0));
    }

    /// ArrowDown on the last row of a block hands focus to the next block.
    #[test]
    fn arrow_down_at_last_row_moves_to_next_block() {
        let mut h = Harness::new("First.\n\nSecond.");
        let (id0, id1) = (bid(0), bid(1));
        h.frame(vec![]);
        h.focus(id0);
        h.frame(vec![]);
        h.set_caret(id0, "First.".chars().count());
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::ArrowDown)]);
        h.frame(vec![]);
        assert_eq!(h.focused(), Some(id1));
    }

    /// ArrowRight in the middle of a paragraph stays in the same block.
    #[test]
    fn arrow_right_midtext_does_not_jump() {
        let mut h = Harness::new("First.\n\nSecond.");
        let id0 = bid(0);
        h.frame(vec![]);
        h.focus(id0);
        h.frame(vec![]);
        h.set_caret(id0, 2);
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::ArrowRight)]);
        h.frame(vec![]);
        assert_eq!(h.focused(), Some(id0));
    }

    /// Enter mid-paragraph splits it into two paragraphs at the caret.
    #[test]
    fn enter_splits_paragraph_at_caret() {
        let mut h = Harness::new("Hello world.");
        let id0 = bid(0);
        h.frame(vec![]);
        h.focus(id0);
        h.frame(vec![]);
        h.set_caret(id0, 5); // after "Hello"
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::Enter)]);
        assert_eq!(h.app.source, "Hello\n\n world.");
    }

    /// Enter at the very end does not split (no unrepresentable empty paragraph).
    #[test]
    fn enter_at_end_does_not_split() {
        let mut h = Harness::new("Hello.");
        let id0 = bid(0);
        h.frame(vec![]);
        h.focus(id0);
        h.frame(vec![]);
        h.set_caret(id0, "Hello.".chars().count());
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::Enter)]);
        assert!(!h.app.source.contains("\n\n"));
    }

    /// Tab in the last table cell appends a new row.
    #[test]
    fn tab_in_last_cell_adds_row() {
        let mut h = Harness::new("| A | B |\n|---|---|\n| 1 | 2 |\n");
        let last = egui::Id::new(("table_cell", 0usize, 1usize, 1usize));
        h.frame(vec![]);
        h.focus(last);
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::Tab)]);
        // Source now has a header + 2 body rows (the original + a blank one).
        let body_rows = h.app.source.lines()
            .filter(|l| l.trim_start().starts_with('|') && !super::is_table_delimiter(l.trim()))
            .count();
        assert_eq!(body_rows, 3, "header + original + new blank row");
    }

    /// Backspace at the start of a paragraph merges it into the previous one.
    #[test]
    fn backspace_at_start_merges_with_previous() {
        let mut h = Harness::new("First.\n\nSecond.");
        let id1 = bid(1);
        h.frame(vec![]);
        h.focus(id1);
        h.frame(vec![]);
        h.set_caret(id1, 0);
        h.frame(vec![]);
        h.frame(vec![key(egui::Key::Backspace)]);
        assert_eq!(h.app.source, "First.Second.");
    }

    fn text(s: &str) -> egui::Event { egui::Event::Text(s.to_string()) }
    fn append_id() -> egui::Id { egui::Id::new("wysiwyg_append") }

    /// CONTRACT (ADR-002 1bis, do not regress): a brand-new EMPTY document is
    /// immediately typable in the rendered view via the trailing append region,
    /// with no detour through the Source view.
    #[test]
    fn empty_document_is_typable() {
        let mut h = Harness::new("");
        h.frame(vec![]);
        h.focus(append_id());
        h.frame(vec![]);
        h.frame(vec![text("Hi")]);
        h.frame(vec![]);
        assert_eq!(h.app.source.trim(), "Hi", "typing in an empty doc must create the first paragraph");
    }

    /// CONTRACT: typing in the trailing region appends a NEW paragraph at the end.
    #[test]
    fn append_region_adds_paragraph_at_end() {
        let mut h = Harness::new("Para one.");
        h.frame(vec![]);
        h.focus(append_id());
        h.frame(vec![]);
        h.frame(vec![text("Para two.")]);
        h.frame(vec![]);
        assert_eq!(h.app.source, "Para one.\n\nPara two.");
    }

    /// CONTRACT: character-by-character typing in the append region must NOT turn
    /// each keystroke into its own paragraph; focus is handed to the new block.
    #[test]
    fn append_region_char_by_char_stays_one_paragraph() {
        let mut h = Harness::new("");
        h.frame(vec![]);
        h.focus(append_id());
        h.frame(vec![]);
        h.frame(vec![text("H")]);
        h.frame(vec![]);            // focus settles on the materialized block
        h.frame(vec![text("i")]);
        h.frame(vec![]);
        assert_eq!(h.app.source.trim(), "Hi", "consecutive keystrokes must stay in one paragraph");
    }

    fn ctrl_a() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::A, physical_key: None, pressed: true, repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }
    }

    fn press(p: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos: p, button: egui::PointerButton::Primary, pressed: true,
            modifiers: egui::Modifiers::default(),
        }
    }
    fn release(p: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos: p, button: egui::PointerButton::Primary, pressed: false,
            modifiers: egui::Modifiers::default(),
        }
    }
    fn moved(p: egui::Pos2) -> egui::Event { egui::Event::PointerMoved(p) }
    fn copy_event() -> egui::Event { egui::Event::Copy }

    /// CONTRACT (plan B step 3): Ctrl+A selects the WHOLE document, across blocks.
    #[test]
    fn ctrl_a_selects_whole_document() {
        let mut h = Harness::new("First.\n\nSecond.\n\nThird.");
        h.frame(vec![]);
        h.frame(vec![ctrl_a()]);
        h.frame(vec![]);
        let sel = h.app.doc_selection.expect("Ctrl+A must set a document selection");
        assert_eq!(sel.anchor, crate::doc_select::DocPos::new(0, 0));
        assert_eq!(sel.head.block, 2, "selection reaches the last block");
        assert!(sel.touches_block(0) && sel.touches_block(1) && sel.touches_block(2));
    }

    /// A pointer press clears the document selection.
    #[test]
    fn pointer_press_clears_doc_selection() {
        let mut h = Harness::new("First.\n\nSecond.");
        h.frame(vec![]);
        h.frame(vec![ctrl_a()]);
        h.frame(vec![]);
        assert!(h.app.doc_selection.is_some());
        h.frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(400.0, 300.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }]);
        h.frame(vec![]);
        assert!(h.app.doc_selection.is_none(), "a click clears the document selection");
    }

    /// CONTRACT (plan B step 5): a screen point maps to the DocPos of the block it
    /// sits in - the cross-block hit-test that click / drag selection will route
    /// through. Each block's centre must resolve to that block's index.
    #[test]
    fn docpos_at_picks_the_block_under_the_point() {
        let mut h = Harness::new("Alpha line.\n\nBeta line.\n\nGamma line.");
        for i in 0..3 {
            let c = h.block_rect(i).expect("block hit cached").center();
            let p = h.docpos_at(c).expect("docpos_at must resolve a point in a block");
            assert_eq!(p.block, i, "centre of block {i} maps to block {i}");
        }
    }

    /// The left edge (vertical mid) of a block maps to byte 0 of its visible text.
    /// (The shared top corner is a knife-edge between two blocks - `block_at_y`'s
    /// inclusive bounds give it to the upper block - so we probe just inside.)
    #[test]
    fn docpos_at_block_start_is_byte_zero() {
        let mut h = Harness::new("First.\n\nSecond paragraph.");
        let r = h.block_rect(1).expect("block 1 hit cached");
        let left_mid = egui::pos2(r.min.x, r.center().y);
        let p = h.docpos_at(left_mid).expect("docpos_at must resolve the line start");
        assert_eq!((p.block, p.byte), (1, 0));
    }

    /// CONTRACT (plan B step 7, the headline): pressing in one block and dragging
    /// into another builds a document selection that SPANS the crossed blocks - the
    /// cross-block mouse selection egui's per-block TextEdits cannot do on their own.
    #[test]
    fn cross_block_drag_selects_across_blocks() {
        let mut h = Harness::new("Alpha line.\n\nBeta line.\n\nGamma line.");
        let c0 = h.block_rect(0).expect("b0 cached").center();
        let c2 = h.block_rect(2).expect("b2 cached").center();
        // Focus block 0 first so we can prove step 7b surrenders it during the drag.
        h.focus(bid(0));
        h.frame(vec![]);
        assert_eq!(h.focused(), Some(bid(0)), "block 0 is focused before the drag");
        h.frame(vec![press(c0)]);   // press in block 0 (arms the drag)
        h.frame(vec![moved(c2)]);   // drag into block 2 (button still down)
        let sel = h.app.doc_selection.expect("a cross-block drag must build a selection");
        assert!(sel.touches_block(0) && sel.touches_block(1) && sel.touches_block(2),
            "selection spans the dragged-over blocks");
        assert!(h.app.doc_dragging, "the drag is active while the button is held");
        assert!(h.focused().is_none(), "step 7b: no block keeps focus during the drag");
        h.frame(vec![release(c2)]); // release ends the gesture
        assert!(!h.app.doc_dragging, "release ends the drag");
        assert!(h.app.doc_selection.is_some(), "the selection survives the release");
    }

    /// A press + tiny move WITHIN one block must NOT start a document drag: in-block
    /// selection stays egui-native (single block), so normal editing is untouched.
    #[test]
    fn in_block_drag_does_not_start_document_selection() {
        let mut h = Harness::new("A reasonably long single paragraph of text here.");
        let r = h.block_rect(0).expect("b0 cached");
        let a = egui::pos2(r.min.x + 10.0, r.center().y);
        let b = egui::pos2(r.min.x + 60.0, r.center().y); // still inside block 0
        h.frame(vec![press(a)]);
        h.frame(vec![moved(b)]);
        assert!(!h.app.doc_dragging, "an in-block drag must not promote to a doc drag");
        assert!(h.app.doc_selection.is_none(), "no cross-block selection for an in-block drag");
        h.frame(vec![release(b)]);
    }

    /// CONTRACT (plan B step 8): the document selection's text is the VISIBLE text of
    /// the crossed blocks, sliced per block and joined by a blank line.
    #[test]
    fn doc_selection_text_joins_visible_across_blocks() {
        use crate::doc_select::{DocPos, DocSelection};
        let mut h = Harness::new("First para.\n\nSecond para.\n\nThird para.");
        h.app.doc_selection = Some(DocSelection {
            anchor: DocPos::new(0, 6), // "First |para."
            head:   DocPos::new(2, 5), // "Third| para."
        });
        let txt = h.app.doc_selection_text().expect("a multi-block selection yields text");
        assert_eq!(txt, "para.\n\nSecond para.\n\nThird");
    }

    /// Ctrl+C (Event::Copy) over a whole-document selection puts the visible document
    /// text on the clipboard.
    #[test]
    fn copy_event_copies_the_document_selection() {
        let mut h = Harness::new("Alpha.\n\nBeta.");
        h.frame(vec![]);
        h.frame(vec![ctrl_a()]); // whole-document selection
        h.frame(vec![]);
        let copied = h.frame_copy(vec![copy_event()]);
        assert_eq!(copied, "Alpha.\n\nBeta.");
    }

    /// With no document selection, Copy is left to the focused block (we put nothing
    /// on the clipboard from the document layer).
    #[test]
    fn copy_event_without_doc_selection_is_a_noop_here() {
        let mut h = Harness::new("Alpha.\n\nBeta.");
        h.frame(vec![]);
        let copied = h.frame_copy(vec![copy_event()]);
        assert_eq!(copied, "");
    }

    /// selection_row_rects gives one rect spanning the selected glyphs on a single
    /// line (plan B step 7c precise highlight).
    #[test]
    fn selection_row_rects_single_line() {
        let h = Harness::new("");
        let mut rects = Vec::new();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))),
            ..Default::default()
        };
        let _ = h.ctx.run(raw, |ctx| {
            let mut job = egui::text::LayoutJob::default();
            job.append("HELLO WORLD", 0.0, egui::TextFormat {
                font_id: egui::FontId::proportional(16.0), ..Default::default() });
            job.wrap.max_width = f32::INFINITY;
            let galley = ctx.fonts(|f| f.layout_job(job));
            rects = super::selection_row_rects(&galley, 0, 5); // "HELLO"
        });
        assert_eq!(rects.len(), 1, "single line yields one rect");
        assert!(rects[0].width() > 0.0, "the rect spans the selected glyphs");
        assert!(rects[0].min.x.abs() < 1.0, "selection from index 0 starts at the left");
    }

    /// A selection over wrapped text yields one rect per wrapped row.
    #[test]
    fn selection_row_rects_one_per_wrapped_row() {
        let h = Harness::new("");
        let mut n = 0usize;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))),
            ..Default::default()
        };
        let _ = h.ctx.run(raw, |ctx| {
            let mut job = egui::text::LayoutJob::default();
            job.append(&"word ".repeat(40), 0.0, egui::TextFormat {
                font_id: egui::FontId::proportional(16.0), ..Default::default() });
            job.wrap.max_width = 120.0;
            let galley = ctx.fonts(|f| f.layout_job(job));
            let chars = galley.text().chars().count();
            n = super::selection_row_rects(&galley, 0, chars).len();
        });
        assert!(n > 1, "a wrapped selection yields one rect per row, got {n}");
    }
}

#[cfg(test)]
mod cache_tests {
    use crate::{MdApp, wysiwyg_map};

    #[test]
    fn mapped_block_cache_is_correct_and_not_stale() {
        let mut app = MdApp::default();
        let src = "a **b** c";
        let direct = wysiwyg_map::map_block(src);
        let miss = app.mapped_block(src);   // compute + store
        let hit = app.mapped_block(src);    // served from cache
        assert_eq!(miss.visible, direct.visible);
        assert_eq!(hit.visible, direct.visible);
        assert_eq!(hit.spans.len(), direct.spans.len());
        // A different source must map fresh, never return the cached entry.
        let edited = app.mapped_block("a **B** c");
        assert_eq!(edited.visible, "a B c");
    }
}

#[cfg(test)]
mod wrap_tests {
    use eframe::egui;
    use crate::wysiwyg_map;

    /// Regression guard: a rendered region galley must wrap at its max width.
    /// (The editor bug was a layouter that ignored wrap_width, leaving the galley
    /// infinitely wide so text overflowed the page margins.)
    #[test]
    fn buffer_job_wraps_at_max_width() {
        let ctx = egui::Context::default();
        let text = "word ".repeat(60);
        let mut rows = 0usize;
        // Fonts are only available inside a running frame.
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mb = wysiwyg_map::map_block(text.trim());
            let mut job = wysiwyg_map::render_buffer_job(&mb.visible, &mb, 14.0, &egui::Visuals::light());
            job.wrap.max_width = 120.0;
            rows = ctx.fonts(|f| f.layout_job(job)).rows.len();
        });
        assert!(rows > 1, "narrow region must wrap text onto multiple rows");
    }
}

#[cfg(test)]
mod frame_tests {
    use super::parse_frame;

    #[test]
    fn parses_frame_interior_and_range() {
        let src = "<div class=\"frame\">\n\nHello **world**\n\n</div>\n";
        let (interior, range) = parse_frame(src, 0).unwrap();
        assert_eq!(interior, "Hello **world**");
        assert_eq!(&src[range], "Hello **world**");
    }

    #[test]
    fn ignores_non_frame_divs() {
        assert!(parse_frame("<div style=\"text-align:center\">\n\nx\n\n</div>", 0).is_none());
        assert!(parse_frame("just text", 0).is_none());
    }

    #[test]
    fn frame_range_absolute_with_offset() {
        let prefix = "abc\n\n";
        let frame = "<div class='frame'>\n\nNote\n\n</div>\n";
        let full = format!("{}{}", prefix, frame);
        let (interior, range) = parse_frame(frame, prefix.len()).unwrap();
        assert_eq!(interior, "Note");
        assert_eq!(&full[range], "Note");
    }

    #[test]
    fn parses_aligned_div() {
        use super::parse_aligned_div;
        let src = "<div style=\"text-align: center\">\n\nHello\n\n</div>\n";
        let (a, interior, range) = parse_aligned_div(src, 0).unwrap();
        assert_eq!(a, "center");
        assert_eq!(interior, "Hello");
        assert_eq!(&src[range], "Hello");
        // A frame div is not an alignment div; a plain div has no alignment.
        assert!(parse_aligned_div("<div class=\"frame\">\n\nx\n\n</div>", 0).is_none());
        assert!(parse_aligned_div("<div>\n\nx\n\n</div>", 0).is_none());
    }
}
