//! File operations, editing commands, keyboard shortcuts and search.
//! Methods on MdApp, extracted from main.rs.

use eframe::egui;
use crate::MdApp;
use crate::ViewMode;
use crate::ui::state::LinkDialog;
use crate::{char_to_byte_index, byte_to_char_index};
use mdall_core::{export, source_embed};
use crate::i18n::t;

/// The composable inline character formats handled by the normalizing toggle.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineFmt { Bold, Italic, Strike, Code, Underline }

#[derive(Default, Clone, Copy)]
struct InlineFlags { bold: bool, italic: bool, strike: bool, code: bool, underline: bool }

impl InlineFlags {
    fn get(&self, f: InlineFmt) -> bool {
        match f {
            InlineFmt::Bold => self.bold,
            InlineFmt::Italic => self.italic,
            InlineFmt::Strike => self.strike,
            InlineFmt::Code => self.code,
            InlineFmt::Underline => self.underline,
        }
    }
    fn set(&mut self, f: InlineFmt, v: bool) {
        match f {
            InlineFmt::Bold => self.bold = v,
            InlineFmt::Italic => self.italic = v,
            InlineFmt::Strike => self.strike = v,
            InlineFmt::Code => self.code = v,
            InlineFmt::Underline => self.underline = v,
        }
    }
}

fn inline_markers(f: InlineFmt) -> (&'static str, &'static str) {
    match f {
        InlineFmt::Bold => ("**", "**"),
        InlineFmt::Italic => ("*", "*"),
        InlineFmt::Strike => ("~~", "~~"),
        InlineFmt::Code => ("`", "`"),
        InlineFmt::Underline => ("<u>", "</u>"),
    }
}

/// (open, close, format). Order matters for absorb/peel: ** is tried before *.
const INLINE_MARKERS: &[(&str, &str, InlineFmt)] = &[
    ("**", "**", InlineFmt::Bold),
    ("~~", "~~", InlineFmt::Strike),
    ("<u>", "</u>", InlineFmt::Underline),
    ("*", "*", InlineFmt::Italic),
    ("`", "`", InlineFmt::Code),
];

