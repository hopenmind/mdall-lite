//! Command palette (Ctrl+K) - a fuzzy-searchable list of every action.
//! A modern-app signature (Linear / Raycast / VS Code). Renders as a centered
//! overlay; executes through existing MdApp methods, so it adds no new logic.

use eframe::egui::{self, Align2, Color32, FontId};

use crate::MdApp;
use crate::ViewMode;
use crate::theme;
use crate::ui::icons::{self, Icon};

/// Every command the palette can run. Mapped to existing MdApp methods.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    ImportDocx,
    ExportDialog,
    QuickPdf,
    QuickHtml,
    Print,
    ViewEditor,
    ViewSource,
    ViewSplit,
    ViewHub,
    Bold,
    Italic,
    Underline,
    Strikethrough,
    InlineCode,
    Heading1,
    Heading2,
    Heading3,
    EquationBlock,
    InlineEquation,
    InsertLink,
    InsertImage,
    InsertTable,
    CodeBlock,
    Blockquote,
    BulletList,
    NumberedList,
    HorizontalRule,
    ToggleTheme,
    Options,
    FindReplace,
    Metadata,
}

/// (action, label, shortcut hint). Order is the default ranking when the query is empty.
const ACTIONS: &[(Action, &str, &str)] = &[
    (Action::Open, "Open file", "Ctrl+O"),
    (Action::Save, "Save", "Ctrl+S"),
    (Action::SaveAs, "Save As", "Ctrl+Shift+S"),
    (Action::New, "New document", "Ctrl+N"),
    (Action::ImportDocx, "Import DOCX (recover source)", ""),
    (Action::ExportDialog, "Export As", ""),
    (Action::QuickPdf, "Export PDF", ""),
    (Action::QuickHtml, "Export HTML", ""),
    (Action::Print, "Print", "Ctrl+P"),
    (Action::ViewEditor, "View: Editor", ""),
    (Action::ViewSource, "View: Source", ""),
    (Action::ViewSplit, "View: Split", ""),
    (Action::ViewHub, "View: Hub", ""),
    (Action::ToggleTheme, "Toggle light / dark theme", "Ctrl+Shift+D"),
    (Action::Options, "Open Options", ""),
    (Action::FindReplace, "Find & Replace", "Ctrl+H"),
    (Action::Metadata, "Edit document metadata", ""),
    (Action::Bold, "Bold", "Ctrl+B"),
    (Action::Italic, "Italic", "Ctrl+I"),
    (Action::Underline, "Underline", "Ctrl+U"),
    (Action::Strikethrough, "Strikethrough", ""),
    (Action::InlineCode, "Inline code", ""),
    (Action::Heading1, "Heading 1", ""),
    (Action::Heading2, "Heading 2", ""),
    (Action::Heading3, "Heading 3", ""),
    (Action::EquationBlock, "Insert equation block", "Ctrl+E"),
    (Action::InlineEquation, "Insert inline equation", ""),
    (Action::InsertLink, "Insert link", "Ctrl+K"),
    (Action::InsertImage, "Insert image", ""),
    (Action::InsertTable, "Insert table", ""),
    (Action::CodeBlock, "Insert code block", ""),
    (Action::Blockquote, "Insert blockquote", ""),
    (Action::BulletList, "Insert bullet list", ""),
    (Action::NumberedList, "Insert numbered list", ""),
    (Action::HorizontalRule, "Insert horizontal rule", ""),
];

