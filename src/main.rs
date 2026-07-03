// Hide the console for the GUI release build, but NOT for test builds
// (otherwise `cargo test` output would be suppressed on Windows).
#![cfg_attr(not(test), windows_subsystem = "windows")]

// All document/conversion/equation logic lives in the `mdall_core` crate
// (crates/core). The binary is the egui UI shell that consumes it.
use mdall_core::{editor, export, inline_math};

// `wysiwyg` and `equation_layout` are UI (egui), so they live in the binary,
// not the core library.
mod wysiwyg;
mod wysiwyg_map;
mod i18n;
mod modules;
mod settings;
mod equation_layout;
mod doc_select;

use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;

// Heimdall design system - see src/theme.rs.
mod theme;

fn heimdall_light_visuals() -> egui::Visuals { theme::light_visuals() }

/// Build the application's full font set: embedded New Computer Modern (serif +
/// math) plus, on Windows, Cambria / Cambria Math / Bold / Italic and the named
/// families the editor and equation rendering reference ("CambriaMath", etc.).
/// `user_font_path`, when given, is layered on top as the preferred proportional
/// face WITHOUT dropping the math / named families. This is why changing the font
/// can never remove the glyphs equations need (the "misrendered Phi" risk) and can
/// never panic on a missing font family.
fn build_font_definitions(user_font_path: Option<&std::path::Path>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    // Cross-platform base: Typst's New Computer Modern (serif + math).
    if let Some(bytes) = mdall_core::fonts::embedded_ui_serif() {
        fonts.font_data.insert("NewCM".to_string(), egui::FontData::from_static(bytes));
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "NewCM".to_string());
        }
    }
    if let Some(bytes) = mdall_core::fonts::embedded_ui_math() {
        fonts.font_data.insert("NewCMMath".to_string(), egui::FontData::from_static(bytes));
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.push("NewCMMath".to_string());
        }
        fonts.families.insert(
            egui::FontFamily::Name("CambriaMath".into()),
            vec!["NewCMMath".to_string(), "NewCM".to_string()],
        );
    }

    // System fonts (Windows): Cambria + Cambria Math / Bold / Italic + Symbols.
    let use_system_fonts = std::env::var_os("MD2ALL_NO_SYSTEM_FONTS").is_none();
    if use_system_fonts {
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\cambria.ttc") {
            fonts.font_data.insert("Cambria".to_string(), egui::FontData::from_owned(data.clone()));
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "Cambria".to_string());
            }
            fonts.font_data.insert(
                "CambriaMath".to_string(),
                egui::FontData { font: data.into(), index: 1, tweak: Default::default() },
            );
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(1, "CambriaMath".to_string());
            }
            fonts.families.insert(
                egui::FontFamily::Name("CambriaMath".into()),
                vec!["CambriaMath".to_string(), "Cambria".to_string(), "Symbols".to_string()],
            );
        }
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\cambriab.ttf") {
            fonts.font_data.insert("CambriaBold".to_string(), egui::FontData::from_owned(data));
            fonts.families.insert(
                egui::FontFamily::Name("CambriaBold".into()),
                vec!["CambriaBold".to_string()],
            );
        }
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\cambriai.ttf") {
            fonts.font_data.insert("CambriaItalic".to_string(), egui::FontData::from_owned(data));
            fonts.families.insert(
                egui::FontFamily::Name("CambriaItalic".into()),
                vec!["CambriaItalic".to_string()],
            );
        }
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
            fonts.font_data.insert("Symbols".to_string(), egui::FontData::from_owned(data));
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push("Symbols".to_string());
            }
        }
    }

    // The user-chosen proportional face, layered on top, never replacing the set.
    if let Some(p) = user_font_path {
        if let Ok(data) = std::fs::read(p) {
            fonts.font_data.insert("UserFont".to_string(), egui::FontData::from_owned(data));
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "UserFont".to_string());
            }
        }
    }
    fonts
}

