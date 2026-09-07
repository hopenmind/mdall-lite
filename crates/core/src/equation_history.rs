//! Equation edit history - "git for equations".
//!
//! The TEMPORAL companion to [`crate::source_embed`]: where source_embed recovers
//! the CURRENT source from a shared file (spatial recovery), this keeps ALL past
//! versions of every equation, locally, timestamped, with revert.
//!
//! - Each display equation (`$$...$$`) carries a stable injected id, an HTML-comment
//!   meta tag `<!--mdall-eq:ID-->` placed right after its block, so the same equation
//!   is tracked across reloads and reorders.
//! - The log lives under `%APPDATA%/<app>/history/<doc-id>.xml` (Roaming), one file
//!   per document, append-only. A revert never rewrites history: it appends a new
//!   change that re-posts an older version.
//! - Exposed to the UI (a history panel) and over MCP (list / revert / lint).
//!
//! Pure logic (no egui). Timestamps via `chrono`; XML via hand-rolled writing +
//! `quick-xml` reading (attributes only, so parsing stays trivial and robust).

use std::path::{Path, PathBuf};

// ── Data model ──────────────────────────────────────────────────────────

/// One recorded change to an equation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquationChange {
    /// RFC-3339 UTC timestamp.
    pub ts: String,
    /// Who made the change: `"user"`, `"import"`, or `"mcp"`.
    pub by: String,
    /// LaTeX before the change.
    pub from: String,
    /// LaTeX after the change.
    pub to: String,
    /// Optional human note (e.g. an MCP lint reason).
    pub reason: Option<String>,
}

/// The full history of one equation, identified by its injected id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquationRecord {
    pub id: String,
    /// First-seen LaTeX (the "origin" version).
    pub origin: String,
    pub changes: Vec<EquationChange>,
}

impl EquationRecord {
    /// The latest LaTeX: the last change's `to`, or the origin if never changed.
    pub fn current(&self) -> &str {
        self.changes.last().map(|c| c.to.as_str()).unwrap_or(&self.origin)
    }
    /// Number of recorded changes.
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}

/// The history of one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocHistory {
    pub doc_id: String,
    pub initial_file: String,
    pub created: String,
    pub equations: Vec<EquationRecord>,
}

impl DocHistory {
    pub fn equation(&self, id: &str) -> Option<&EquationRecord> {
        self.equations.iter().find(|e| e.id == id)
    }
    fn equation_mut(&mut self, id: &str) -> Option<&mut EquationRecord> {
        self.equations.iter_mut().find(|e| e.id == id)
    }
}

// ── Identity (no uuid dependency) ───────────────────────────────────────

/// A short stable id from a seed string (FNV-1a 64 -> 12 hex). Deterministic,
/// so tests are stable; collisions within a single document are astronomically
/// unlikely and the injection nonce (order) disambiguates identical LaTeX.
pub fn make_id(seed: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:012x}", h & 0x0000_ffff_ffff_ffff)
}

/// Document id from its initial file path (stable per location).
pub fn doc_id_for_path(path: &str) -> String {
    make_id(path)
}

/// Current time as an RFC-3339 UTC string.
pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── Injection: <!--mdall-eq:ID--> markers next to $$...$$ blocks ─────────

const MARK_OPEN: &str = "<!--mdall-eq:";
const MARK_CLOSE: &str = "-->";

/// Byte ranges + LaTeX of each display equation (`$$...$$`) in document order.
/// `end` is the offset just past the closing `$$`.
fn scan_display_equations(md: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = md[i..].find("$$") {
        let open = i + rel;
        let content_start = open + 2;
        if content_start > md.len() {
            break;
        }
        if let Some(rel2) = md[content_start..].find("$$") {
            let close = content_start + rel2;
            let end = close + 2;
            let latex = md[content_start..close].trim().to_string();
            out.push((open, end, latex));
            i = end;
        } else {
            break;
        }
    }
    out
}