/// Read the inline formats wrapping `source[bs..be]`: absorb matching marker pairs
/// immediately outside it, then peel pairs the range itself included. Returns the
/// detected flags, the expanded byte range, and the bare inner text.
fn collapse_inline(source: &str, mut bs: usize, mut be: usize) -> (InlineFlags, usize, usize, String) {
    let mut flags = InlineFlags::default();
    loop {
        let mut changed = false;
        for &(op, cl, f) in INLINE_MARKERS {
            if bs >= op.len()
                && be + cl.len() <= source.len()
                && source.is_char_boundary(bs - op.len())
                && source.is_char_boundary(be + cl.len())
                && &source[bs - op.len()..bs] == op
                && &source[be..be + cl.len()] == cl
            {
                // A lone "*" must not grab the inner "*" of a "**" (bold).
                if op == "*"
                    && (source.as_bytes().get(bs.wrapping_sub(2)) == Some(&b'*')
                        || source.as_bytes().get(be + 1) == Some(&b'*'))
                {
                    continue;
                }
                bs -= op.len();
                be += cl.len();
                flags.set(f, true);
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    let mut plain = source[bs..be].to_string();
    loop {
        let mut changed = false;
        for &(op, cl, f) in INLINE_MARKERS {
            if plain.len() >= op.len() + cl.len() && plain.starts_with(op) && plain.ends_with(cl) {
                if op == "*"
                    && (plain.as_bytes().get(op.len()) == Some(&b'*')
                        || plain[..plain.len() - cl.len()].ends_with('*'))
                {
                    continue;
                }
                plain = plain[op.len()..plain.len() - cl.len()].to_string();
                flags.set(f, true);
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    (flags, bs, be, plain)
}

/// Toggle one inline format on `source[bs0..be0]` by NORMALIZING: read the real
/// format set, flip `fmt`, and re-emit one canonical clean span. Never accumulates
/// markers; bold + italic compose to `***`. Returns the new source and the byte
/// range of the content between the new markers. Pure - unit-tested below.
fn normalize_inline_toggle(
    source: &str,
    bs0: usize,
    be0: usize,
    fmt: InlineFmt,
) -> (String, std::ops::Range<usize>) {
    let (top, tcl) = inline_markers(fmt);
    let (mut flags, mut bs, mut be, mut plain) = collapse_inline(source, bs0, be0);
    let on = flags.get(fmt);

    if !on {
        // ADD with MERGE: swallow stray markers of THIS format adjacent to the
        // range, re-collapse, then strip any of its markers left inside, so a partly
        // formatted phrase becomes one clean span ("**A** B" -> "**A B**").
        while bs >= top.len()
            && source.is_char_boundary(bs - top.len())
            && &source[bs - top.len()..bs] == top
        {
            if top == "*" && source.as_bytes().get(bs.wrapping_sub(2)) == Some(&b'*') {
                break;
            }
            bs -= top.len();
        }
        while be + tcl.len() <= source.len()
            && source.is_char_boundary(be + tcl.len())
            && &source[be..be + tcl.len()] == tcl
        {
            if tcl == "*" && source.as_bytes().get(be + tcl.len()) == Some(&b'*') {
                break;
            }
            be += tcl.len();
        }
        let (f2, nb, ne, p2) = collapse_inline(source, bs, be);
        flags = f2;
        bs = nb;
        be = ne;
        plain = p2;
        if top.len() >= 2 || fmt == InlineFmt::Code || fmt == InlineFmt::Underline {
            plain = plain.replace(top, "");
            if tcl != top {
                plain = plain.replace(tcl, "");
            }
        }
    }

    flags.set(fmt, !on);

    // Re-emit canonically (outer -> inner): underline, strike, bold, italic, code.
    let (mut open, mut close) = (String::new(), String::new());
    for &(op, cl, f) in &[
        ("<u>", "</u>", InlineFmt::Underline),
        ("~~", "~~", InlineFmt::Strike),
        ("**", "**", InlineFmt::Bold),
        ("*", "*", InlineFmt::Italic),
        ("`", "`", InlineFmt::Code),
    ] {
        if flags.get(f) {
            open.push_str(op);
            close = format!("{}{}", cl, close);
        }
    }
    let content_start = bs + open.len();
    let content_end = content_start + plain.len();
    let mut out = String::with_capacity(source.len() + open.len() + close.len());
    out.push_str(&source[..bs]);
    out.push_str(&open);
    out.push_str(&plain);
    out.push_str(&close);
    out.push_str(&source[be..]);
    (out, content_start..content_end)
}

/// Inject or replace `text-align:<align>` in the opening tag of a `<div ...>`
/// block, keeping all its other styles. Used by block alignment so a styled box
/// gains its alignment in place instead of being wrapped in a nested div.
fn set_div_text_align(block: &str, align: &str) -> Option<String> {
    let open_end = block.find('>')?;
    let tag = &block[..open_end];
    let rest = &block[open_end..];
    let new_tag = if let Some(sp) = tag.to_ascii_lowercase().find("style=\"") {
        let val_start = sp + "style=\"".len();
        let val_end = val_start + tag[val_start..].find('"')?;
        let style = &tag[val_start..val_end];
        let new_style = if let Some(tp) = style.to_ascii_lowercase().find("text-align") {
            let after = &style[tp..];
            let colon = after.find(':')?;
            let semi = after[colon..].find(';').map(|i| colon + i).unwrap_or(after.len());
            format!("{}text-align:{}{}", &style[..tp], align, &after[semi..])
        } else {
            format!("{}; text-align:{}", style.trim_end().trim_end_matches(';').trim_end(), align)
        };
        format!("{}{}{}", &tag[..val_start], new_style, &tag[val_end..])
    } else {
        format!("<div style=\"text-align:{}\"{}", align, &tag["<div".len()..])
    };
    Some(format!("{}{}", new_tag, rest))
}

#[cfg(test)]
mod block_align_tests {
    use super::set_div_text_align;

    #[test]
    fn injects_into_existing_style_keeping_it() {
        let div = "<div style=\"background:#eef; border:1px solid #abc\">\nhi\n</div>";
        let out = set_div_text_align(div, "center").unwrap();
        assert!(out.contains("background:#eef"), "kept existing style: {out}");
        assert!(out.contains("text-align:center"), "added alignment: {out}");
        assert!(out.ends_with(">\nhi\n</div>"), "body untouched: {out}");
    }

    #[test]
    fn replaces_existing_text_align() {
        let div = "<div style=\"text-align:left; color:#111\">\nhi\n</div>";
        let out = set_div_text_align(div, "right").unwrap();
        assert!(out.contains("text-align:right") && !out.contains("text-align:left"), "{out}");
        assert!(out.contains("color:#111"), "{out}");
    }

    #[test]
    fn adds_style_attr_when_absent() {
        let div = "<div class=\"frame\">\nhi\n</div>";
        let out = set_div_text_align(div, "center").unwrap();
        assert!(out.contains("text-align:center") && out.contains("class=\"frame\""), "{out}");
    }
}

#[cfg(test)]
mod inline_toggle_tests {
    use super::{normalize_inline_toggle, InlineFmt};

    /// Toggle `fmt` over the byte span of the first `sel` found in `src`.
    fn tog(src: &str, sel: &str, fmt: InlineFmt) -> String {
        let bs = src.find(sel).expect("selection not found");
        normalize_inline_toggle(src, bs, bs + sel.len(), fmt).0
    }

    #[test]
    fn bold_plain_text() {
        assert_eq!(tog("a X b", "X", InlineFmt::Bold), "a **X** b");
    }

    #[test]
    fn bold_off_removes_markers() {
        assert_eq!(tog("a **X** b", "X", InlineFmt::Bold), "a X b");
    }

    #[test]
    fn italic_on_bold_composes_to_triple_star() {
        assert_eq!(tog("a **X** b", "X", InlineFmt::Italic), "a ***X*** b");
    }

    #[test]
    fn italic_toggled_twice_returns_to_bold_no_accumulation() {
        let once = tog("a **X** b", "X", InlineFmt::Italic);
        assert_eq!(once, "a ***X*** b");
        let twice = tog(&once, "X", InlineFmt::Italic);
        assert_eq!(twice, "a **X** b", "italic on then off must return to bold");
        let thrice = tog(&twice, "X", InlineFmt::Italic);
        assert_eq!(thrice, "a ***X*** b", "third press re-adds italic, still no stacking");
    }

    #[test]
    fn bold_merges_a_partly_bold_phrase() {
        assert_eq!(tog("**A** B", "A** B", InlineFmt::Bold), "**A B**");
    }

    #[test]
    fn removing_bold_keeps_italic() {
        assert_eq!(tog("***Word***", "Word", InlineFmt::Bold), "*Word*");
    }

    #[test]
    fn unicode_selection_is_safe() {
        assert_eq!(tog("cafe X", "X", InlineFmt::Bold), "cafe **X**");
        // multi-byte content inside the selection
        assert_eq!(tog("a deja b", "deja", InlineFmt::Italic), "a *deja* b");
    }
}

#[cfg(test)]
mod value_span_tests {
    //! Colour / highlight / font-size spans must REPLACE a same-class span on the
    //! selection, never nest a new one (the source-corruption bug the user hit:
    //! re-colouring stacked `<span><mark><span>...`).
    fn app_sel(src: &str, sel: &str) -> crate::MdApp {
        let mut app = crate::MdApp::default();
        app.source = src.to_string();
        let bs = src.find(sel).expect("selection not found");
        app.selection_anchor = src[..bs].chars().count();
        app.cursor_pos = app.selection_anchor + sel.chars().count();
        app
    }

    #[test]
    fn colour_wraps_then_replaces_then_toggles_off() {
        let mut a = app_sel("hello world", "world");
        a.wrap_value_span("span", "color", "#ff0000");
        assert_eq!(a.source, "hello <span style=\"color:#ff0000\">world</span>");
        // Re-colour the (now inner-selected) word: REPLACE, never nest.
        a.wrap_value_span("span", "color", "#0000ff");
        assert_eq!(a.source, "hello <span style=\"color:#0000ff\">world</span>",
            "recolour must replace, not stack <span><span>");
        // Same colour again -> toggle off.
        a.wrap_value_span("span", "color", "#0000ff");
        assert_eq!(a.source, "hello world", "re-applying the same colour removes it");
    }

    #[test]
    fn highlight_replaces_not_nests() {
        let mut a = app_sel("a Z b", "Z");
        a.wrap_value_span("mark", "background", "#ffff00");
        assert_eq!(a.source, "a <mark style=\"background:#ffff00\">Z</mark> b");
        a.wrap_value_span("mark", "background", "#00ff00");
        assert_eq!(a.source, "a <mark style=\"background:#00ff00\">Z</mark> b",
            "re-highlight must replace, not stack <mark><mark>");
    }

    #[test]
    fn colour_and_highlight_coexist() {
        // Different style classes are both allowed on the same text.
        let mut a = app_sel("a Z b", "Z");
        a.wrap_value_span("mark", "background", "#ffff00");
        // Selection is now the inner "Z"; applying colour must NOT drop the mark.
        a.wrap_value_span("span", "color", "#ff0000");
        assert!(a.source.contains("background:#ffff00"), "highlight kept: {}", a.source);
        assert!(a.source.contains("color:#ff0000"), "colour added: {}", a.source);
    }

    #[test]
    fn whole_span_selected_in_source_view_replaces() {
        let mut a = app_sel("x <span style=\"color:#111111\">Y</span> z",
                            "<span style=\"color:#111111\">Y</span>");
        a.wrap_value_span("span", "color", "#222222");
        assert_eq!(a.source, "x <span style=\"color:#222222\">Y</span> z",
            "selecting the whole span and recolouring replaces it");
    }
}

/// Shared status of a background dictionary download (thread → UI poll).
pub(crate) struct DictDownload {
    pub lang: String,
    pub done: bool,
    pub error: Option<String>,
}

/// `<exe-dir>/dictionaries/` - where bundled and downloaded dictionaries live.
fn dict_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("dictionaries")))
}

/// True when both `{lang}.dic` and `{lang}.aff` are present on disk.
fn dict_downloaded(lang: &str) -> bool {
    dict_dir()
        .map(|d| {
            d.join(format!("{lang}.dic")).exists() && d.join(format!("{lang}.aff")).exists()
        })
        .unwrap_or(false)
}

impl MdApp {
    /// Record the pre-edit source for undo when the document changed since the
    /// previous frame. Called once per frame before any shortcut runs. Granularity
    /// is per-frame edit (precise); the stack is capped so it can't grow without
    /// bound.
    pub(crate) fn capture_undo_snapshot(&mut self) {
        if self.source == self.prev_source {
            return;
        }
        // Skip the initial load (empty baseline) so opening a file is not undoable.
        if !self.prev_source.is_empty() || !self.undo_stack.is_empty() {
            if self.undo_stack.last() != Some(&self.prev_source) {
                self.undo_stack.push(std::mem::take(&mut self.prev_source));
                if self.undo_stack.len() > 300 {
                    self.undo_stack.remove(0);
                }
            }
            self.redo_stack.clear();
        }
        self.prev_source = self.source.clone();
    }

    pub(crate) fn do_undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.source.clone());
            self.source = prev;
            self.prev_source = self.source.clone(); // do not re-capture this restore
            self.segments_dirty = true;
            self.modified = true;
            self.last_sel = None;
            self.status_msg = "Undo".into();
        } else {
            self.status_msg = "Nothing to undo".into();
        }
    }

    pub(crate) fn do_redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.source.clone());
            self.source = next;
            self.prev_source = self.source.clone();
            self.segments_dirty = true;
            self.modified = true;
            self.last_sel = None;
            self.status_msg = "Redo".into();
        } else {
            self.status_msg = "Nothing to redo".into();
        }
    }

    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut should_quit = false;
        let source_focused = ctx.memory(|m| m.has_focus(egui::Id::new("source_edit")));
        ctx.input_mut(|i| {
            // ── Pinch / two-finger touchpad zoom (no modifier required) ────
            let pinch = i.zoom_delta();
            if (pinch - 1.0).abs() > 0.001 {
                self.zoom_level = (self.zoom_level * pinch).clamp(0.5, 3.0);
            }

            // ── Ctrl + scroll wheel → zoom (consume scroll so panel doesn't also scroll)
            if i.modifiers.ctrl && i.smooth_scroll_delta.y != 0.0 {
                let delta = i.smooth_scroll_delta.y * 0.002;
                self.zoom_level = (self.zoom_level + delta).clamp(0.5, 3.0);
                i.smooth_scroll_delta = egui::Vec2::ZERO;
            }

            if i.modifiers.ctrl {
                // ── File ──────────────────────────────────────────────────
                if i.key_pressed(egui::Key::N) { self.do_new(); }
                if i.key_pressed(egui::Key::O) { self.do_open(); }
                if i.key_pressed(egui::Key::S) {
                    if i.modifiers.shift { self.do_save_as(); } else { self.do_save(); }
                }
                if i.key_pressed(egui::Key::P) {
                    // Ctrl+Shift+P → command palette; Ctrl+P → print.
                    if i.modifiers.shift {
                        self.command_palette_open = true;
                        self.palette_query.clear();
                    } else {
                        self.do_print();
                    }
                }
                if i.key_pressed(egui::Key::Q) { should_quit = true; }

                // ── App ───────────────────────────────────────────────────
                // Ctrl+Shift+D → toggle light / dark theme (light stays the default).
                if i.modifiers.shift && i.key_pressed(egui::Key::D) {
                    self.dark_mode = !self.dark_mode;
                }

                // ── Formatting ────────────────────────────────────────────
                if i.key_pressed(egui::Key::B) { self.toggle_inline_format(InlineFmt::Bold); }
                if i.key_pressed(egui::Key::I) { self.toggle_inline_format(InlineFmt::Italic); }
                if i.key_pressed(egui::Key::U) { self.toggle_inline_format(InlineFmt::Underline); }

                // ── Undo / Redo ───────────────────────────────────────────
                // consume_key so egui's per-field undo does not also fire.
                if i.consume_key(egui::Modifiers::CTRL, egui::Key::Z) { self.do_undo(); }
                if i.consume_key(egui::Modifiers::CTRL, egui::Key::Y) { self.do_redo(); }
                if i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z) { self.do_redo(); }

                // ── Insert ────────────────────────────────────────────────
                if i.key_pressed(egui::Key::K) {
                    self.open_link_dialog(false);
                }
                if i.key_pressed(egui::Key::E) {
                    self.insert_text("$$\n\\sum_{i=0}^{n} x_i\n$$\n");
                }

                // ── Search / Find ─────────────────────────────────────────
                if i.key_pressed(egui::Key::F) {
                    self.show_search = true;
                    self.search_show_replace = false;
                    self.compute_search_matches();
                }
                if i.key_pressed(egui::Key::H) {
                    self.show_search = true;
                    self.search_show_replace = true;
                    self.compute_search_matches();
                }

                // ── View modes ────────────────────────────────────────────
                if i.key_pressed(egui::Key::Num1) { self.view_mode = ViewMode::Source; }
                if i.key_pressed(egui::Key::Num2) { self.view_mode = ViewMode::Split; }
                if i.key_pressed(egui::Key::Num3) {
                    self.view_mode = ViewMode::Editor;
                    self.segments_dirty = true;
                }

                // ── Zoom keyboard ─────────────────────────────────────────
                if i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus) {
                    self.zoom_level = (self.zoom_level + 0.1).min(3.0);
                }
                if i.key_pressed(egui::Key::Minus) {
                    self.zoom_level = (self.zoom_level - 0.1).max(0.5);
                }
                if i.key_pressed(egui::Key::Num0) {
                    self.zoom_level = 1.0;
                }

                // ── Source code editor (only when the code editor is focused) ─
                if source_focused {
                    if i.key_pressed(egui::Key::D) && !i.modifiers.shift { self.duplicate_line(); }
                    if i.key_pressed(egui::Key::G) { self.goto_line_open = true; self.goto_line_input.clear(); }
                    if i.key_pressed(egui::Key::Slash) { self.toggle_source_comment(); }
                }
            }

            // ── Tab / Shift+Tab - indent / outdent the code-editor selection
            //    (consume so the TextEdit does not insert a tab or move focus) ─
            if source_focused {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) { self.indent_selection(); }
                if i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) { self.outdent_selection(); }
            }
            // ── F3 / Shift+F3 - find next / previous (no Ctrl required) ──
            if i.key_pressed(egui::Key::F3) {
                if i.modifiers.shift { self.do_find_prev(); } else { self.do_find_next(); }
            }
            // ── Escape - close search bar ─────────────────────────────────
            if i.key_pressed(egui::Key::Escape) && self.show_search {
                self.show_search = false;
            }
        });
        // Quit handled outside the closure (ctx is borrowed inside)
        if should_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Re-import a DOCX previously exported by MD -> ALL.
    /// Recovers the original markdown (+ LaTeX) from the embedded source entry.
    pub(crate) fn do_import_docx(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Word Document", &["docx"])
            .set_title("Import DOCX (MD -> ALL export)")
            .pick_file()
        {
            match source_embed::import_docx_source(&path) {
                Ok(markdown) => {
                    self.source = markdown;
                    self.current_file = None; // unsaved - prompt on next save
                    self.modified = true;
                    self.segments_dirty = true;
                    self.view_mode = ViewMode::Editor;
                    // Surface any reviewer feedback (tracked changes + comments) the
                    // supervisor left in Word, so it can be read in-app.
                    self.review_items =
                        mdall_core::docx_review::extract_review_items(&path).unwrap_or_default();
                    self.show_review_panel = !self.review_items.is_empty();
                    let review_note = if self.review_items.is_empty() {
                        String::new()
                    } else {
                        format!(" - {} review item(s)", self.review_items.len())
                    };
                    self.status_msg = format!(
                        "Imported from \u{201C}{}\u{201D} - {} chars recovered{}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        self.source.len(),
                        review_note,
                    );
                }
                Err(e) => {
                    self.status_msg = format!("Import failed: {}", e);
                }
            }
        }
    }

    pub(crate) fn do_new(&mut self) {
        self.source.clear();
        self.current_file = None;
        self.modified = false;
        self.segments_dirty = true;
        self.status_msg = "New file".into();
    }

    /// Open a blank document directly in the Split editor (source + rendered view
    /// side by side), the quickest path from the hub into editing.
    pub(crate) fn open_blank_split_editor(&mut self) {
        self.do_new();
        self.view_mode = ViewMode::Split;
        self.segments_dirty = true;
    }

    pub(crate) fn do_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("All Supported", &[
                "md","markdown","txt","docx","html","htm","epub","odt","rtf",
                "tex","latex","org","rst","wiki","mediawiki","adoc","asciidoc","asc","typ",
                "ipynb","bib","fb2","pptx","eml","csv","tsv","rmd","qmd",
                "py","js","ts","rs","c","cpp","java","go","rb","php","sh","r",
            ])
            .add_filter("Markdown",            &["md","markdown"])
            .add_filter("Word Document",       &["docx"])
            .add_filter("HTML",                &["html","htm"])
            .add_filter("EPUB eBook",          &["epub"])
            .add_filter("OpenDocument",        &["odt"])
            .add_filter("Rich Text (RTF)",     &["rtf"])
            .add_filter("LaTeX",               &["tex","latex"])
            .add_filter("Org-mode",            &["org"])
            .add_filter("reStructuredText",    &["rst"])
            .add_filter("AsciiDoc",            &["adoc","asciidoc","asc"])
            .add_filter("Typst",               &["typ"])
            .add_filter("Jupyter Notebook",    &["ipynb"])
            .add_filter("BibTeX",              &["bib"])
            .add_filter("FictionBook",         &["fb2"])
            .add_filter("PowerPoint",          &["pptx"])
            .add_filter("Email",               &["eml"])
            .add_filter("CSV / TSV",           &["csv","tsv"])
            .add_filter("R Markdown / Quarto", &["rmd","qmd"])
            .add_filter("Source Code",         &["py","js","ts","rs","c","cpp","java","go","rb","php","sh","r"])
            .add_filter("Plain Text",          &["txt"])
            .add_filter("All Files",           &["*"])
            .pick_file()
        {
            match Self::import_to_md(&path) {
                Ok(content) => {
                    self.source = content;
                    self.load_annotations(&path);
                    self.current_file = Some(path);
                    self.modified = false;
                    self.segments_dirty = true;
                    self.status_msg = "Opened".into();
                    // Switch to Editor so user sees the imported content
                    if self.view_mode == ViewMode::Converter {
                        self.view_mode = ViewMode::Editor;
                    }
                }
                Err(e) => self.status_msg = format!("Import error: {}", e),
            }
        }
    }

    pub(crate) fn do_save(&mut self) {
        if let Some(ref path) = self.current_file.clone() {
            match std::fs::write(path, &self.source) {
                Ok(()) => { self.modified = false; self.status_msg = "Saved".into(); self.save_annotations(path); }
                Err(e) => self.status_msg = format!("Save error: {}", e),
            }
        } else {
            self.do_save_as();
        }
    }

    pub(crate) fn do_save_as(&mut self) {
        // Install this document's custom LaTeX macros so the equation renderers
        // (KaTeX/Typst) can expand them on export.
        mdall_core::latex_macros::install_from_source(&self.source);
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .add_filter("PDF", &["pdf"])
            .add_filter("HTML", &["html", "htm"])
            .add_filter("All Files", &["*"]);
        if let Some(ref f) = self.current_file {
            if let Some(dir) = f.parent() { dlg = dlg.set_directory(dir); }
        }
        if let Some(path) = dlg.save_file()
        {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("md").to_lowercase();
            match ext.as_str() {
                "pdf" => {
                    let metadata = self.meta.clone();
                    let source_dir = self.current_file.as_ref().and_then(|f| f.parent());
                    match export::export_pdf_with_tier(&self.source, &path, &metadata, source_dir) {
                        Ok(tier) if path.exists() => {
                            self.status_msg = if tier.is_degraded() {
                                "PDF exported via basic fallback - equations are text approximations".into()
                            } else {
                                "PDF exported".into()
                            };
                            let _ = open::that(&path);
                        }
                        Ok(_) => self.status_msg = "PDF error: file not created".into(),
                        Err(e) => self.status_msg = format!("PDF error: {}", e),
                    }
                }
                "html" | "htm" => {
                    let metadata = self.meta.clone();
                    let source_dir = self.current_file.as_ref().and_then(|f| f.parent());
                    match export::export_html(&self.source, &path, &metadata, source_dir) {
                        Ok(()) if path.exists() => { self.status_msg = "HTML exported".into(); let _ = open::that(&path); }
                        Ok(()) => self.status_msg = "HTML error: file not created".into(),
                        Err(e) => self.status_msg = format!("HTML error: {}", e),
                    }
                }
                _ => {
                    match std::fs::write(&path, &self.source) {
                        Ok(()) => {
                            self.save_annotations(&path);
                            self.current_file = Some(path);
                            self.modified = false;
                            self.status_msg = "Saved".into();
                        }
                        Err(e) => self.status_msg = format!("Save error: {}", e),
                    }
                }
            }
        }
    }

    pub(crate) fn do_export_pdf(&mut self) {
        mdall_core::latex_macros::install_from_source(&self.source);
        let mut dlg = rfd::FileDialog::new().add_filter("PDF", &["pdf"]);
        if let Some(ref f) = self.current_file {
            if let Some(dir) = f.parent() { dlg = dlg.set_directory(dir); }
            if let Some(stem) = f.file_stem() {
                dlg = dlg.set_file_name(&format!("{}.pdf", stem.to_string_lossy()));
            }
        }
        if let Some(path) = dlg.save_file() {
            let metadata = self.meta.clone();
            let source_dir = self.current_file.as_ref().and_then(|f| f.parent());
            match export::export_pdf_with_tier(&self.source, &path, &metadata, source_dir) {
                Ok(tier) if path.exists() => {
                    self.status_msg = if tier.is_degraded() {
                        "PDF exported via basic fallback - equations are text approximations".into()
                    } else {
                        "PDF exported".into()
                    };
                    let _ = open::that(&path);
                }
                Ok(_) => self.status_msg = "PDF error: file not created".into(),
                Err(e) => self.status_msg = format!("PDF error: {}", e),
            }
        }
    }

    pub(crate) fn do_export_html(&mut self) {
        mdall_core::latex_macros::install_from_source(&self.source);
        let mut dlg = rfd::FileDialog::new().add_filter("HTML", &["html"]);
        if let Some(ref f) = self.current_file {
            if let Some(dir) = f.parent() { dlg = dlg.set_directory(dir); }
            if let Some(stem) = f.file_stem() {
                dlg = dlg.set_file_name(&format!("{}.html", stem.to_string_lossy()));
            }
        }
        if let Some(path) = dlg.save_file() {
            let metadata = self.meta.clone();
            let source_dir = self.current_file.as_ref().and_then(|f| f.parent());
            match export::export_html(&self.source, &path, &metadata, source_dir) {
                Ok(()) if path.exists() => { self.status_msg = "HTML exported".into(); let _ = open::that(&path); }
                Ok(()) => self.status_msg = "HTML error: file not created".into(),
                Err(e) => self.status_msg = format!("HTML error: {}", e),
            }
        }
    }

    pub(crate) fn do_insert_image_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "svg", "webp"])
            .pick_file()
        {
            let rel = self.import_image_to_assets(&path);
            let alt = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
            self.insert_text(&format!("![{}]({})", alt, rel));
        }
    }

    /// Copy an image into an `assets/` folder beside the current document and
    /// return the relative `assets/<name>` path, so exports resolve it. Falls
    /// back to the absolute path when there is no document folder yet, or the
    /// copy fails.
    pub(crate) fn import_image_to_assets(&self, src: &std::path::Path) -> String {
        if let Some(dir) = self.current_file.as_ref().and_then(|f| f.parent()) {
            let assets = dir.join("assets");
            if std::fs::create_dir_all(&assets).is_ok() {
                let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "image.png".into());
                let dest = Self::unique_dest(&assets, &name);
                if std::fs::copy(src, &dest).is_ok() {
                    if let Some(fname) = dest.file_name() {
                        return format!("assets/{}", fname.to_string_lossy());
                    }
                }
            }
        }
        src.display().to_string()
    }

    /// A non-clobbering destination: `name`, else `name_1`, `name_2`, ...
    fn unique_dest(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
        let np = std::path::Path::new(name);
        let stem = np.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = np.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
        for i in 1..1000 {
            let cand = dir.join(format!("{stem}_{i}{ext}"));
            if !cand.exists() {
                return cand;
            }
        }
        p
    }

    /// Insert image files dropped onto the editor: copied under `assets/` with a
    /// Markdown reference written at the cursor. The converter home handles its
    /// own drops; this runs only in the editor views.
    pub(crate) fn handle_editor_file_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for path in dropped {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg") {
                let rel = self.import_image_to_assets(&path);
                let alt = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
                self.insert_text(&format!("![{}]({})\n", alt, rel));
            }
        }
    }

    /// Open the visual table editor. If the cursor sits inside an existing GFM
    /// table, load it for editing (Apply overwrites it); otherwise start a fresh
    /// 3x3 grid to insert at the cursor.
    pub(crate) fn open_table_dialog(&mut self) {
        if let Some((range, mut td)) = self.table_at_cursor() {
            td.replace = Some(range);
            td.visible = true;
            self.table_dialog = td;
        } else {
            self.table_dialog.reset(3, 3);
            self.table_dialog.visible = true;
        }
    }

    /// If the cursor sits within a contiguous block of `|`-prefixed lines that
    /// parses as a GFM table, return its source byte range and parsed grid.
    fn table_at_cursor(&self) -> Option<(std::ops::Range<usize>, crate::ui::state::TableDialog)> {
        let src = &self.source;
        let cur = self.cursor_pos.min(src.len());
        let mut lines: Vec<(usize, &str)> = Vec::new();
        let mut idx = 0usize;
        for line in src.split_inclusive('\n') {
            let trimmed = line.strip_suffix('\n').unwrap_or(line);
            lines.push((idx, trimmed));
            idx += line.len();
        }
        if lines.is_empty() {
            return None;
        }
        let mut li = lines.len() - 1;
        for (i, (start, text)) in lines.iter().enumerate() {
            if cur >= *start && cur <= start + text.len() {
                li = i;
                break;
            }
        }
        let is_tbl = |t: &str| t.trim_start().starts_with('|');
        if !is_tbl(lines[li].1) {
            return None;
        }
        let mut top = li;
        while top > 0 && is_tbl(lines[top - 1].1) {
            top -= 1;
        }
        let mut bot = li;
        while bot + 1 < lines.len() && is_tbl(lines[bot + 1].1) {
            bot += 1;
        }
        let start_byte = lines[top].0;
        let end_byte = lines[bot].0 + lines[bot].1.len();
        let block = &src[start_byte..end_byte];
        crate::ui::state::TableDialog::from_markdown(block).map(|td| (start_byte..end_byte, td))
    }

    /// Open the link/image dialog, pre-filling the text field with the current
    /// selection so wrapping selected text as a hyperlink keeps that text (the
    /// dialog's Insert replaces the selection with `[text](url)`; an empty text
    /// field would otherwise erase the selected words).
    pub(crate) fn open_link_dialog(&mut self, is_image: bool) {
        let s = self.cursor_pos.min(self.selection_anchor);
        let e = self.cursor_pos.max(self.selection_anchor);
        let text = if s != e {
            let bs = char_to_byte_index(&self.source, s).min(self.source.len());
            let be = char_to_byte_index(&self.source, e).min(self.source.len());
            self.source[bs..be].to_string()
        } else {
            String::new()
        };
        self.link_dialog = LinkDialog { visible: true, text, url: String::new(), is_image };
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let sel_start  = self.cursor_pos.min(self.selection_anchor);
        let sel_end    = self.cursor_pos.max(self.selection_anchor);
        let byte_start = char_to_byte_index(&self.source, sel_start).min(self.source.len());
        let byte_end   = char_to_byte_index(&self.source, sel_end).min(self.source.len());

        self.source.replace_range(byte_start..byte_end, text);
        let new_pos = sel_start + text.chars().count();
        self.cursor_pos    = new_pos;
        self.selection_anchor = new_pos;

        self.apply_cursor_to_editor_state();
        self.modified = true;
        self.segments_dirty = true;
        self.status_msg = "Modified".into();
    }


    /// Schedule a cursor move for the next frame (applied inside show_source_editor).
    pub(crate) fn apply_cursor_to_editor_state(&mut self) {
        self.pending_cursor = Some((self.cursor_pos, self.selection_anchor));
    }

    /// Wraps the current cursor position (or selected text) in an HTML alignment div.
    /// `align` is one of: left | center | right | justify.
    pub(crate) fn wrap_block_align(&mut self, align: &str) {
        // Alignment is a BLOCK operation: align the whole block the cursor sits in,
        // never wrap an inline selection in a nested <div> (which lands a block
        // element mid-line and breaks the rendering of everything after it).
        if self.segments_dirty {
            self.blocks = mdall_core::editor::parse_document(&self.source);
        }
        let nchars = self.source.chars().count();
        let cur_b = char_to_byte_index(&self.source, self.cursor_pos.min(nchars)).min(self.source.len());
        let block = self
            .blocks
            .iter()
            .find(|b| cur_b >= b.source_range.start && cur_b < b.source_range.end)
            .or_else(|| self.blocks.iter().rev().find(|b| cur_b >= b.source_range.start))
            .or_else(|| self.blocks.first())
            .cloned();
        let Some(block) = block else { return };
        if matches!(block.kind, mdall_core::editor::BlockKind::FencedCode { .. }) {
            self.status_msg = "Alignment does not apply inside a code block".into();
            return;
        }
        let bs = block.source_range.start.min(self.source.len());
        let be = block.source_range.end.min(self.source.len());
        let body_len = self.source[bs..be].trim_end().len();
        if body_len == 0 {
            return;
        }
        let body = self.source[bs..bs + body_len].to_string();

        // Already a <div> container (e.g. a styled box): set text-align in its own
        // style attribute, so it just gains / changes its alignment without nesting.
        if body.trim_start().to_ascii_lowercase().starts_with("<div") {
            if let Some(new_div) = set_div_text_align(&body, align) {
                self.source.replace_range(bs..bs + body_len, &new_div);
                self.after_format_edit();
                return;
            }
        }
        if align == "left" {
            self.status_msg = "Left is the default alignment".into();
            return;
        }
        // Plain block: wrap the whole block in an aligned div at clean boundaries.
        let wrapped = format!("<div style=\"text-align:{}\">\n\n{}\n\n</div>", align, body);
        self.source.replace_range(bs..bs + body_len, &wrapped);
        self.after_format_edit();
    }

    /// Wrap selected text with `before`/`after` markers.
    /// If text is selected, the selection is wrapped: "hello" → "**hello**".
    /// If nothing is selected, inserts "before·text·after" and selects the placeholder.
    /// Toggle a composable inline format (bold / italic / strike / code /
    /// underline) on the selection by NORMALIZING: read the selection's real
    /// format set, flip the requested one, and re-emit one canonical clean span.
    /// This never accumulates markers and makes bold + italic compose to `***`.
    pub(crate) fn toggle_inline_format(&mut self, fmt: InlineFmt) {
        let nchars = self.source.chars().count();
        let (mut sel_start, mut sel_end) = (
            self.cursor_pos.min(self.selection_anchor),
            self.cursor_pos.max(self.selection_anchor),
        );
        if sel_start == sel_end {
            if let Some((s, e)) = self.last_sel {
                if s < e && e <= nchars {
                    sel_start = s;
                    sel_end = e;
                }
            }
        }
        // Nothing selected: insert a placeholder already wrapped in the format.
        if sel_start == sel_end {
            let (op, cl) = inline_markers(fmt);
            let bs = char_to_byte_index(&self.source, sel_start).min(self.source.len());
            self.source.insert_str(bs, &format!("{}text{}", op, cl));
            let opc = op.chars().count();
            self.select_range(sel_start + opc, sel_start + opc + 4);
            self.after_format_edit();
            return;
        }

        let bs = char_to_byte_index(&self.source, sel_start).min(self.source.len());
        let be = char_to_byte_index(&self.source, sel_end).min(self.source.len());
        let (new_source, content) = normalize_inline_toggle(&self.source, bs, be, fmt);
        let cs = new_source[..content.start].chars().count();
        let ce = new_source[..content.end].chars().count();
        self.source = new_source;
        self.select_range(cs, ce);
        self.after_format_edit();
    }

    pub(crate) fn wrap_text(&mut self, before: &str, after: &str) {
        let nchars = self.source.chars().count();
        let (mut sel_start, mut sel_end) = (
            self.cursor_pos.min(self.selection_anchor),
            self.cursor_pos.max(self.selection_anchor),
        );
        // The live selection collapses when the click moves focus to the toolbar.
        // Fall back to the last real editor selection (as the comment tool does) so
        // the format wraps the intended text instead of inserting a placeholder.
        if sel_start == sel_end {
            if let Some((s, e)) = self.last_sel {
                if s < e && e <= nchars {
                    sel_start = s;
                    sel_end = e;
                }
            }
        }
        let has_sel = sel_start != sel_end;

        let byte_start = char_to_byte_index(&self.source, sel_start).min(self.source.len());
        let byte_end   = char_to_byte_index(&self.source, sel_end).min(self.source.len());

        // Toggle OFF: a second click on an already-formatted selection removes the
        // markers instead of nesting another pair (the accumulation bug).
        if has_sel {
            let inner = self.source[byte_start..byte_end].to_string();
            // (a) the markers are inside the selection: "**word**" is selected.
            //     Guard: a lone "*" (italic) must not strip the "**" of bold.
            let a_ok = !(before == "*" && (inner.starts_with("**") || inner.ends_with("**")));
            if a_ok
                && inner.len() >= before.len() + after.len()
                && inner.starts_with(before)
                && inner.ends_with(after)
            {
                let stripped = inner[before.len()..inner.len() - after.len()].to_string();
                let n = stripped.chars().count();
                self.source.replace_range(byte_start..byte_end, &stripped);
                self.select_range(sel_start, sel_start + n);
                self.after_format_edit();
                return;
            }
            // (b) the markers sit just outside the selection: the rendered content
            //     "word" of "**word**" is selected (markup hidden in WYSIWYG).
            //     Guard: a lone "*" must not match the inner "*" of a "**" (bold),
            //     which is what breaks combining bold + italic.
            let b_ok = before != "*"
                || (self.source.as_bytes().get(byte_start.wrapping_sub(2)) != Some(&b'*')
                    && self.source.as_bytes().get(byte_end + after.len()) != Some(&b'*'));
            if b_ok
                && byte_start >= before.len()
                && byte_end + after.len() <= self.source.len()
                && self.source.is_char_boundary(byte_start - before.len())
                && self.source.is_char_boundary(byte_end + after.len())
                && &self.source[byte_start - before.len()..byte_start] == before
                && &self.source[byte_end..byte_end + after.len()] == after
            {
                self.source.replace_range(byte_end..byte_end + after.len(), "");
                self.source.replace_range(byte_start - before.len()..byte_start, "");
                let bl = before.chars().count();
                self.select_range(sel_start - bl, sel_end - bl);
                self.after_format_edit();
                return;
            }
        }

        // ADD a "text" placeholder when nothing is selected.
        if !has_sel {
            let replacement = format!("{}text{}", before, after);
            self.source.replace_range(byte_start..byte_end, &replacement);
            let bl = before.chars().count();
            self.select_range(sel_start + bl, sel_start + bl + 4);
            self.after_format_edit();
            return;
        }
        // ADD on a selection. MERGE: absorb the same-format markers immediately
        // around the selection and strip any inside it, so re-formatting a partly
        // formatted phrase yields ONE clean span ("**A** B" -> bold "A B" ->
        // "**A B**") instead of nesting broken markers. Only for distinct multi-char
        // tokens, where stripping can't damage a different format (a lone "*" / "$"
        // is left alone so it never touches nested bold).
        let (mut ms, mut me) = (byte_start, byte_end);
        let mut inner = self.source[ms..me].to_string();
        if before.len() >= 2 || before == "`" {
            while ms >= before.len()
                && self.source.is_char_boundary(ms - before.len())
                && &self.source[ms - before.len()..ms] == before
            {
                ms -= before.len();
            }
            while me + after.len() <= self.source.len()
                && self.source.is_char_boundary(me + after.len())
                && &self.source[me..me + after.len()] == after
            {
                me += after.len();
            }
            let raw = self.source[ms..me].to_string();
            inner = if before == after {
                raw.replace(before, "")
            } else {
                raw.replace(before, "").replace(after, "")
            };
        }
        let start_char = self.source[..ms].chars().count();
        let replacement = format!("{}{}{}", before, inner, after);
        self.source.replace_range(ms..me, &replacement);
        let bl = before.chars().count();
        let il = inner.chars().count();
        // Select the formatted content so the user sees it and can chain another
        // format; this also refreshes last_sel for the next toolbar click.
        self.select_range(start_char + bl, start_char + bl + il);
        self.after_format_edit();
    }

    /// Apply a VALUE-carrying inline style span (text colour, highlight, font size)
    /// to the selection, REPLACING any same-class span already wrapping it instead
    /// of nesting a new one. This kills the corruption where re-colouring stacked
    /// `<span><mark><span>...` (the source rotted while the render showed only the
    /// last colour). Re-applying the exact same value toggles it off.
    ///
    /// `tag` = "span" | "mark"; `style_key` = "color" | "background" | "font-size";
    /// `value` = the full value, e.g. "#e74c3c" or "14pt".
    pub(crate) fn wrap_value_span(&mut self, tag: &str, style_key: &str, value: &str) {
        // Byte index where a `<tag ...style_key:...>` open tag begins, if `s[..end]`
        // ends exactly with such a tag (the span wrapping the selection).
        fn open_tag_ending_at(s: &str, end: usize, tag: &str, key: &str) -> Option<usize> {
            if end == 0 || !s.is_char_boundary(end) || s.as_bytes().get(end - 1) != Some(&b'>') {
                return None;
            }
            let open = s[..end].rfind('<')?;
            let t = s[open..end].to_ascii_lowercase();
            (t.starts_with(&format!("<{tag}")) && t.contains(&format!("{key}:"))).then_some(open)
        }
        // `start + len("</tag>")` if `s[start..]` begins with that closing tag.
        fn close_tag_at(s: &str, start: usize, tag: &str) -> Option<usize> {
            let close = format!("</{tag}>");
            (start <= s.len() && s[start..].to_ascii_lowercase().starts_with(&close))
                .then_some(start + close.len())
        }
        // Read `key:` value from a style tag (up to `;`, quote or `>`).
        fn style_value(tag: &str, key: &str) -> Option<String> {
            let needle = format!("{key}:");
            let kpos = tag.to_ascii_lowercase().find(&needle)?;
            let after = &tag[kpos + needle.len()..];
            let end = after.find(|c: char| matches!(c, ';' | '"' | '\'' | '>')).unwrap_or(after.len());
            Some(after[..end].trim().to_string())
        }

        let nchars = self.source.chars().count();
        let (mut sel_start, mut sel_end) = (
            self.cursor_pos.min(self.selection_anchor),
            self.cursor_pos.max(self.selection_anchor),
        );
        if sel_start == sel_end {
            if let Some((s, e)) = self.last_sel {
                if s < e && e <= nchars { sel_start = s; sel_end = e; }
            }
        }
        if sel_start == sel_end { return; } // value spans need a real selection

        let mut bs = char_to_byte_index(&self.source, sel_start).min(self.source.len());
        let mut be = char_to_byte_index(&self.source, sel_end).min(self.source.len());

        // (a) The whole `<tag ...key...>inner</tag>` sits inside the selection
        //     (Source view): shrink to the inner text so we re-wrap, not nest.
        {
            let close = format!("</{tag}>");
            let seg = self.source[bs..be].to_ascii_lowercase();
            if seg.starts_with(&format!("<{tag}")) && seg.ends_with(&close) {
                if let Some(gt) = self.source[bs..be].find('>') {
                    if self.source[bs..bs + gt + 1].to_ascii_lowercase().contains(&format!("{style_key}:")) {
                        bs += gt + 1;
                        be -= close.len();
                    }
                }
            }
        }

        // (b) A same-class span wraps the selection just OUTSIDE it (WYSIWYG: the
        //     rendered inner text is selected, the tags are hidden). Swallow it so
        //     we replace its value instead of nesting.
        let (mut region_start, mut region_end, mut existing) = (bs, be, None);
        if let Some(os) = open_tag_ending_at(&self.source, bs, tag, style_key) {
            if let Some(ce) = close_tag_at(&self.source, be, tag) {
                existing = style_value(&self.source[os..bs], style_key);
                region_start = os;
                region_end = ce;
            }
        }

        let inner = self.source[bs..be].to_string();
        let toggle_off = existing.as_deref() == Some(value);
        let open = format!("<{tag} style=\"{style_key}:{value}\">");
        let close = format!("</{tag}>");
        let replacement = if toggle_off { inner.clone() } else { format!("{open}{inner}{close}") };
        self.source.replace_range(region_start..region_end, &replacement);

        let rs_char = self.source[..region_start].chars().count();
        let inner_chars = inner.chars().count();
        if toggle_off {
            self.select_range(rs_char, rs_char + inner_chars);
        } else {
            let lead = open.chars().count();
            self.select_range(rs_char + lead, rs_char + lead + inner_chars);
        }
        self.after_format_edit();
    }

    /// Set the editor selection and remember it as the last real selection, so a
    /// following toolbar format wraps the same text after focus moves.
    fn select_range(&mut self, start: usize, end: usize) {
        self.selection_anchor = start;
        self.cursor_pos = end;
        self.last_sel = if start != end { Some((start, end)) } else { None };
    }

    /// Shared post-edit bookkeeping for a toolbar format command.
    fn after_format_edit(&mut self) {
        self.apply_cursor_to_editor_state();
        self.modified = true;
        self.segments_dirty = true;
        self.status_msg = "Modified".into();
    }

    /// Apply `f` to every line the selection touches (whole-line transform),
    /// keeping the transformed block selected. Used by indent/outdent.
    fn transform_selected_lines<F: Fn(&str) -> String>(&mut self, f: F) {
        let sel_start = self.cursor_pos.min(self.selection_anchor);
        let sel_end = self.cursor_pos.max(self.selection_anchor);
        let bstart = char_to_byte_index(&self.source, sel_start).min(self.source.len());
        let bend = char_to_byte_index(&self.source, sel_end).min(self.source.len());
        let line_start = self.source[..bstart].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = self.source[bend..].find('\n').map(|i| bend + i).unwrap_or(self.source.len());

        let block = self.source[line_start..line_end].to_string();
        let new_block = block.split('\n').map(|l| f(l)).collect::<Vec<_>>().join("\n");
        self.source.replace_range(line_start..line_end, &new_block);

        let start_char = self.source[..line_start].chars().count();
        self.selection_anchor = start_char;
        self.cursor_pos = start_char + new_block.chars().count();
        self.apply_cursor_to_editor_state();
        self.mark_source_modified();
    }

    /// Indent the selected lines by two spaces (Source code editor).
    pub(crate) fn indent_selection(&mut self) {
        self.transform_selected_lines(|l| format!("  {}", l));
    }

    /// Remove up to two leading spaces (or a tab) from the selected lines.
    pub(crate) fn outdent_selection(&mut self) {
        self.transform_selected_lines(|l| {
            if let Some(r) = l.strip_prefix("  ") { r.to_string() }
            else if let Some(r) = l.strip_prefix(' ') { r.to_string() }
            else if let Some(r) = l.strip_prefix('\t') { r.to_string() }
            else { l.to_string() }
        });
    }

    /// Toggle an HTML comment (`<!-- ... -->`) around the selection.
    pub(crate) fn toggle_source_comment(&mut self) {
        let s = self.cursor_pos.min(self.selection_anchor);
        let e = self.cursor_pos.max(self.selection_anchor);
        let bs = char_to_byte_index(&self.source, s).min(self.source.len());
        let be = char_to_byte_index(&self.source, e).min(self.source.len());
        let sel = self.source[bs..be].to_string();
        let t = sel.trim();
        if t.len() >= 7 && t.starts_with("<!--") && t.ends_with("-->") {
            let inner = t[4..t.len() - 3].trim().to_string();
            self.source.replace_range(bs..be, &inner);
            self.selection_anchor = s;
            self.cursor_pos = s + inner.chars().count();
            self.apply_cursor_to_editor_state();
            self.mark_source_modified();
        } else {
            self.wrap_text("<!-- ", " -->");
        }
    }

    /// Duplicate the line the cursor is on, placing the copy just below it.
    pub(crate) fn duplicate_line(&mut self) {
        let pos = self.cursor_pos;
        let bpos = char_to_byte_index(&self.source, pos).min(self.source.len());
        let line_start = self.source[..bpos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = self.source[bpos..].find('\n').map(|i| bpos + i).unwrap_or(self.source.len());
        let line = self.source[line_start..line_end].to_string();
        self.source.insert_str(line_end, &format!("\n{}", line));
        let added = line.chars().count() + 1;
        self.cursor_pos = pos + added;
        self.selection_anchor = self.cursor_pos;
        self.apply_cursor_to_editor_state();
        self.mark_source_modified();
    }

    /// Move the cursor to the start of line `line` (1-based) and focus the editor.
    pub(crate) fn go_to_line(&mut self, line: usize) {
        let target = line.max(1);
        let mut idx = 0usize;
        let mut ln = 1usize;
        for ch in self.source.chars() {
            if ln == target { break; }
            idx += 1;
            if ch == '\n' { ln += 1; }
        }
        self.cursor_pos = idx;
        self.selection_anchor = idx;
        self.apply_cursor_to_editor_state();
        self.request_source_focus = true;
    }

    /// Shared bookkeeping after a programmatic source edit.
    fn mark_source_modified(&mut self) {
        self.modified = true;
        self.segments_dirty = true;
        self.status_msg = "Modified".into();
    }

    /// Recompute all match positions in `source` for the current query + case setting.
    pub(crate) fn compute_search_matches(&mut self) {
        self.search_matches.clear();
        if self.search_query.is_empty() { return; }

        let (haystack, needle) = if self.search_case_sensitive {
            (self.source.clone(), self.search_query.clone())
        } else {
            (self.source.to_lowercase(), self.search_query.to_lowercase())
        };

        let needle_len = needle.len();
        if needle_len == 0 { return; }
        let mut start = 0usize;
        while start < haystack.len() {
            match haystack[start..].find(&needle) {
                Some(rel) => {
                    self.search_matches.push(start + rel);
                    start += rel + needle_len;
                }
                None => break,
            }
        }
    }

    /// Jump source cursor to the current match and select it.
    /// In Preview mode → switch to Split so the user can see the match in context.
    pub(crate) fn jump_to_match(&mut self) {
        let Some(&byte_start) = self.search_matches.get(self.search_match_idx) else {
            self.status_msg = "No match".into();
            return;
        };
        let byte_end = (byte_start + self.search_query.len()).min(self.source.len());
        let char_start = byte_to_char_index(&self.source, byte_start);
        let char_end   = byte_to_char_index(&self.source, byte_end);

        self.cursor_pos       = char_end;
        self.selection_anchor = char_start;
        self.apply_cursor_to_editor_state();

        // In Editor-only mode, switch to Split so the source match is visible
        if self.view_mode == ViewMode::Editor {
            self.view_mode = ViewMode::Split;
        }
        self.request_source_focus = true;

        let total = self.search_matches.len();
        self.status_msg = format!(
            "Match {} of {}",
            self.search_match_idx + 1,
            total,
        );
    }

    pub(crate) fn do_find_next(&mut self) {
        if self.search_query.is_empty() { return; }
        self.compute_search_matches();
        if self.search_matches.is_empty() {
            self.status_msg = "Not found".into();
            return;
        }
        // Advance past current match (wrap around)
        self.search_match_idx = (self.search_match_idx + 1) % self.search_matches.len();
        self.jump_to_match();
    }

    pub(crate) fn do_find_prev(&mut self) {
        if self.search_query.is_empty() { return; }
        self.compute_search_matches();
        if self.search_matches.is_empty() {
            self.status_msg = "Not found".into();
            return;
        }
        let len = self.search_matches.len();
        self.search_match_idx = if self.search_match_idx == 0 { len - 1 } else { self.search_match_idx - 1 };
        self.jump_to_match();
    }

    /// Replace the currently highlighted occurrence and jump to the next match.
    pub(crate) fn do_replace_current(&mut self) {
        if self.search_matches.is_empty() { return; }
        let byte_start = self.search_matches[self.search_match_idx];
        let byte_end   = (byte_start + self.search_query.len()).min(self.source.len());
        self.source.replace_range(byte_start..byte_end, &self.replace_query);
        self.modified       = true;
        self.segments_dirty = true;
        self.compute_search_matches();
        // Keep index in bounds after replacement
        if !self.search_matches.is_empty() {
            self.search_match_idx = self.search_match_idx.min(self.search_matches.len() - 1);
            self.jump_to_match();
        } else {
            self.status_msg = "All occurrences replaced".into();
        }
    }

    pub(crate) fn do_replace_all(&mut self) {
        if self.search_query.is_empty() { return; }
        // Compute fresh matches with current case setting
        self.compute_search_matches();
        let count = self.search_matches.len();
        if count == 0 {
            self.status_msg = "Not found".into();
            return;
        }
        // Replace from end to start to preserve byte offsets
        for &byte_start in self.search_matches.iter().rev() {
            let byte_end = (byte_start + self.search_query.len()).min(self.source.len());
            self.source.replace_range(byte_start..byte_end, &self.replace_query);
        }
        self.modified       = true;
        self.segments_dirty = true;
        self.search_matches.clear();
        self.search_match_idx = 0;
        self.status_msg = format!("Replaced {} occurrence{}", count, if count == 1 { "" } else { "s" });
    }
}