fn main() -> eframe::Result<()> {
    // Crash diagnostics - the release build hides the console (windows_subsystem),
    // so a panic would otherwise vanish silently. Write full panic info to
    // crash_log.txt next to the exe to pinpoint field crashes (e.g. editor selection).
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".into());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let log = format!("PANIC at {loc}\n{msg}\n\nBacktrace:\n{bt}\n");
        let path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("crash_log.txt")))
            .unwrap_or_else(|| std::path::PathBuf::from("crash_log.txt"));
        let _ = std::fs::write(&path, &log);
        eprintln!("{log}");
    }));

    let icon = load_icon();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "MD -> ALL",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(heimdall_light_visuals());
            theme::apply_modern_style(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);

            cc.egui_ctx.set_fonts(build_font_definitions(None));

            // Open a file passed on the command line (file association / drag onto
            // the exe / `mdall <file>`), converting it to Markdown like the Hub.
            let mut app = MdApp::default();
            // Restore persisted preferences (UI language, theme, PDF engine).
            let prefs = crate::settings::load();
            app.app_lang = prefs.app_lang;
            app.dark_mode = prefs.dark_mode;
            app.pdf_native = prefs.pdf_native;
            app.source_line_numbers = prefs.source_line_numbers;
            app.source_wrap = prefs.source_wrap;
            app.toolbar_minified = prefs.toolbar_minified;
            app.show_page_numbers = prefs.show_page_numbers;
            app.header_text = prefs.header_text.clone();
            app.footer_text = prefs.footer_text.clone();
            app.page_color = prefs.page_color;
            app.page_frame = prefs.page_frame;
            app.icon_set = crate::ui::icons::IconSet::from_key(&prefs.icon_set);
            app.ui_scale = prefs.ui_scale;
            app.a11y_high_contrast = prefs.a11y_high_contrast;
            app.a11y_reduced_motion = prefs.a11y_reduced_motion;
            app.a11y_large_targets = prefs.a11y_large_targets;
            mdall_core::export::set_native_pdf(app.pdf_native);
            crate::ui::icons::set_icon_set(app.icon_set);
            app.prefs_snapshot = app.current_prefs();
            app.autoload_default_dictionary();
            if let Some(arg) = std::env::args().nth(1) {
                let p = std::path::PathBuf::from(&arg);
                if p.is_file() {
                    match MdApp::import_to_md(&p) {
                        Ok(md) => {
                            app.source = md;
                            app.segments_dirty = true;
                            app.view_mode = ViewMode::Editor;
                            // Surface reviewer feedback when opening a DOCX directly.
                            if p.extension().and_then(|e| e.to_str())
                                .map(|e| e.eq_ignore_ascii_case("docx")).unwrap_or(false)
                            {
                                app.review_items =
                                    mdall_core::docx_review::extract_review_items(&p).unwrap_or_default();
                                app.show_review_panel = !app.review_items.is_empty();
                            }
                            app.current_file = Some(p);
                        }
                        Err(e) => app.status_msg = format!("Open failed: {e}"),
                    }
                }
            }
            Ok(Box::new(app))
        }),
    )
}

/// Build the window/taskbar icon programmatically: a gold Heimdall mark on a
/// rounded tile (an upward chevron over a bar = the Bifrost bridge / ascent).
/// Drawn at 256px and downscaled by the OS, it stays legible at 16/32px where
/// the full wordmark logo turns to mush.
fn load_icon() -> egui::IconData {
    // The real brand logo (transparent-background RGBA) as the window/taskbar
    // icon - the Heimdall crown-and-ring emblem, not a generic placeholder.
    let img = match image::load_from_memory(include_bytes!("../assets/Logo.png")) {
        Ok(img) => img.into_rgba8(),
        // Embedded asset; a decode failure would mean a broken build, not user
        // input. Degrade to a 1x1 transparent icon rather than panic at launch.
        Err(_) => return egui::IconData { width: 1, height: 1, rgba: vec![0, 0, 0, 0] },
    };
    egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    }
}

#[derive(PartialEq, Clone, Copy)]
enum ViewMode {
    Converter, // Conversion Hub - default startup screen
    Split,
    Source,
    Editor,
}

// Conversion output formats - see src/output_format.rs.
mod output_format;

// UI state structs (no egui, no logic) - see src/ui/state.rs.
mod ui;
use ui::state::{
    ConversionHub, ConversionSettings, EditorMode, EquationEditor, ExportDialog,
    LinkDialog, Toast, WysiwygFormatState,
};