/// If an `<!--mdall-eq:ID-->` marker immediately follows (ignoring whitespace),
/// return its ID.
fn marker_right_after(s: &str) -> Option<String> {
    let t = s.trim_start_matches([' ', '\t', '\r', '\n']);
    if let Some(rest) = t.strip_prefix(MARK_OPEN) {
        if let Some(idx) = rest.find(MARK_CLOSE) {
            let id = rest[..idx].trim().to_string();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

/// Ensure every `$$...$$` block has an id marker, injecting the missing ones.
/// Returns the (possibly modified) markdown and the ordered `(id, latex)` list.
pub fn ensure_ids(md: &str) -> (String, Vec<(String, String)>) {
    let blocks = scan_display_equations(md);
    if blocks.is_empty() {
        return (md.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(md.len() + blocks.len() * 32);
    let mut ids = Vec::with_capacity(blocks.len());
    let mut last = 0usize;
    for (ordinal, (_start, end, latex)) in blocks.iter().enumerate() {
        out.push_str(&md[last..*end]);
        match marker_right_after(&md[*end..]) {
            Some(id) => ids.push((id, latex.clone())),
            None => {
                let id = make_id(&format!("{}\u{1}{}", latex, ordinal));
                out.push_str(&format!("\n{}{}{}", MARK_OPEN, id, MARK_CLOSE));
                ids.push((id, latex.clone()));
            }
        }
        last = *end;
    }
    out.push_str(&md[last..]);
    (out, ids)
}

/// The ordered `(id, latex)` list already present in the markdown (no injection).
pub fn scan_ids(md: &str) -> Vec<(String, String)> {
    scan_display_equations(md)
        .into_iter()
        .map(|(_s, end, latex)| (marker_right_after(&md[end..]).unwrap_or_default(), latex))
        .collect()
}

// ── XML store (write hand-rolled, read via quick-xml, attributes only) ──

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            _ => o.push(c),
        }
    }
    o
}

/// Serialize a document history to XML.
pub fn to_xml(h: &DocHistory) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<mdall-history v=\"1\" doc-id=\"{}\" initial-file=\"{}\" created=\"{}\">\n",
        xml_escape(&h.doc_id),
        xml_escape(&h.initial_file),
        xml_escape(&h.created),
    ));
    for eq in &h.equations {
        s.push_str(&format!(
            "  <equation id=\"{}\" origin=\"{}\">\n",
            xml_escape(&eq.id),
            xml_escape(&eq.origin),
        ));
        for c in &eq.changes {
            s.push_str(&format!(
                "    <change ts=\"{}\" by=\"{}\" from=\"{}\" to=\"{}\"{}/>\n",
                xml_escape(&c.ts),
                xml_escape(&c.by),
                xml_escape(&c.from),
                xml_escape(&c.to),
                match &c.reason {
                    Some(r) => format!(" reason=\"{}\"", xml_escape(r)),
                    None => String::new(),
                },
            ));
        }
        s.push_str("  </equation>\n");
    }
    s.push_str("</mdall-history>\n");
    s
}

