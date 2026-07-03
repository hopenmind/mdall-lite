//! Plain UI state structs used by `MdApp` (no egui types, no logic).
//! Fields are `pub` because `MdApp` (in the crate root) constructs and reads
//! them across the module boundary.

use crate::output_format::OutputFormat;
use std::path::PathBuf;

/// Three-phase state machine for the Conversion Hub.
#[derive(PartialEq, Clone, Copy)]
#[allow(dead_code)] // FormatPick reserved for the conversion-grid phase
pub enum HubPhase {
    /// No file loaded - show drop zone + Browse button.
    Idle,
    /// At least one file loaded - show file list + actions.
    FileReady,
    /// User clicked Convert - show format grid.
    FormatPick,
}

/// Per-file status in the conversion queue.
#[derive(PartialEq, Clone, Copy)]
pub enum FileStatus {
    /// Loaded, not yet converted.
    Pending,
    /// Converted successfully.
    Done,
    /// Conversion failed (see `message`).
    Failed,
}

/// One file loaded into the hub (drag&drop or browse).
pub struct HubFile {
    pub path:        PathBuf,
    pub status:      FileStatus,
    /// Output path of the last successful conversion of this file.
    pub output_path: Option<PathBuf>,
    /// Per-file status / error message.
    pub message:     String,
    /// Per-file target format override. `None` = use the batch target.
    pub target:      Option<OutputFormat>,
}

impl HubFile {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            status: FileStatus::Pending,
            output_path: None,
            message: String::new(),
            target: None,
        }
    }
}

/// Conversion Hub runtime state (multi-file + batch queue).
pub struct ConversionHub {
    pub phase:        HubPhase,
    /// All loaded files (one = single-file flow, many = batch flow).
    pub files:        Vec<HubFile>,
    /// Markdown source after import of the single file (for "Open in Editor").
    pub converted_md: Option<String>,
    /// Index of the file whose per-file options panel is expanded.
    pub selected:     Option<usize>,
    /// Chosen output format for the batch (per-file `target` overrides it).
    pub batch_target: Option<OutputFormat>,
    /// True while the format-picker grid is shown.
    pub pick_format:  bool,
    /// Global status / error message.
    pub status:       String,
    pub is_error:     bool,
    /// True while a file is being dragged over the window.
    pub hovering:     bool,
    /// True while the batch queue is being processed (one file per frame).
    pub converting:   bool,
    /// Next file index to process in the batch queue.
    pub queue_index:  usize,
}

impl Default for ConversionHub {
    fn default() -> Self {
        Self {
            phase:        HubPhase::Idle,
            files:        Vec::new(),
            converted_md: None,
            selected:     None,
            batch_target: None,
            pick_format:  false,
            status:       String::new(),
            is_error:     false,
            hovering:     false,
            converting:   false,
            queue_index:  0,
        }
    }
}

/// Conversion output path settings.
pub struct ConversionSettings {
    /// false = SaveAs dialog (default), true = auto-save next to source.
    pub auto_save: bool,
    /// false = suffix (default), true = prefix.
    pub use_prefix: bool,
    /// Affix string added to the output filename. Default: "MDALL".
    pub affix: String,
}

impl Default for ConversionSettings {
    fn default() -> Self {
        Self { auto_save: false, use_prefix: false, affix: "MDALL".into() }
    }
}

pub struct EquationEditor {
    pub visible: bool,
    pub latex: String,
    /// Display equation: block index.  Inline equation: unused (0).
    pub index: usize,
    /// True when editing an inline $...$ or \(...\) equation.
    pub is_inline: bool,
    /// Inline only: byte range of the containing Paragraph block in source.
    pub inline_block_range: std::ops::Range<usize>,
    /// Inline only: original opening delimiter ("$" or "\\(").
    pub inline_delim_open: String,
    /// Inline only: original closing delimiter ("$" or "\\)").
    pub inline_delim_close: String,
    /// Inline only: original latex content before editing (used to evict old texture on Apply).
    pub inline_orig_latex: String,
    /// Inline only: index of the clicked run inside the paragraph's Vec<InlineRun>.
    pub inline_run_idx: usize,
}

/// Inline format state at the WYSIWYG cursor - updated every frame.
/// Used to highlight toolbar buttons and detect current formatting.
#[derive(Default, Clone, Copy)]
pub struct WysiwygFormatState {
    pub bold:          bool,
    pub italic:        bool,
    pub code:          bool,
    pub strikethrough: bool,
    /// 0 = not inside a heading; 1-6 = heading level at cursor.
    pub heading:       u8,
}

pub struct LinkDialog {
    pub visible: bool,
    pub text: String,
    pub url: String,
    pub is_image: bool,
}