struct MdApp {
    source: String,
    current_file: Option<PathBuf>,
    modified: bool,
    show_metadata: bool,
    show_search: bool,
    search_query: String,
    replace_query: String,
    status_msg: String,
    cursor_pos: usize,
    /// Anchor of the current selection (secondary cursor).  Equal to cursor_pos when no selection.
    selection_anchor: usize,
    meta: export::PdfMetadata,
    logo_tex: Option<egui::TextureHandle>,
    view_mode: ViewMode,
    eq_editor: EquationEditor,
    link_dialog: LinkDialog,
    image_dialog: crate::ui::state::ImageDialog,
    table_dialog: crate::ui::state::TableDialog,
    svg_editor: crate::ui::svg_editor::SvgEditor,
    /// Currently selected standalone image (by source range): shows a resize frame.
    selected_image: Option<std::ops::Range<usize>>,
    /// Live edit buffer for the focused rich-text region (Id + visible text). While
    /// a region is focused, egui owns this buffer continuously instead of having it
    /// re-derived from the source each frame - that re-derivation lagged a frame
    /// behind fast keystrokes and made egui reset the caret (typed chars scattered).
    region_live: Option<(egui::Id, String)>,
    /// Buffer for the always-present trailing "new paragraph" region. The rendered
    /// page must stay typable even with zero blocks (a brand-new empty document) or
    /// at the very end (adding a paragraph), without detouring through the Source
    /// view (ADR-002 1bis). The first keystroke here is materialized into the source
    /// as a new paragraph and focus is handed to that real block.
    append_buf: String,
    /// Print-layout pagination state, recomputed each frame from the measured block
    /// heights: the block indices that start a new page, and the resulting page
    /// count. Used to paint discrete A4 sheets and insert page-break spacers so the
    /// editing surface is a stack of pages, not one ribbon that grows.
    page_breaks: Vec<usize>,
    page_count: usize,
    /// Document-level selection spanning blocks (plan B). Inert for now: set, painted
    /// and acted on by later increments. egui's per-block selection cannot cross
    /// blocks, so this lives in the app, not in any TextEdit.
    #[allow(dead_code)]
    doc_selection: Option<doc_select::DocSelection>,
    /// Per-frame cache of where each block sits on screen + its source range, for
    /// document-level hit-testing (plan B). Rebuilt every render; read by the
    /// document selection in later increments.
    #[allow(dead_code)]
    block_hits: Vec<crate::ui::editor::BlockHit>,
    /// Anchor of an in-progress cross-block drag-selection (plan B step 7): the
    /// DocPos where the press landed. `None` when no drag is active.
    #[allow(dead_code)]
    doc_drag_anchor: Option<doc_select::DocPos>,
    /// True only while a cross-block drag is actively building a document selection
    /// (plan B step 7). Gates the read-only galley paint that lets the drag own the
    /// pointer; false in all normal editing, so the default render path is unchanged.
    #[allow(dead_code)]
    doc_dragging: bool,
    /// Reviewer feedback (tracked changes + comments) recovered from the last
    /// imported DOCX, surfaced in the right-hand Review panel.
    review_items: Vec<mdall_core::docx_review::ReviewItem>,
    show_review_panel: bool,
    /// Comment-authoring dialog (right-click selection → Add comment).
    comment_dialog: crate::ui::state::CommentDialog,
    /// Anchor text of the review item currently hovered in the panel (transient
    /// frame in the editor). Cleared each frame when nothing is hovered.
    review_hl: Option<String>,
    /// Anchor text of the review item clicked/marked in the panel (persistent
    /// frame in the editor until another item is clicked or it is toggled off).
    review_mark: Option<String>,
    /// Last non-empty text selection (char range), kept so "Add comment" survives
    /// the right-click collapsing the live selection. Cleared on a plain click.
    last_sel: Option<(usize, usize)>,
    /// Module system (spell engine + dictionary/language/style management).
    module_open: bool,
    module_tab: u8,
    /// Loaded spell-check engine (one language). None until a dictionary is added.
    spell: Option<mdall_core::spell::SpellChecker>,
    spell_enabled: bool,
    show_spelling_panel: bool,
    /// In-flight opt-in dictionary download (background thread + shared status).
    dict_dl: Option<std::sync::Arc<std::sync::Mutex<crate::ui::commands::DictDownload>>>,
    /// Last download status, shown in the Module panel (download is otherwise silent).
    dict_status: String,
    /// Misspellings in the current source (recomputed when the source changes).
    spell_issues: Vec<mdall_core::spell::Misspelling>,
    /// Per-word suggestion cache (suggest() is costly; computed once per word).
    spell_sugg_cache: std::collections::HashMap<String, Vec<String>>,
    /// Application UI language tag (i18n; `t(key)` wiring lands in a later step).
    app_lang: String,
    /// PDF engine choice: true = Native converter (Typst), false = General.
    pdf_native: bool,
    /// Source code-editor view: show the line-number gutter.
    source_line_numbers: bool,
    /// Source code-editor view: soft-wrap long lines instead of horizontal scroll.
    source_wrap: bool,
    /// WYSIWYG editing toolbar: minified (default) vs the full detailed bar.
    toolbar_minified: bool,
    /// Print layout: show a page number in each sheet's footer.
    show_page_numbers: bool,
    /// Print layout: optional header / footer text repeated on every page.
    header_text: String,
    footer_text: String,
    /// Print layout: A4 sheet fill colour and an optional frame around each sheet.
    page_color: [u8; 3],
    page_frame: bool,
    /// Editing-toolbar icon style (accessibility): sober / colored / high contrast.
    icon_set: crate::ui::icons::IconSet,
    /// Accessibility (WCAG): global UI scale, high contrast, reduced motion, large targets.
    ui_scale: f32,
    a11y_high_contrast: bool,
    a11y_reduced_motion: bool,
    a11y_large_targets: bool,
    /// Go-to-line popup (Source editor): open flag + the line number being typed.
    goto_line_open: bool,
    goto_line_input: String,
    /// Last-persisted preferences snapshot, to detect changes and re-save.
    prefs_snapshot: crate::settings::Settings,
    blocks: Vec<editor::DocumentBlock>,
    segments_dirty: bool,
    /// Document-level undo/redo: full-source snapshots (egui only undoes within a
    /// single focused field, which is useless across the segmented WYSIWYG flow).
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
    /// The source as of the previous frame, to detect edits for the undo capture.
    prev_source: String,
    font_size: f32,
    /// Size (pt) the toolbar applies to the SELECTION via a font-size span,
    /// independent of `font_size` (the editor's global base size).
    apply_size: f32,
    zoom_level: f32,
    base_ppp: f32,
    text_color: [u8; 3],
    highlight_color: [u8; 3],
    font_list: Vec<(String, String)>,
    selected_font: String,
    fonts_scanned: bool,
    /// Cache: equation LaTeX string → rendered egui texture (PNG via Typst).
    eq_tex_cache: HashMap<String, egui::TextureHandle>,
    export_dialog: ExportDialog,
    conversion_hub: ConversionHub,
    conversion_settings: ConversionSettings,
    /// Set to true to request keyboard focus on the source editor next frame.
    request_source_focus: bool,
    /// When Some((pos, anchor)), the source editor cursor is moved there next frame.
    pending_cursor: Option<(usize, usize)>,
    // ── Search state ─────────────────────────────────────────────────────────
    search_case_sensitive: bool,
    /// true = opened with Ctrl+H (replace row visible); false = Ctrl+F (find only)
    search_show_replace: bool,
    /// Byte positions of all match starts in `source`.  Recomputed when query or source changes.
    search_matches: Vec<usize>,
    /// Index into `search_matches` for the currently highlighted match.
    search_match_idx: usize,
    /// Format state at the WYSIWYG cursor (bold/italic/code/heading level).
    /// Refreshed every frame in show_wysiwyg_editor; used by toolbar buttons.
    wysiwyg_fmt: WysiwygFormatState,
    // ── Modernization / UX state (light Heimdall is the default identity) ─────
    /// Editor rendering mode: segmented flow (default) or block model.
    editor_mode: EditorMode,
    /// false = light Heimdall theme (default identity); true = warm dark theme (opt-in).
    dark_mode: bool,
    /// Transient bottom-right notifications, auto-dismissed each frame.
    toasts: Vec<Toast>,
    /// Ctrl+K command palette overlay visibility.
    command_palette_open: bool,
    /// Fuzzy filter query for the command palette.
    palette_query: String,
    /// Options panel (theme, editor mode, default font, conversion) visibility.
    options_open: bool,
    /// Cross-block caret (ADR-002 §7): pending focus jump to a neighbour region
    /// `(target TextEdit id, where to place the caret)`, applied next frame.
    region_focus_req: Option<(egui::Id, crate::ui::editor::CaretAim)>,
    /// Last caret char index for the focused rich region `(id, index)`; lets us
    /// detect an arrow key that egui clamped at an edge (caret did not move).
    region_caret_prev: Option<(egui::Id, usize)>,
    /// Left/right page margins in pixels, adjustable by dragging the ruler
    /// handles (like a word processor). Default ≈ 1 inch at 96 DPI.
    margin_left: f32,
    margin_right: f32,
    /// Per-block `MappedBlock` cache, keyed by a hash of the block source, to
    /// avoid re-parsing every text region every frame. Content-keyed, so an edit
    /// produces a new key; bounded in size and verified against the stored source.
    map_cache: std::collections::HashMap<u64, (String, crate::wysiwyg_map::MappedBlock)>,
    /// Absolute source byte range of an inline equation clicked in a region this
    /// frame; resolved after the block loop to open the LaTeX editor.
    pending_inline_eq: Option<std::ops::Range<usize>>,
    /// Custom LaTeX macros (`\newcommand`/`\def`) collected from the document
    /// source. Rebuilt when the source changes; applied to every equation before
    /// rendering so preamble macros like `\sket` expand correctly.
    macro_table: mdall_core::latex_macros::MacroTable,
}