/// Parse a document history from XML. Tolerant: unknown elements are ignored.
pub fn from_xml(xml: &str) -> DocHistory {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut hist = DocHistory::default();

    let attr = |e: &quick_xml::events::BytesStart, key: &[u8]| -> Option<String> {
        e.attributes().flatten().find(|a| a.key.as_ref() == key).map(|a| {
            a.unescape_value().map(|v| v.into_owned()).unwrap_or_default()
        })
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"mdall-history" => {
                    hist.doc_id = attr(&e, b"doc-id").unwrap_or_default();
                    hist.initial_file = attr(&e, b"initial-file").unwrap_or_default();
                    hist.created = attr(&e, b"created").unwrap_or_default();
                }
                b"equation" => {
                    hist.equations.push(EquationRecord {
                        id: attr(&e, b"id").unwrap_or_default(),
                        origin: attr(&e, b"origin").unwrap_or_default(),
                        changes: Vec::new(),
                    });
                }
                b"change" => {
                    if let Some(eq) = hist.equations.last_mut() {
                        eq.changes.push(EquationChange {
                            ts: attr(&e, b"ts").unwrap_or_default(),
                            by: attr(&e, b"by").unwrap_or_default(),
                            from: attr(&e, b"from").unwrap_or_default(),
                            to: attr(&e, b"to").unwrap_or_default(),
                            reason: attr(&e, b"reason"),
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    hist
}

// ── Filesystem (testable: the FS functions take an explicit dir) ────────

/// The history directory `%APPDATA%/<app_dir>/history/` (falls back next to the
/// executable). `app_dir` is `"MD-ALL"` (classic) or `"MD-ALL-lite"` (lite).
pub fn history_dir(app_dir: &str) -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        })?;
    Some(base.join(app_dir).join("history"))
}

fn history_file_in(dir: &Path, doc_id: &str) -> PathBuf {
    dir.join(format!("{}.xml", doc_id))
}

/// Load a document history from a specific directory (empty if absent/corrupt).
pub fn load_in(dir: &Path, doc_id: &str) -> DocHistory {
    let path = history_file_in(dir, doc_id);
    match std::fs::read_to_string(&path) {
        Ok(xml) => from_xml(&xml),
        Err(_) => DocHistory::default(),
    }
}

/// Save a document history into a specific directory.
pub fn save_in(dir: &Path, h: &DocHistory) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(history_file_in(dir, &h.doc_id), to_xml(h))
}

/// Record one change into a specific directory. Creates the equation record on
/// first sight (setting its origin to `from`). Idempotent-safe: a no-op change
/// (`from == to`) is not recorded.
#[allow(clippy::too_many_arguments)]
pub fn record_change_in(
    dir: &Path,
    doc_id: &str,
    initial_file: &str,
    eq_id: &str,
    from: &str,
    to: &str,
    by: &str,
    reason: Option<&str>,
) -> std::io::Result<()> {
    if from == to || eq_id.is_empty() {
        return Ok(());
    }
    let mut h = load_in(dir, doc_id);
    if h.doc_id.is_empty() {
        h.doc_id = doc_id.to_string();
        h.initial_file = initial_file.to_string();
        h.created = now_ts();
    }
    let change = EquationChange {
        ts: now_ts(),
        by: by.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        reason: reason.map(|s| s.to_string()),
    };
    match h.equation_mut(eq_id) {
        Some(rec) => rec.changes.push(change),
        None => h.equations.push(EquationRecord {
            id: eq_id.to_string(),
            origin: from.to_string(),
            changes: vec![change],
        }),
    }
    save_in(dir, &h)
}

/// Revert an equation to a prior version in a specific directory. `target_ts` is
/// either `"origin"` or the exact `ts` of a recorded change. Records the revert as
/// a NEW change and returns the target LaTeX for the caller to apply to the source.
pub fn revert_in(
    dir: &Path,
    doc_id: &str,
    eq_id: &str,
    target_ts: &str,
    by: &str,
) -> Option<String> {
    let mut h = load_in(dir, doc_id);
    let rec = h.equation_mut(eq_id)?;
    let target = if target_ts == "origin" {
        rec.origin.clone()
    } else {
        rec.changes.iter().find(|c| c.ts == target_ts)?.to.clone()
    };
    let current = rec.current().to_string();
    if current != target {
        rec.changes.push(EquationChange {
            ts: now_ts(),
            by: by.to_string(),
            from: current,
            to: target.clone(),
            reason: Some(format!("revert to {}", target_ts)),
        });
        let _ = save_in(dir, &h);
    }
    Some(target)
}

// ── Convenience wrappers resolving the real AppData dir ─────────────────

/// Load the history for a document by its initial path.
pub fn history_for(app_dir: &str, initial_file: &str) -> DocHistory {
    match history_dir(app_dir) {
        Some(dir) => load_in(&dir, &doc_id_for_path(initial_file)),
        None => DocHistory::default(),
    }
}

/// Record a change for a document by its initial path.
pub fn record_change(
    app_dir: &str,
    initial_file: &str,
    eq_id: &str,
    from: &str,
    to: &str,
    by: &str,
    reason: Option<&str>,
) {
    if let Some(dir) = history_dir(app_dir) {
        let doc_id = doc_id_for_path(initial_file);
        let _ = record_change_in(&dir, &doc_id, initial_file, eq_id, from, to, by, reason);
    }
}

/// Revert a document's equation by its initial path.
pub fn revert(app_dir: &str, initial_file: &str, eq_id: &str, target_ts: &str, by: &str) -> Option<String> {
    let dir = history_dir(app_dir)?;
    revert_in(&dir, &doc_id_for_path(initial_file), eq_id, target_ts, by)
}

// ── Locate / rewrite / lint an equation by id (for MCP + the UI) ────────

/// Replace the LaTeX of the display equation carrying `id` (its injected marker).
/// Returns the modified markdown, or None if no block carries that id.
pub fn set_equation_latex(md: &str, id: &str, new_latex: &str) -> Option<String> {
    for (start, end, _latex) in scan_display_equations(md) {
        if marker_right_after(&md[end..]).as_deref() == Some(id) {
            let mut out = String::with_capacity(md.len() + new_latex.len());
            out.push_str(&md[..start]);
            out.push_str(&format!("$$\n{}\n$$", new_latex.trim()));
            out.push_str(&md[end..]);
            return Some(out);
        }
    }
    None
}

/// A cheap well-formedness check: balanced `{}` and matching `\left`/`\right`.
/// Returns a short problem description, or None if it looks fine.
pub fn latex_problem(latex: &str) -> Option<String> {
    let mut depth = 0i32;
    for c in latex.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Some("unbalanced '}'".to_string());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Some(format!("{} unclosed '{{'", depth));
    }
    let lefts = latex.matches("\\left").count();
    let rights = latex.matches("\\right").count();
    if lefts != rights {
        return Some(format!("\\left/\\right mismatch ({} vs {})", lefts, rights));
    }
    None
}