impl MdApp {
    /// Export document to a temp PDF then send it to the system default printer.
    pub(crate) fn do_print(&mut self) {
        mdall_core::latex_macros::install_from_source(&self.source);
        let tmp_path = std::env::temp_dir().join("mdall-print.pdf");
        let metadata  = self.meta.clone();
        let source_dir = self.current_file.as_ref().and_then(|f| f.parent());

        match export::export_pdf(&self.source, &tmp_path, &metadata, source_dir) {
            Ok(()) if tmp_path.exists() => {
                // Use the PDF verb so Windows opens the default PDF printer dialog.
                let path_str = tmp_path.to_string_lossy().replace('\'', "''"); // escape for PS string
                let cmd = format!("Start-Process -FilePath '{}' -Verb Print", path_str);
                match std::process::Command::new("powershell")
                    .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd])
                    .spawn()
                {
                    Ok(_) => self.status_msg = "Sent to printer".into(),
                    Err(e) => self.status_msg = format!("Print error: {}", e),
                }
            }
            Ok(()) => self.status_msg = "Print error: PDF not created".into(),
            Err(e) => self.status_msg = format!("Print error: {}", e),
        }
    }
}

impl MdApp {
    /// Right-hand panel listing reviewer feedback (tracked changes + comments)
    /// recovered from an imported DOCX. Read-only display + jump-to-source; it
    /// never mutates the document source.
    pub(crate) fn render_review_panel(&mut self, ctx: &egui::Context) {
        if !self.show_review_panel || self.review_items.is_empty() {
            return;
        }
        use mdall_core::docx_review::ReviewKind;
        let mut jump: Option<String> = None;
        let mut dismiss: Option<usize> = None;
        let mut close = false;
        let mut hover_anchor: Option<String> = None;
        let mut click_anchor: Option<String> = None;
        egui::SidePanel::right("review_panel")
            .resizable(true)
            .default_width(300.0)
            .min_width(220.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Review").heading());
                    ui.label(egui::RichText::new(format!("({})", self.review_items.len())).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("\u{2715}").on_hover_text("Close panel").clicked() {
                            close = true;
                        }
                    });
                });
                ui.label(egui::RichText::new("Tracked changes & comments from the imported DOCX")
                    .small().weak());
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, it) in self.review_items.iter().enumerate() {
                        let (tag, col) = match it.kind {
                            ReviewKind::Insertion => ("INSERT", egui::Color32::from_rgb(39, 174, 96)),
                            ReviewKind::Deletion  => ("DELETE", egui::Color32::from_rgb(192, 57, 43)),
                            ReviewKind::Comment   => ("COMMENT", egui::Color32::from_rgb(41, 128, 185)),
                        };
                        // The passage this item is anchored to (for the editor frame).
                        let anchor = if it.context.is_empty() { it.text.clone() } else { it.context.clone() };
                        let marked = self.review_mark.as_deref() == Some(anchor.as_str());
                        let mut frame = egui::Frame::group(ui.style());
                        if marked {
                            // Persistent "marked" item: amber tint + border.
                            frame = frame
                                .fill(egui::Color32::from_rgba_unmultiplied(201, 146, 10, 28))
                                .stroke(egui::Stroke::new(1.0, crate::theme::ACCENT));
                        }
                        let ir = frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(tag).small().strong().color(col));
                                if !it.author.is_empty() {
                                    ui.label(egui::RichText::new(&it.author).small().weak());
                                }
                            });
                            if it.kind == ReviewKind::Comment && !it.context.is_empty() {
                                ui.label(egui::RichText::new(format!("\u{201C}{}\u{201D}", it.context))
                                    .small().italics().weak());
                            }
                            let body = if it.kind == ReviewKind::Deletion {
                                egui::RichText::new(&it.text).strikethrough()
                            } else {
                                egui::RichText::new(&it.text)
                            };
                            ui.label(body);
                            ui.horizontal(|ui| {
                                if ui.small_button("Jump to text").clicked() {
                                    jump = Some(anchor.clone());
                                }
                                if ui.small_button("Dismiss").clicked() {
                                    dismiss = Some(i);
                                }
                            });
                        });
                        // Hover frames the passage; clicking the card marks it.
                        let resp = ir.response.interact(egui::Sense::click());
                        if resp.hovered() {
                            hover_anchor = Some(anchor.clone());
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            click_anchor = Some(anchor.clone());
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        if close {
            self.show_review_panel = false;
        }
        // Hover frames the anchored passage in the editor this frame; a click
        // toggles a persistent mark on it.
        self.review_hl = hover_anchor;
        if let Some(a) = click_anchor {
            self.review_mark = if self.review_mark.as_deref() == Some(a.as_str()) {
                None
            } else {
                Some(a)
            };
        }
        if let Some(t) = jump {
            self.jump_to_text(&t);
        }
        if let Some(i) = dismiss {
            if i < self.review_items.len() {
                self.review_items.remove(i);
            }
            if self.review_items.is_empty() {
                self.show_review_panel = false;
            }
        }
    }

    /// Open the comment-authoring dialog anchored to the given selected passage.
    pub(crate) fn open_comment_dialog(&mut self, anchor: String) {
        self.comment_dialog = crate::ui::state::CommentDialog {
            visible: true,
            anchor,
            body: String::new(),
        };
    }

    /// Modal to write a comment anchored to a selected passage. On Add it appends
    /// a `Comment` review item (author "You") and opens the Review panel, so the
    /// new comment behaves exactly like an imported one (hover = frame, etc.).
    pub(crate) fn show_comment_dialog(&mut self, ctx: &egui::Context) {
        if !self.comment_dialog.visible {
            return;
        }
        let mut add = false;
        let mut cancel = false;
        egui::Window::new("Add comment")
            .id(egui::Id::new("comment_dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label(egui::RichText::new("Comment on this passage:").small().weak());
                let preview: String = self.comment_dialog.anchor.chars().take(140).collect();
                let ellipsis = if self.comment_dialog.anchor.chars().count() > 140 { "..." } else { "" };
                ui.label(egui::RichText::new(format!("\u{201C}{preview}{ellipsis}\u{201D}")).italics());
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.comment_dialog.body)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Your comment..."),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Add").clicked() {
                        add = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if add && !self.comment_dialog.body.trim().is_empty() {
            self.review_items.push(mdall_core::docx_review::ReviewItem {
                kind: mdall_core::docx_review::ReviewKind::Comment,
                author: "You".into(),
                date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                text: self.comment_dialog.body.trim().to_string(),
                context: self.comment_dialog.anchor.clone(),
            });
            self.show_review_panel = true;
            self.comment_dialog.visible = false;
            self.status_msg = "Comment added".into();
        } else if cancel || (add && self.comment_dialog.body.trim().is_empty()) {
            self.comment_dialog.visible = false;
        }
    }

    /// Select the first occurrence of `needle` in the source and reveal it
    /// (switches Editor-only mode to Split so the source selection is visible).
    fn jump_to_text(&mut self, needle: &str) {
        if needle.is_empty() {
            return;
        }
        if let Some(b) = self.source.find(needle) {
            self.selection_anchor = byte_to_char_index(&self.source, b);
            self.cursor_pos = byte_to_char_index(&self.source, b + needle.len());
            if self.view_mode == ViewMode::Editor {
                self.view_mode = ViewMode::Split;
            }
            self.request_source_focus = true;
            self.apply_cursor_to_editor_state();
            self.status_msg = "Jumped to reviewed text".into();
        } else {
            self.status_msg = "Reviewed text not found in the recovered document".into();
        }
    }
}