impl Default for MdApp {
    fn default() -> Self {
        Self {
            source: String::new(),
            current_file: None,
            modified: false,
            show_metadata: false,
            show_search: false,
            search_query: String::new(),
            replace_query: String::new(),
            status_msg: "Ready".into(),
            cursor_pos: 0,
            selection_anchor: 0,
            meta: export::PdfMetadata::default(),
            logo_tex: None,
            view_mode: ViewMode::Converter,
            eq_editor: EquationEditor {
                visible: false,
                latex: String::new(),
                index: 0,
                is_inline: false,
                inline_block_range: 0..0,
                inline_delim_open: "$".into(),
                inline_delim_close: "$".into(),
                inline_orig_latex: String::new(),
                inline_run_idx: 0,
            },
            link_dialog: LinkDialog {
                visible: false,
                text: String::new(),
                url: String::new(),
                is_image: false,
            },
            image_dialog: crate::ui::state::ImageDialog {
                visible: false,
                alt: String::new(),
                url: String::new(),
                width: String::new(),
                align: crate::ui::editor::ImgAlign::None,
                replace: 0..0,
            },
            table_dialog: crate::ui::state::TableDialog::default(),
            svg_editor: crate::ui::svg_editor::SvgEditor::default(),
            selected_image: None,
            region_live: None,
            append_buf: String::new(),
            page_breaks: Vec::new(),
            page_count: 1,
            doc_selection: None,
            block_hits: Vec::new(),
            doc_drag_anchor: None,
            doc_dragging: false,
            review_items: Vec::new(),
            show_review_panel: false,
            comment_dialog: crate::ui::state::CommentDialog {
                visible: false,
                anchor: String::new(),
                body: String::new(),
            },
            review_hl: None,
            review_mark: None,
            last_sel: None,
            module_open: false,
            module_tab: 0,
            spell: None,
            spell_enabled: false,
            show_spelling_panel: false,
            dict_dl: None,
            dict_status: String::new(),
            spell_issues: Vec::new(),
            spell_sugg_cache: std::collections::HashMap::new(),
            app_lang: "en".into(),
            pdf_native: false,
            source_line_numbers: true,
            source_wrap: false,
            toolbar_minified: true,
            show_page_numbers: true,
            header_text: String::new(),
            footer_text: String::new(),
            page_color: [255, 255, 255],
            page_frame: false,
            icon_set: crate::ui::icons::IconSet::Sober,
            ui_scale: 1.0,
            a11y_high_contrast: false,
            a11y_reduced_motion: false,
            a11y_large_targets: false,
            goto_line_open: false,
            goto_line_input: String::new(),
            prefs_snapshot: crate::settings::Settings::default(),
            blocks: Vec::new(),
            segments_dirty: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            prev_source: String::new(),
            font_size: 14.0,
            apply_size: 18.0,
            zoom_level: 1.0,
            base_ppp: 0.0,
            text_color: [0, 0, 0],
            highlight_color: [255, 255, 0],
            font_list: Vec::new(),
            selected_font: "Cambria".into(),
            fonts_scanned: false,
            eq_tex_cache: HashMap::new(),
            export_dialog: ExportDialog { visible: false },
            conversion_hub: ConversionHub::default(),
            conversion_settings: ConversionSettings::default(),
            request_source_focus: false,
            pending_cursor: None,
            search_case_sensitive: false,
            search_show_replace: false,
            search_matches: Vec::new(),
            search_match_idx: 0,
            wysiwyg_fmt: WysiwygFormatState::default(),
            editor_mode: EditorMode::default(),
            dark_mode: false,
            toasts: Vec::new(),
            command_palette_open: false,
            palette_query: String::new(),
            options_open: false,
            region_focus_req: None,
            region_caret_prev: None,
            margin_left: 72.0,
            margin_right: 72.0,
            map_cache: std::collections::HashMap::new(),
            pending_inline_eq: None,
            macro_table: mdall_core::latex_macros::MacroTable::new(),
        }
    }
}