/// Comment-authoring dialog: create a Review comment anchored to selected text.
pub struct CommentDialog {
    pub visible: bool,
    /// The selected passage the comment refers to (shown read-only as the anchor).
    pub anchor: String,
    /// The comment body being typed.
    pub body: String,
}

pub struct ExportDialog {
    pub visible: bool,
}

/// Properties popup for editing a standalone image (alt / url / width / align).
/// `replace` is the source byte range of the image block to overwrite on Apply.
pub struct ImageDialog {
    pub visible: bool,
    pub alt: String,
    pub url: String,
    /// Width in px as typed; empty = auto (no width attribute).
    pub width: String,
    pub align: crate::ui::editor::ImgAlign,
    pub replace: std::ops::Range<usize>,
}

/// Column text alignment in the visual table editor (maps to the GFM separator
/// markers `:---`, `:---:`, `---:`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ColAlign {
    Left,
    Center,
    Right,
}

/// Visual table editor: a grid of cells (row 0 is the header) with per-column
/// alignment, serialized to a GFM pipe table on insert. `replace` holds the
/// source byte range of an existing table to overwrite (None = insert at cursor).
pub struct TableDialog {
    pub visible: bool,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Vec<String>>,
    pub aligns: Vec<ColAlign>,
    pub replace: Option<std::ops::Range<usize>>,
}

impl Default for TableDialog {
    fn default() -> Self {
        Self {
            visible: false,
            rows: 3,
            cols: 3,
            cells: vec![vec![String::new(); 3]; 3],
            aligns: vec![ColAlign::Left; 3],
            replace: None,
        }
    }
}

impl TableDialog {
    /// Reset to a fresh NxM grid (header + body), ready to insert at the cursor.
    pub fn reset(&mut self, rows: usize, cols: usize) {
        self.rows = rows.max(2);
        self.cols = cols.max(1);
        self.cells = vec![vec![String::new(); self.cols]; self.rows];
        self.aligns = vec![ColAlign::Left; self.cols];
        self.replace = None;
    }

    pub fn add_row(&mut self) {
        self.cells.push(vec![String::new(); self.cols]);
        self.rows += 1;
    }

    pub fn del_row(&mut self) {
        if self.rows > 2 {
            self.cells.pop();
            self.rows -= 1;
        }
    }

    pub fn add_col(&mut self) {
        for row in &mut self.cells {
            row.push(String::new());
        }
        self.aligns.push(ColAlign::Left);
        self.cols += 1;
    }

    pub fn del_col(&mut self) {
        if self.cols > 1 {
            for row in &mut self.cells {
                row.pop();
            }
            self.aligns.pop();
            self.cols -= 1;
        }
    }

    /// Serialize the grid to a GFM pipe table (row 0 is the header row).
    pub fn to_markdown(&self) -> String {
        let cell = |r: usize, c: usize| -> String {
            self.cells
                .get(r)
                .and_then(|row| row.get(c))
                .map(|s| s.replace('|', "\\|").replace('\n', " "))
                .unwrap_or_default()
        };
        let mut out = String::new();
        out.push('|');
        for c in 0..self.cols {
            out.push_str(&format!(" {} |", cell(0, c)));
        }
        out.push('\n');
        out.push('|');
        for c in 0..self.cols {
            out.push_str(match self.aligns.get(c).copied().unwrap_or(ColAlign::Left) {
                ColAlign::Left => " :--- |",
                ColAlign::Center => " :---: |",
                ColAlign::Right => " ---: |",
            });
        }
        out.push('\n');
        for r in 1..self.rows {
            out.push('|');
            for c in 0..self.cols {
                out.push_str(&format!(" {} |", cell(r, c)));
            }
            out.push('\n');
        }
        out
    }