// ── Clean editor buffer <-> on-disk markers (strip on load / inject on save) ──
//
// The egui editor must never SEE the markers (its wysiwyg layer renders HTML
// comments as faint-but-present chars, which would clutter the view and offset
// the caret map). So the in-memory editing buffer is kept marker-free; markers
// live only in the saved .md. `strip_ids` cleans a loaded file; `inject_ids`
// re-applies the SAME ids (by document order) when saving, preserving identity
// across reloads even after an equation's LaTeX changed.

/// Remove every `<!--mdall-eq:ID-->` marker (and the single `\n` injected before
/// it), yielding a clean editor buffer.
pub fn strip_ids(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut last = 0usize;
    let mut search = 0usize;
    while let Some(rel) = md[search..].find(MARK_OPEN) {
        let mstart = search + rel;
        match md[mstart..].find(MARK_CLOSE) {
            Some(crel) => {
                let mend = mstart + crel + MARK_CLOSE.len();
                let mut seg_end = mstart;
                if md[..mstart].ends_with('\n') {
                    seg_end -= 1; // also drop the '\n' we added before the marker
                }
                out.push_str(&md[last..seg_end]);
                last = mend;
                search = mend;
            }
            None => break,
        }
    }
    out.push_str(&md[last..]);
    out
}

/// Re-apply ids (in document order) as markers after each `$$...$$` block. An
/// equation beyond the id list gets a fresh id; extra ids are ignored.
pub fn inject_ids(md: &str, ids: &[(String, String)]) -> String {
    let blocks = scan_display_equations(md);
    if blocks.is_empty() {
        return md.to_string();
    }
    let mut out = String::with_capacity(md.len() + blocks.len() * 32);
    let mut last = 0usize;
    for (i, (_s, end, latex)) in blocks.iter().enumerate() {
        out.push_str(&md[last..*end]);
        let id = ids
            .get(i)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| make_id(&format!("{}\u{1}{}", latex, i)));
        out.push_str(&format!("\n{}{}{}", MARK_OPEN, id, MARK_CLOSE));
        last = *end;
    }
    out.push_str(&md[last..]);
    out
}