impl MdApp {
    /// Apply the document's custom macros and shared sanitization to a raw
    /// equation string, producing render-ready LaTeX. Used by every on-screen
    /// equation path (Typst texture, layout-job fallback, equation editor).
    fn prepare_latex(&self, raw: &str) -> String {
        mdall_core::latex_macros::expand_and_sanitize(raw, &self.macro_table)
    }

    /// Snapshot the persisted preferences from the live app state.
    fn current_prefs(&self) -> crate::settings::Settings {
        crate::settings::Settings {
            app_lang: self.app_lang.clone(),
            dark_mode: self.dark_mode,
            pdf_native: self.pdf_native,
            source_line_numbers: self.source_line_numbers,
            source_wrap: self.source_wrap,
            toolbar_minified: self.toolbar_minified,
            show_page_numbers: self.show_page_numbers,
            header_text: self.header_text.clone(),
            footer_text: self.footer_text.clone(),
            page_color: self.page_color,
            page_frame: self.page_frame,
            icon_set: self.icon_set.as_key().to_string(),
            ui_scale: self.ui_scale,
            a11y_high_contrast: self.a11y_high_contrast,
            a11y_reduced_motion: self.a11y_reduced_motion,
            a11y_large_targets: self.a11y_large_targets,
        }
    }
}