    /// Parse a GFM pipe table (header + separator + body) into a grid. Returns
    /// None when `src` is not a well-formed table (needs a separator row whose
    /// cells are all dashes, optionally colon-anchored).
    pub fn from_markdown(src: &str) -> Option<Self> {
        let lines: Vec<&str> = src.lines().filter(|l| l.trim_start().starts_with('|')).collect();
        if lines.len() < 2 {
            return None;
        }
        let split = |line: &str| -> Vec<String> {
            let t = line.trim();
            let t = t.strip_prefix('|').unwrap_or(t);
            let t = t.strip_suffix('|').unwrap_or(t);
            // Split on unescaped pipes.
            let mut cells = Vec::new();
            let mut cur = String::new();
            let mut esc = false;
            for ch in t.chars() {
                if esc {
                    cur.push(ch);
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == '|' {
                    cells.push(cur.trim().to_string());
                    cur = String::new();
                } else {
                    cur.push(ch);
                }
            }
            cells.push(cur.trim().to_string());
            cells
        };
        let header = split(lines[0]);
        let sep = split(lines[1]);
        // Validate the separator row: every cell is dashes with optional colons.
        let is_sep = !sep.is_empty()
            && sep.iter().all(|c| {
                let core = c.trim();
                !core.is_empty()
                    && core.chars().all(|ch| ch == '-' || ch == ':')
                    && core.contains('-')
            });
        if !is_sep {
            return None;
        }
        let cols = header.len().max(sep.len());
        let aligns: Vec<ColAlign> = (0..cols)
            .map(|c| {
                let s = sep.get(c).map(|x| x.trim()).unwrap_or("---");
                let left = s.starts_with(':');
                let right = s.ends_with(':');
                match (left, right) {
                    (true, true) => ColAlign::Center,
                    (false, true) => ColAlign::Right,
                    _ => ColAlign::Left,
                }
            })
            .collect();
        let mut cells: Vec<Vec<String>> = Vec::new();
        let mut push_row = |row: Vec<String>| {
            let mut r = row;
            r.resize(cols, String::new());
            cells.push(r);
        };
        push_row(header);
        for line in &lines[2..] {
            push_row(split(line));
        }
        let rows = cells.len();
        Some(Self { visible: true, rows, cols, cells, aligns, replace: None })
    }
}

#[cfg(test)]
mod table_tests {
    use super::{ColAlign, TableDialog};

    #[test]
    fn to_markdown_has_header_separator_body() {
        let mut td = TableDialog::default(); // 3x3
        td.cells[0] = vec!["A".into(), "B".into(), "C".into()];
        td.cells[1] = vec!["1".into(), "2".into(), "3".into()];
        td.aligns[1] = ColAlign::Center;
        let lines: Vec<String> = td.to_markdown().lines().map(|s| s.to_string()).collect();
        assert!(lines[0].starts_with("| A | B | C |"));
        assert!(lines[1].contains(":---:"), "center column marker missing: {}", lines[1]);
        assert!(lines[2].starts_with("| 1 | 2 | 3 |"));
    }

    #[test]
    fn from_markdown_round_trips() {
        let src = "| H1 | H2 |\n| :--- | ---: |\n| a | b |\n| c | d |\n";
        let td = TableDialog::from_markdown(src).expect("should parse a GFM table");
        assert_eq!(td.cols, 2);
        assert_eq!(td.rows, 3);
        assert_eq!(td.cells[0], vec!["H1".to_string(), "H2".to_string()]);
        assert_eq!(td.aligns[1], ColAlign::Right);
        let md = td.to_markdown();
        assert!(md.contains("| a | b |"));
        assert!(md.lines().nth(1).unwrap().contains("---:"));
    }

    #[test]
    fn from_markdown_rejects_non_tables() {
        assert!(TableDialog::from_markdown("just text\nmore text").is_none());
        // Pipe rows but no dashed separator on the second line.
        assert!(TableDialog::from_markdown("| a | b |\n| c | d |").is_none());
    }

    #[test]
    fn pipes_in_cells_are_escaped() {
        let mut td = TableDialog::default();
        td.cells[1][0] = "a|b".into();
        assert!(td.to_markdown().contains("a\\|b"));
    }
}

/// Editor rendering mode, switchable in the Options panel.
/// Default is the continuous segmented flow; Block is the legacy click-to-edit model.
#[derive(PartialEq, Clone, Copy)]
pub enum EditorMode {
    /// Default: continuous segmented flow (text runs + inline rendered equations).
    SegmentedFlow,
    /// Block model: click a block to open an inline source editor.
    Block,
}

impl Default for EditorMode {
    fn default() -> Self { EditorMode::SegmentedFlow }
}

/// Severity of a transient toast notification.
#[derive(PartialEq, Clone, Copy)]
#[allow(dead_code)] // toast notification API, not all severities wired yet
pub enum ToastKind {
    Success,
    Error,
    Info,
}

/// A transient, auto-dismissing notification (shown bottom-right of the window).
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    /// Seconds left before auto-dismiss; decremented by the frame delta each update.
    pub remaining: f32,
}

#[allow(dead_code)] // toast constructors, wired incrementally
impl Toast {
    /// Default on-screen lifetime, in seconds.
    pub const DEFAULT_SECS: f32 = 4.0;

    pub fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        Self { message: message.into(), kind, remaining: Self::DEFAULT_SECS }
    }
    pub fn success(message: impl Into<String>) -> Self { Self::new(message, ToastKind::Success) }
    pub fn error(message: impl Into<String>)   -> Self { Self::new(message, ToastKind::Error) }
    pub fn info(message: impl Into<String>)     -> Self { Self::new(message, ToastKind::Info) }
}