/// Case-insensitive subsequence match (fuzzy): every char of `q` appears in
/// `hay` in order. Empty query always matches.
fn fuzzy(hay: &str, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let mut chars = hay.chars();
    for needle in q.chars() {
        loop {
            match chars.next() {
                Some(c) if c == needle => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

impl MdApp {
    pub(crate) fn show_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.command_palette_open = false;
            self.palette_query.clear();
            return;
        }

        // Dimmed backdrop; clicking it closes the palette.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("palette_dim"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.left_top())
            .show(ctx, |ui| {
                let resp = ui.allocate_rect(screen, egui::Sense::click());
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(60));
                if resp.clicked() {
                    self.command_palette_open = false;
                }
            });

        let q = self.palette_query.to_lowercase();
        let filtered: Vec<&(Action, &str, &str)> = ACTIONS
            .iter()
            .filter(|(_, label, hint)| {
                fuzzy(&label.to_lowercase(), &q) || (!hint.is_empty() && hint.to_lowercase().contains(&q))
            })
            .collect();

        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let mut chosen: Option<Action> = None;

        egui::Area::new(egui::Id::new("palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .rounding(10.0)
                    .inner_margin(egui::Margin::same(10.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0),
                        blur: 18.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(70),
                    })
                    .show(ui, |ui| {
                        ui.set_width(540.0);

                        // Search row
                        ui.horizontal(|ui| {
                            let (sr, _) = ui.allocate_exact_size(egui::vec2(24.0, 28.0), egui::Sense::hover());
                            icons::paint_icon(ui.painter(), Icon::Search, sr.shrink(5.0), theme::TEXT_2);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.palette_query)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Type a command...")
                                    .frame(false),
                            );
                            resp.request_focus();
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                            if filtered.is_empty() {
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("No matching command")
                                        .size(12.5)
                                        .color(theme::TEXT_MUTED),
                                );
                            }
                            for (idx, (action, label, hint)) in filtered.iter().enumerate() {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 28.0),
                                    egui::Sense::click(),
                                );
                                // First result is the default (Enter target); also highlight on hover.
                                if idx == 0 || resp.hovered() {
                                    ui.painter().rect_filled(rect, 6.0, theme::ACCENT_PALE);
                                }
                                ui.painter().text(
                                    rect.left_center() + egui::vec2(8.0, 0.0),
                                    Align2::LEFT_CENTER,
                                    *label,
                                    FontId::proportional(13.5),
                                    theme::TEXT,
                                );
                                if !hint.is_empty() {
                                    ui.painter().text(
                                        rect.right_center() - egui::vec2(8.0, 0.0),
                                        Align2::RIGHT_CENTER,
                                        *hint,
                                        FontId::proportional(11.5),
                                        theme::TEXT_MUTED,
                                    );
                                }
                                if resp.clicked() {
                                    chosen = Some(*action);
                                }
                                ui.add_space(2.0);
                            }
                        });
                    });
            });

        // Enter runs the top result.
        if enter {
            if let Some((action, _, _)) = filtered.first() {
                chosen = Some(*action);
            }
        }

        if let Some(action) = chosen {
            self.command_palette_open = false;
            self.palette_query.clear();
            self.run_palette_action(action, ctx);
        }
    }

    /// Execute a palette action via existing MdApp methods (no new behavior here).
    fn run_palette_action(&mut self, action: Action, _ctx: &egui::Context) {
        match action {
            Action::New => self.do_new(),
            Action::Open => self.do_open(),
            Action::Save => self.do_save(),
            Action::SaveAs => self.do_save_as(),
            Action::ImportDocx => self.do_import_docx(),
            Action::ExportDialog => self.export_dialog.visible = true,
            Action::QuickPdf => self.do_export_pdf(),
            Action::QuickHtml => self.do_export_html(),
            Action::Print => self.do_print(),
            Action::ViewEditor => {
                self.view_mode = ViewMode::Editor;
                self.segments_dirty = true;
            }
            Action::ViewSource => self.view_mode = ViewMode::Source,
            Action::ViewSplit => self.view_mode = ViewMode::Split,
            Action::ViewHub => self.view_mode = ViewMode::Converter,
            Action::Bold => self.wrap_text("**", "**"),
            Action::Italic => self.wrap_text("*", "*"),
            Action::Underline => self.wrap_text("<u>", "</u>"),
            Action::Strikethrough => self.wrap_text("~~", "~~"),
            Action::InlineCode => self.wrap_text("`", "`"),
            Action::Heading1 => self.insert_text("# "),
            Action::Heading2 => self.insert_text("## "),
            Action::Heading3 => self.insert_text("### "),
            Action::EquationBlock => self.insert_text("$$\n\\sum_{i=0}^{n} x_i\n$$\n"),
            Action::InlineEquation => self.wrap_text("$", "$"),
            Action::InsertLink => self.open_link_dialog(false),
            Action::InsertImage => self.open_link_dialog(true),
            Action::InsertTable => self.insert_text(
                "| Col 1 | Col 2 | Col 3 |\n|--------|--------|--------|\n|  |  |  |\n",
            ),
            Action::CodeBlock => self.insert_text("```\n\n```\n"),
            Action::Blockquote => self.insert_text("> "),
            Action::BulletList => self.insert_text("- "),
            Action::NumberedList => self.insert_text("1. "),
            Action::HorizontalRule => self.insert_text("---\n"),
            Action::ToggleTheme => self.dark_mode = !self.dark_mode,
            Action::Options => self.options_open = true,
            Action::FindReplace => self.show_search = true,
            Action::Metadata => self.show_metadata = true,
        }
    }
}