impl eframe::App for MdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply the chosen UI language for every t(key) call this frame.
        crate::i18n::set_language(&self.app_lang);
        // Persist prefs when language/theme/PDF-engine changed (vs last snapshot).
        let cur_prefs = self.current_prefs();
        if cur_prefs != self.prefs_snapshot {
            mdall_core::export::set_native_pdf(cur_prefs.pdf_native);
            crate::ui::icons::set_icon_set(self.icon_set);
            crate::settings::save(&cur_prefs);
            self.prefs_snapshot = cur_prefs;
        }
        // Zoom
        if self.base_ppp == 0.0 {
            self.base_ppp = ctx.pixels_per_point();
        }
        ctx.set_pixels_per_point(self.base_ppp * self.zoom_level * self.ui_scale);

        // Apply the active theme each frame so the toggle takes effect live.
        // Light warm Heimdall is the default identity; dark is opt-in.
        ctx.set_visuals(theme::current_visuals(self.dark_mode));

        // Accessibility (WCAG): high contrast over the active theme, plus motion
        // and target-size tweaks set explicitly each frame so toggling restores.
        if self.a11y_high_contrast {
            let mut v = ctx.style().visuals.clone();
            theme::apply_high_contrast(&mut v, self.dark_mode);
            ctx.set_visuals(v);
        }
        {
            let mut s = (*ctx.style()).clone();
            s.animation_time = if self.a11y_reduced_motion { 0.0 } else { 0.083 };
            let (pad, h) = if self.a11y_large_targets {
                (egui::vec2(14.0, 9.0), 32.0)
            } else {
                (egui::vec2(10.0, 6.0), 24.0)
            };
            s.spacing.button_padding = pad;
            s.spacing.interact_size.y = h;
            ctx.set_style(s);
        }
        theme::apply_modern_style(ctx);

        self.load_logo(ctx);
        self.scan_fonts();
        // Snapshot the previous frame's source for undo before this frame's
        // shortcuts (so Ctrl+Z sees the edit that just landed).
        self.capture_undo_snapshot();
        self.handle_shortcuts(ctx);
        // Editor-view image drops (the converter home handles its own drops).
        if self.view_mode != ViewMode::Converter {
            self.handle_editor_file_drops(ctx);
        }

        // Top bar: on the converter home it collapses to a thin hint strip and
        // reveals the view switcher + menus only when the pointer nears the top
        // edge (or a menu is open). In editor modes it is always the full bar.
        let on_home = self.view_mode == ViewMode::Converter;
        let reveal_top = if on_home {
            let near_top = ctx.input(|i| i.pointer.latest_pos()).map_or(false, |p| p.y < 40.0);
            near_top || ctx.memory(|m| m.any_popup_open())
        } else {
            true
        };
        self.show_menu_bar(ctx, on_home, reveal_top);

        // The editor formatting toolbar is irrelevant on the converter home.
        // Source -> code toolbar; Editor -> WYSIWYG; Split renders a toolbar per
        // pane inside its own panels (see the ViewMode::Split arm below).
        if !on_home {
            match self.view_mode {
                ViewMode::Source => self.show_source_toolbar(ctx),
                ViewMode::Split => {}
                _ => self.show_toolbar(ctx),
            }
        }

        // Status bar
        self.show_status_bar(ctx);

        // ── Search / Replace bar (multiline) ─────────────────────────────────
        self.show_search_bar(ctx);

        // Rebuild semantic document blocks + prune stale equation textures.
        if self.segments_dirty {
            self.blocks = editor::parse_document(&self.source);
            self.segments_dirty = false;

            // Recompute spelling issues when the document changes (cheap: hash
            // lookups). Suggestions are computed lazily/cached, not here.
            self.spell_issues = if self.spell_enabled {
                self.spell.as_ref().map(|sc| sc.check_document(&self.source)).unwrap_or_default()
            } else {
                Vec::new()
            };

            // Rebuild the custom-macro table from the whole document source so
            // preamble macros (e.g. \newcommand{\sket}...) expand in equations.
            self.macro_table = mdall_core::latex_macros::MacroTable::collect(&self.source);

            // Collect all latex strings still referenced (display + inline)
            let mut live: std::collections::HashSet<String> = std::collections::HashSet::new();
            for b in &self.blocks {
                match &b.kind {
                    editor::BlockKind::DisplayEquation { latex, .. } => {
                        live.insert(latex.clone());
                    }
                    editor::BlockKind::Paragraph => {
                        let se  = b.source_range.end.min(self.source.len());
                        let raw = &self.source[b.source_range.start..se];
                        if inline_math::needs_reparse(raw) {
                            for run in &inline_math::split_inline(raw) {
                                if let inline_math::InlineRun::Equation { latex, .. } = run {
                                    live.insert(latex.clone());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            self.eq_tex_cache.retain(|k, _| live.contains(k));
        }

        // Install the document's macro table for this thread so inline-math
        // rendering (which goes through the content-keyed block cache and the
        // free fn latex_to_unicode) can expand custom macros. Set every frame
        // because export paths may temporarily install a different table.
        mdall_core::latex_macros::set_active_macros(self.macro_table.clone());

        // Review panel (tracked changes + comments from an imported DOCX). Added
        // before the central content so the right SidePanel reserves its width.
        self.poll_dict_download(ctx);
        // The review (tracked-changes) and spelling panels are editor features -
        // never on the converter home, even if toggled on in a prior session.
        if !on_home {
            self.render_review_panel(ctx);
            self.render_spelling_panel(ctx);
        }

        // Main content
        match self.view_mode {
            ViewMode::Converter => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default().fill(theme::desktop_bg(self.dark_mode)))
                    .show(ctx, |ui| {
                        self.show_converter_hub(ui, ctx);
                    });
            }
            ViewMode::Source => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if self.source.is_empty() && self.current_file.is_none() {
                        self.show_welcome(ui);
                    } else {
                        self.show_source_editor(ui);
                    }
                });
            }
            ViewMode::Editor => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if self.source.is_empty() && self.current_file.is_none() {
                        self.show_welcome(ui);
                    } else {
                        self.show_wysiwyg_editor(ui);
                    }
                });
            }
            ViewMode::Split => {
                // Each pane carries its own toolbar: the code toolbar above the
                // source editor (left), the WYSIWYG toolbar above the rendered
                // editor (right), via nested panels (show_inside).
                let tb_frame = egui::Frame::default()
                    .fill(theme::panel_bg(self.dark_mode))
                    .inner_margin(egui::Margin { left: 6.0, right: 6.0, top: 3.0, bottom: 3.0 });
                let empty = self.source.is_empty() && self.current_file.is_none();

                egui::SidePanel::left("source_panel")
                    .resizable(true)
                    .default_width(ctx.screen_rect().width() * 0.5)
                    .min_width(200.0)
                    .show(ctx, |ui| {
                        egui::TopBottomPanel::top("split_source_toolbar")
                            .frame(tb_frame)
                            .show_inside(ui, |ui| self.source_toolbar_ui(ui));
                        egui::CentralPanel::default()
                            .frame(egui::Frame::none())
                            .show_inside(ui, |ui| {
                                if empty { self.show_welcome(ui); } else { self.show_source_editor(ui); }
                            });
                    });
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::TopBottomPanel::top("split_render_toolbar")
                        .frame(tb_frame)
                        .show_inside(ui, |ui| self.toolbar_ui(ui));
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show_inside(ui, |ui| {
                            if empty { self.show_welcome(ui); } else { self.show_wysiwyg_editor(ui); }
                        });
                });
            }
        }

        if self.module_open { self.show_module_window(ctx); }
        self.show_comment_dialog(ctx);
        if self.show_metadata { self.show_metadata_window(ctx); }
        if self.eq_editor.visible { self.show_equation_editor(ctx); }
        if self.link_dialog.visible { self.show_link_dialog(ctx); }
        if self.image_dialog.visible { self.show_image_dialog(ctx); }
        if self.table_dialog.visible { self.show_table_dialog(ctx); }
        if self.svg_editor.visible { self.show_svg_editor(ctx); }
        if self.export_dialog.visible { self.show_export_dialog(ctx); }

        // Options panel, command palette overlay, and transient toasts (top-most).
        self.show_options_panel(ctx);
        self.show_goto_line_dialog(ctx);
        self.show_command_palette(ctx);
        self.show_toasts(ctx);
    }
}