impl MdApp {
    /// The Module system window: tabbed manager for the spell engine's
    /// dictionaries, the application language (i18n), and reserved slots for
    /// citation styles, themes and export templates (downloadable packs).
    pub(crate) fn show_module_window(&mut self, ctx: &egui::Context) {
        let mut open = self.module_open;
        egui::Window::new(t("module.title"))
            .id(egui::Id::new("modules_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(540.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Tabs come from the module registry (single source of truth).
                    for (i, cat) in crate::modules::ModuleCategory::all().iter().enumerate() {
                        if ui.selectable_label(self.module_tab == i as u8, t(cat.title_key())).clicked() {
                            self.module_tab = i as u8;
                        }
                    }
                });
                ui.separator();
                ui.add_space(4.0);
                match self.module_tab {
                    0 => self.module_tab_dictionaries(ui),
                    1 => self.module_tab_language(ui),
                    _ => {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(t("module.reserved")).strong());
                        ui.label(egui::RichText::new(t("module.reserved_hint")).small().weak());
                    }
                }
            });
        self.module_open = open;
    }

    fn module_tab_dictionaries(&mut self, ui: &mut egui::Ui) {
        if ui.checkbox(&mut self.spell_enabled, t("module.enable_spell")).changed() {
            self.segments_dirty = true; // (re)compute or clear issues
            self.show_spelling_panel = self.spell_enabled && self.spell.is_some();
        }
        ui.add_space(4.0);
        match &self.spell {
            Some(sc) => {
                ui.label(egui::RichText::new(
                    format!("{}: {}", t("module.active_dict"), sc.lang())
                ).strong());
            }
            None => {
                ui.label(egui::RichText::new(t("module.no_dict")).weak());
            }
        }
        ui.add_space(6.0);
        if ui.button(t("module.add_dict")).clicked() {
            self.load_dictionary_dialog();
        }
        ui.label(egui::RichText::new(t("module.add_dict_hint")).small().weak());

        ui.add_space(12.0);
        ui.label(egui::RichText::new(t("module.downloadable")).strong());
        ui.label(egui::RichText::new(t("module.downloadable_hint")).small().weak());
        if !self.dict_status.is_empty() {
            ui.label(egui::RichText::new(&self.dict_status).small().color(crate::theme::ACCENT));
        }
        ui.add_space(4.0);
        // State per row: downloading (spinner) → downloaded (green tick + Use) →
        // active (green "in use"). Buttons stay clickable; re-entry is guarded
        // inside start_dict_download.
        let downloading = self
            .dict_dl
            .as_ref()
            .and_then(|s| s.lock().ok().map(|x| x.lang.clone()));
        let active_dict = self.spell.as_ref().map(|sc| sc.lang().to_string());
        let mut to_dl: Option<(String, String)> = None;
        let mut to_use: Option<String> = None;
        for (tag, repo, label) in [
            ("en_US", "en", "English (US)"), ("en_GB", "en-GB", "English (UK)"),
            ("fr_FR", "fr", "Français"), ("de_DE", "de", "Deutsch"),
            ("es_ES", "es", "Español"), ("it_IT", "it", "Italiano"),
        ] {
            // Shared "downloadable resource" convention (modules::downloadable_row).
            let state = if downloading.as_deref() == Some(tag) {
                crate::modules::DlState::Downloading
            } else if active_dict.as_deref() == Some(tag) {
                crate::modules::DlState::Active
            } else if dict_downloaded(tag) {
                crate::modules::DlState::Installed
            } else {
                crate::modules::DlState::NotInstalled
            };
            match crate::modules::downloadable_row(ui, label, state) {
                crate::modules::DlAction::Download | crate::modules::DlAction::Redownload => {
                    to_dl = Some((tag.to_string(), repo.to_string()));
                }
                crate::modules::DlAction::Use => to_use = Some(tag.to_string()),
                crate::modules::DlAction::None => {}
            }
        }
        if let Some((tag, repo)) = to_dl {
            self.start_dict_download(&tag, &repo);
        }
        if let Some(tag) = to_use {
            self.use_downloaded_dictionary(&tag);
        }
    }

    /// Activate an already-downloaded dictionary (the green "Use" action).
    fn use_downloaded_dictionary(&mut self, lang: &str) {
        let Some(dir) = dict_dir() else { return };
        let dic = dir.join(format!("{lang}.dic"));
        let aff = dir.join(format!("{lang}.aff"));
        match (std::fs::read_to_string(&aff), std::fs::read_to_string(&dic)) {
            (Ok(a), Ok(d)) => match mdall_core::spell::SpellChecker::from_aff_dic(&a, &d, lang) {
                Ok(sc) => {
                    self.spell = Some(sc);
                    self.spell_enabled = true;
                    self.show_spelling_panel = true;
                    self.spell_sugg_cache.clear();
                    self.segments_dirty = true;
                    self.dict_status = format!("Using dictionary '{lang}'");
                    self.status_msg = format!("Dictionary '{lang}' active");
                }
                Err(e) => self.dict_status = format!("Failed to load '{lang}': {e}"),
            },
            _ => self.dict_status = format!("Could not read the '{lang}' files"),
        }
    }

    fn module_tab_language(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(t("module.language")).strong());
        ui.label(egui::RichText::new(t("module.language_hint")).small().weak());
        ui.add_space(8.0);
        for (tag, name) in [
            ("en", "English"), ("fr", "Français"), ("de", "Deutsch"),
            ("es", "Español"), ("it", "Italiano"),
        ] {
            if ui.selectable_label(self.app_lang == tag, name).clicked() {
                self.app_lang = tag.to_string();
            }
        }
    }

    /// Pick a Hunspell `.dic` and load it together with its sibling `.aff`.
    fn load_dictionary_dialog(&mut self) {
        let Some(dic_path) = rfd::FileDialog::new()
            .add_filter("Hunspell dictionary", &["dic"])
            .set_title("Add a Hunspell dictionary (.dic)")
            .pick_file()
        else {
            return;
        };
        let aff_path = dic_path.with_extension("aff");
        let lang = dic_path.file_stem().and_then(|s| s.to_str()).unwrap_or("custom").to_string();
        match (std::fs::read_to_string(&aff_path), std::fs::read_to_string(&dic_path)) {
            (Ok(aff), Ok(dic)) => {
                match mdall_core::spell::SpellChecker::from_aff_dic(&aff, &dic, &lang) {
                    Ok(sc) => {
                        self.spell = Some(sc);
                        self.spell_enabled = true;
                        self.show_spelling_panel = true;
                        self.spell_sugg_cache.clear();
                        self.segments_dirty = true; // recompute issues against the new dict
                        self.status_msg = format!("Dictionary '{}' loaded", lang);
                    }
                    Err(e) => self.status_msg = format!("Dictionary error: {e}"),
                }
            }
            _ => {
                self.status_msg =
                    format!("Need both {}.dic and {}.aff in the same folder", lang, lang);
            }
        }
    }

    /// Fill the suggestion cache for the first `limit` issues that lack one.
    /// Disjoint-field borrows (`spell` vs `spell_sugg_cache`) keep this cheap and
    /// run only when new misspelled words appear, never per frame.
    fn ensure_spell_suggestions(&mut self, limit: usize) {
        let need: Vec<String> = self
            .spell_issues
            .iter()
            .take(limit)
            .map(|m| m.word.clone())
            .filter(|w| !self.spell_sugg_cache.contains_key(w))
            .collect();
        if need.is_empty() {
            return;
        }
        if let Some(sc) = self.spell.as_ref() {
            for w in need {
                let s = sc.suggest(&w);
                self.spell_sugg_cache.insert(w, s);
            }
        }
    }

    /// Right-hand panel listing spelling issues with clickable suggestions,
    /// "Add to dictionary" and jump-to-source. Edits go through the source only.
    pub(crate) fn render_spelling_panel(&mut self, ctx: &egui::Context) {
        if !self.show_spelling_panel || !self.spell_enabled || self.spell.is_none() {
            return;
        }
        self.ensure_spell_suggestions(120);

        let mut close = false;
        let mut jump: Option<(usize, usize)> = None;
        let mut replace: Option<(usize, usize, String)> = None;
        let mut add: Option<String> = None;

        egui::SidePanel::right("spelling_panel")
            .resizable(true)
            .default_width(280.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t("panel.spelling")).heading());
                    ui.label(egui::RichText::new(format!("({})", self.spell_issues.len())).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("\u{2715}").clicked() {
                            close = true;
                        }
                    });
                });
                if let Some(sc) = &self.spell {
                    ui.label(egui::RichText::new(
                        format!("{}: {}", t("panel.dictionary"), sc.lang())
                    ).small().weak());
                }
                ui.separator();
                if self.spell_issues.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t("panel.no_issues")).weak());
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for m in self.spell_issues.iter().take(120) {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new(&m.word)
                                .strong()
                                .color(egui::Color32::from_rgb(192, 57, 43)));
                            if let Some(sugg) = self.spell_sugg_cache.get(&m.word) {
                                if sugg.is_empty() {
                                    ui.label(egui::RichText::new(t("panel.no_suggestions")).small().weak());
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        for s in sugg.iter().take(5) {
                                            if ui.small_button(s).clicked() {
                                                replace = Some((m.start, m.end, s.clone()));
                                            }
                                        }
                                    });
                                }
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button(t("panel.add")).clicked() {
                                    add = Some(m.word.clone());
                                }
                                if ui.small_button(t("panel.jump")).clicked() {
                                    jump = Some((m.start, m.end));
                                }
                            });
                        });
                        ui.add_space(3.0);
                    }
                });
            });

        if close {
            self.show_spelling_panel = false;
        }
        if let Some((s, e, txt)) = replace {
            if e <= self.source.len() && self.source.is_char_boundary(s) && self.source.is_char_boundary(e) {
                self.source.replace_range(s..e, &txt);
                self.modified = true;
                self.segments_dirty = true;
            }
        } else if let Some(w) = add {
            if let Some(sc) = self.spell.as_mut() {
                sc.add_word(&w);
            }
            self.spell_sugg_cache.remove(&w);
            self.segments_dirty = true;
        } else if let Some((s, e)) = jump {
            self.selection_anchor = byte_to_char_index(&self.source, s);
            self.cursor_pos = byte_to_char_index(&self.source, e);
            if self.view_mode == ViewMode::Editor {
                self.view_mode = ViewMode::Split;
            }
            self.request_source_focus = true;
            self.apply_cursor_to_editor_state();
        }
    }

    /// Start an opt-in background download of a dictionary (wooorm/dictionaries)
    /// into `<exe-dir>/dictionaries/`. The UI polls [`poll_dict_download`].
    fn start_dict_download(&mut self, lang: &str, repo: &str) {
        if self.dict_dl.is_some() {
            self.dict_status = "A download is already running...".into();
            return;
        }
        let Some(dir) = dict_dir() else {
            self.dict_status = "Cannot locate the dictionaries folder".into();
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            self.dict_status = "Cannot create the dictionaries folder".into();
            return;
        }
        let state = std::sync::Arc::new(std::sync::Mutex::new(DictDownload {
            lang: lang.to_string(),
            done: false,
            error: None,
        }));
        self.dict_dl = Some(state.clone());
        self.dict_status = format!("Downloading {lang}...");
        self.status_msg = format!("Downloading {lang} dictionary...");
        let lang = lang.to_string();
        let repo = repo.to_string();
        std::thread::spawn(move || {
            let base = format!(
                "https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries/{repo}"
            );
            // Download as raw bytes (dictionaries can be multi-MB and need no
            // UTF-8 assumption); write straight to disk.
            let fetch = |url: String| -> Result<Vec<u8>, String> {
                let resp = ureq::get(&url).call().map_err(|e| e.to_string())?;
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut resp.into_reader(), &mut buf)
                    .map_err(|e| e.to_string())?;
                Ok(buf)
            };
            let res = (|| -> Result<(), String> {
                let dic = fetch(format!("{base}/index.dic"))?;
                let aff = fetch(format!("{base}/index.aff"))?;
                std::fs::write(dir.join(format!("{lang}.dic")), &dic).map_err(|e| e.to_string())?;
                std::fs::write(dir.join(format!("{lang}.aff")), &aff).map_err(|e| e.to_string())?;
                Ok(())
            })();
            if let Ok(mut s) = state.lock() {
                s.error = res.err();
                s.done = true;
            }
        });
    }

    /// Poll the in-flight download; on completion, load the new dictionary.
    pub(crate) fn poll_dict_download(&mut self, ctx: &egui::Context) {
        let Some(state) = self.dict_dl.clone() else { return };
        ctx.request_repaint(); // keep the loop alive while the thread runs
        let (done, lang, error) = {
            let Ok(s) = state.lock() else { return };
            (s.done, s.lang.clone(), s.error.clone())
        };
        if !done {
            return;
        }
        self.dict_dl = None;
        if let Some(e) = error {
            self.dict_status = format!("Download failed: {e}");
            self.status_msg = format!("Download failed: {e}");
            return;
        }
        if let Some(dir) = dict_dir() {
            let dic = dir.join(format!("{lang}.dic"));
            let aff = dir.join(format!("{lang}.aff"));
            if let (Ok(a), Ok(d)) =
                (std::fs::read_to_string(&aff), std::fs::read_to_string(&dic))
            {
                match mdall_core::spell::SpellChecker::from_aff_dic(&a, &d, &lang) {
                    Ok(sc) => {
                        self.spell = Some(sc);
                        self.spell_enabled = true;
                        self.show_spelling_panel = true;
                        self.spell_sugg_cache.clear();
                        self.segments_dirty = true;
                        self.dict_status = format!("Dictionary '{lang}' downloaded and active");
                        self.status_msg = format!("Dictionary '{lang}' downloaded");
                        return;
                    }
                    Err(e) => {
                        self.dict_status = format!("Downloaded '{lang}' but parse failed: {e}");
                        return;
                    }
                }
            }
        }
        self.dict_status = format!("Downloaded '{lang}' but could not read the files");
    }

    /// Auto-load the first Hunspell dictionary found in `<exe-dir>/dictionaries/`
    /// at startup (this is where downloaded/bundled dictionaries land). Silently
    /// does nothing if the folder or a valid `.dic`+`.aff` pair is absent.
    pub(crate) fn autoload_default_dictionary(&mut self) {
        let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("dictionaries")))
        else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|x| x.to_str()) != Some("dic") {
                continue;
            }
            let aff = p.with_extension("aff");
            if let (Ok(a), Ok(d)) = (std::fs::read_to_string(&aff), std::fs::read_to_string(&p)) {
                let lang = p.file_stem().and_then(|s| s.to_str()).unwrap_or("dict").to_string();
                if let Ok(sc) = mdall_core::spell::SpellChecker::from_aff_dic(&a, &d, &lang) {
                    self.spell = Some(sc);
                    // Dictionary loaded and ready, but spell mode stays OFF until the
                    // user turns it on with the ABC toggle (no automatic display).
                    self.spell_enabled = false;
                    self.show_spelling_panel = false;
                    break;
                }
            }
        }
    }

    /// Toggle the spell-check mode (red squiggles + the suggestions panel). Off by
    /// default; the ABC button drives it. Loads a bundled dictionary on first use.
    pub(crate) fn toggle_spell_mode(&mut self) {
        if self.spell.is_none() {
            self.autoload_default_dictionary();
        }
        if self.spell.is_none() {
            self.status_msg = "No dictionary available (add one in Modules > Language)".into();
            return;
        }
        self.spell_enabled = !self.spell_enabled;
        self.show_spelling_panel = self.spell_enabled;
        self.segments_dirty = true; // recompute issues for the new state
    }

    /// Persist this document's review annotations beside `doc` as a sidecar JSON,
    /// so in-app comments and imported reviewer feedback survive save/reload.
    /// Removes the sidecar when there are no annotations left.
    pub(crate) fn save_annotations(&self, doc: &std::path::Path) {
        let path = annotations_path(doc);
        if self.review_items.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        let stored: Vec<StoredReview> = self
            .review_items
            .iter()
            .map(|r| StoredReview {
                kind: r.kind.label().to_string(),
                author: r.author.clone(),
                date: r.date.clone(),
                text: r.text.clone(),
                context: r.context.clone(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string_pretty(&stored) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Load review annotations saved beside `doc`, if a sidecar exists.
    pub(crate) fn load_annotations(&mut self, doc: &std::path::Path) {
        // A freshly opened document starts with no annotations; a sidecar (if
        // present) repopulates them.
        self.review_items.clear();
        self.show_review_panel = false;
        let Ok(text) = std::fs::read_to_string(annotations_path(doc)) else { return };
        let Ok(stored) = serde_json::from_str::<Vec<StoredReview>>(&text) else { return };
        self.review_items = stored
            .into_iter()
            .map(|s| mdall_core::docx_review::ReviewItem {
                kind: review_kind_from_label(&s.kind),
                author: s.author,
                date: s.date,
                text: s.text,
                context: s.context,
            })
            .collect();
        self.show_review_panel = !self.review_items.is_empty();
    }
}

/// Sidecar storage for a document's review annotations (comments + tracked
/// changes), kept in `<doc>.annotations.json` so they travel with the document.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredReview {
    kind: String,
    author: String,
    date: String,
    text: String,
    context: String,
}

fn annotations_path(doc: &std::path::Path) -> std::path::PathBuf {
    let mut s = doc.as_os_str().to_os_string();
    s.push(".annotations.json");
    std::path::PathBuf::from(s)
}

fn review_kind_from_label(s: &str) -> mdall_core::docx_review::ReviewKind {
    use mdall_core::docx_review::ReviewKind;
    match s {
        "Insertion" => ReviewKind::Insertion,
        "Deletion" => ReviewKind::Deletion,
        _ => ReviewKind::Comment,
    }
}