/// The document-order index of the display equation (`$$...$$`) whose block
/// contains byte offset `byte`, or None if `byte` is not inside one. Lets the UI
/// map an edited block back to its entry in the ordered id list.
pub fn equation_index_at(md: &str, byte: usize) -> Option<usize> {
    scan_display_equations(md)
        .into_iter()
        .position(|(start, end, _)| byte >= start && byte < end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_short() {
        assert_eq!(make_id("E = mc^2"), make_id("E = mc^2"));
        assert_ne!(make_id("a"), make_id("b"));
        assert_eq!(make_id("x").len(), 12);
    }

    #[test]
    fn ensure_ids_injects_then_scans() {
        let md = "text\n\n$$\nE = mc^2\n$$\n\nmore\n\n$$ x+y $$\n";
        let (out, ids) = ensure_ids(md);
        assert_eq!(ids.len(), 2, "two display equations");
        assert!(out.contains("<!--mdall-eq:"), "markers injected");
        // Re-running is idempotent: same ids, no new markers.
        let (out2, ids2) = ensure_ids(&out);
        assert_eq!(ids, ids2, "ids stable across a second pass");
        assert_eq!(out, out2, "no double injection");
        // scan_ids sees the same ids without modifying.
        let scanned = scan_ids(&out);
        assert_eq!(scanned, ids);
    }

    #[test]
    fn xml_round_trips() {
        let h = DocHistory {
            doc_id: "doc1".into(),
            initial_file: "paper.md".into(),
            created: "2026-07-06T00:00:00Z".into(),
            equations: vec![EquationRecord {
                id: "eq1".into(),
                origin: "E = mc^2".into(),
                changes: vec![EquationChange {
                    ts: "2026-07-06T01:00:00Z".into(),
                    by: "user".into(),
                    from: "E = mc^2".into(),
                    to: "E = mc^{2}".into(),
                    reason: Some("brace & <clarity>".into()),
                }],
            }],
        };
        let xml = to_xml(&h);
        let back = from_xml(&xml);
        assert_eq!(h, back, "history survives an XML round-trip incl. escaped chars");
    }

    #[test]
    fn record_and_revert_in_tempdir() {
        let dir = std::env::temp_dir().join(format!("mdall-hist-test-{}", make_id("t1")));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = "doc-x";
        record_change_in(&dir, doc, "p.md", "eqA", "a", "b", "user", None).unwrap();
        record_change_in(&dir, doc, "p.md", "eqA", "b", "c", "mcp", Some("fix")).unwrap();
        let h = load_in(&dir, doc);
        let rec = h.equation("eqA").expect("record exists");
        assert_eq!(rec.origin, "a");
        assert_eq!(rec.change_count(), 2);
        assert_eq!(rec.current(), "c");
        // No-op change is not recorded.
        record_change_in(&dir, doc, "p.md", "eqA", "c", "c", "user", None).unwrap();
        assert_eq!(load_in(&dir, doc).equation("eqA").unwrap().change_count(), 2);
        // Revert to origin sets latex back to "a" and records the revert.
        let target = revert_in(&dir, doc, "eqA", "origin", "user").expect("revert ok");
        assert_eq!(target, "a");
        let h2 = load_in(&dir, doc);
        let rec2 = h2.equation("eqA").unwrap();
        assert_eq!(rec2.current(), "a");
        assert_eq!(rec2.change_count(), 3, "revert appended a change");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_equation_latex_by_id() {
        let md = "a\n\n$$\nx\n$$\n<!--mdall-eq:abc-->\n\nb\n";
        let out = set_equation_latex(md, "abc", "y = 2").expect("id found");
        assert!(out.contains("y = 2"), "new latex present: {:?}", out);
        assert!(!out.contains("\nx\n"), "old latex replaced: {:?}", out);
        assert!(out.contains("<!--mdall-eq:abc-->"), "marker preserved");
        assert!(set_equation_latex(md, "nope", "z").is_none());
    }

    #[test]
    fn latex_problem_detects_imbalance() {
        assert!(latex_problem("E = mc^2").is_none());
        assert!(latex_problem("\\frac{a}{b").is_some());
        assert!(latex_problem("\\left( x").is_some());
        assert!(latex_problem("\\left( x \\right)").is_none());
    }

    #[test]
    fn strip_then_inject_round_trips() {
        let md = "a\n\n$$\nE = mc^2\n$$\n\nb\n\n$$ x+y $$\n";
        let (marked, ids) = ensure_ids(md);
        let clean = strip_ids(&marked);
        assert_eq!(clean, md, "strip restores the clean source: {:?}", clean);
        assert!(!clean.contains("mdall-eq"), "no markers in the editor buffer");
        let re = inject_ids(&clean, &ids);
        assert_eq!(re, marked, "inject re-applies the same ids by order");
        assert_eq!(scan_ids(&re), ids, "ids survive the round-trip");
    }

    #[test]
    fn equation_index_at_locates_block() {
        let md = "aa\n\n$$ x $$\n\n$$ y $$\n";
        let first = md.find('x').unwrap();
        let second = md.find('y').unwrap();
        assert_eq!(equation_index_at(md, first), Some(0));
        assert_eq!(equation_index_at(md, second), Some(1));
        assert_eq!(equation_index_at(md, 0), None);
    }
}