impl MdApp {
    fn load_logo(&mut self, ctx: &egui::Context) {
        if self.logo_tex.is_some() { return; }
        let img = match image::load_from_memory(include_bytes!("../assets/Logo.png")) {
            Ok(img) => img.into_rgba8(),
            Err(_) => return, // no logo texture rather than a startup panic
        };
        let size = [img.width() as usize, img.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        self.logo_tex = Some(ctx.load_texture("logo", color_image, egui::TextureOptions::LINEAR));
    }

    fn scan_fonts(&mut self) {
        if self.fonts_scanned { return; }
        self.fonts_scanned = true;

        let internal = [
            ("Cambria", "cambria.ttc"),
            ("Times New Roman", "times.ttf"),
            ("Georgia", "georgia.ttf"),
            ("Calibri", "calibri.ttf"),
            ("Arial", "arial.ttf"),
            ("Segoe UI", "segoeui.ttf"),
            ("Consolas", "consola.ttf"),
            ("Verdana", "verdana.ttf"),
        ];

        for (name, file) in &internal {
            let path = format!("C:\\Windows\\Fonts\\{}", file);
            if std::path::Path::new(&path).exists() {
                self.font_list.push((name.to_string(), path));
            }
        }

        self.font_list.push(("---".to_string(), String::new()));

        let mut system: Vec<(String, String)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir("C:\\Windows\\Fonts") {
            let internal_files: Vec<&str> = internal.iter().map(|(_, f)| *f).collect();
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                let lower = fname.to_lowercase();
                if (lower.ends_with(".ttf") || lower.ends_with(".ttc")) && !internal_files.contains(&lower.as_str()) {
                    let display = fname.trim_end_matches(".ttf").trim_end_matches(".ttc")
                        .trim_end_matches(".TTF").trim_end_matches(".TTC").to_string();
                    let path = format!("C:\\Windows\\Fonts\\{}", fname);
                    system.push((display, path));
                }
            }
        }
        system.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        self.font_list.extend(system);
    }

    fn apply_font_change(&mut self, ctx: &egui::Context) {
        let path = self.font_list.iter()
            .find(|(name, _)| name == &self.selected_font)
            .map(|(_, p)| p.clone());
        // Rebuild the FULL font set (math + named families intact) with the chosen
        // face on top, so changing the font never drops the equation fonts or
        // panics on a now-missing family (CambriaBold / CambriaMath).
        ctx.set_fonts(build_font_definitions(path.as_deref().map(std::path::Path::new)));
    }







}

fn char_to_byte_index(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn byte_to_char_index(s: &str, byte_index: usize) -> usize {
    // Floor to a char boundary first - slicing at an arbitrary byte (e.g. inside
    // a multi-byte 'é') would panic. Mapped offsets can land mid-char.
    let mut i = byte_index.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    s[..i].chars().count()
}

/// Detect which inline formatting is active at `byte_offset` in `source`.
///
/// Finds the block containing the offset, then parses its inline markdown
/// with pulldown-cmark to determine if the cursor is inside bold/italic/code/etc.
/// Also detects heading level if the block is a heading.
fn detect_format_at(
    byte_offset: usize,
    source: &str,
    segments: &[editor::DocumentBlock],
) -> WysiwygFormatState {
    let mut fmt = WysiwygFormatState::default();

    // Find the block that contains the cursor
    let block = segments.iter().find(|b| {
        b.source_range.start <= byte_offset && byte_offset <= b.source_range.end
    });
    let block = match block { Some(b) => b, None => return fmt };

    // Heading: report the level, no inline parse needed
    if let editor::BlockKind::Heading(n) = block.kind {
        fmt.heading = n;
        return fmt;
    }

    // For paragraph / list / blockquote: parse inline formatting
    let b_start = block.source_range.start.min(source.len());
    let b_end   = block.source_range.end.min(source.len());
    if b_start >= b_end { return fmt; }

    let block_text = &source[b_start..b_end];
    let local = byte_offset.saturating_sub(b_start);  // offset within this block

    // Use pulldown-cmark offset iterator to collect formatting span ranges
    let mut opts = pulldown_cmark::Options::empty();
    opts.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);

    let mut bold_start:   Option<usize> = None;
    let mut italic_start: Option<usize> = None;
    let mut strike_start: Option<usize> = None;

    let mut bold_ranges:   Vec<std::ops::Range<usize>> = Vec::new();
    let mut italic_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut strike_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut code_ranges:   Vec<std::ops::Range<usize>> = Vec::new();

    for (event, range) in
        pulldown_cmark::Parser::new_ext(block_text, opts).into_offset_iter()
    {
        use pulldown_cmark::{Event, Tag, TagEnd};
        match event {
            Event::Start(Tag::Strong)         => { bold_start   = Some(range.start); }
            Event::End(TagEnd::Strong)        => {
                if let Some(s) = bold_start.take() { bold_ranges.push(s..range.end); }
            }
            Event::Start(Tag::Emphasis)       => { italic_start = Some(range.start); }
            Event::End(TagEnd::Emphasis)      => {
                if let Some(s) = italic_start.take() { italic_ranges.push(s..range.end); }
            }
            Event::Start(Tag::Strikethrough)  => { strike_start = Some(range.start); }
            Event::End(TagEnd::Strikethrough) => {
                if let Some(s) = strike_start.take() { strike_ranges.push(s..range.end); }
            }
            Event::Code(_) => { code_ranges.push(range); }
            _ => {}
        }
    }

    fmt.bold          = bold_ranges.iter().any(|r| r.contains(&local));
    fmt.italic        = italic_ranges.iter().any(|r| r.contains(&local));
    fmt.strikethrough = strike_ranges.iter().any(|r| r.contains(&local));
    fmt.code          = code_ranges.iter().any(|r| r.contains(&local));

    fmt
}
