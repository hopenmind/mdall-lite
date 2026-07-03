//! purify.rs - zone-aware text purification.
//!
//! A pure-Rust port of the `llm_clean` pipeline, adapted for MD -> ALL. It
//! strips LLM watermarks and encoding artifacts, and (opt-in) neutralizes
//! conversational tics and applies French Imprimerie Nationale typography,
//! all while respecting "frozen" zones so it never corrupts structure.
//!
//! ## MD -> ALL adaptation (critical)
//! On top of the usual frozen zones (code fences, inline code, YAML front
//! matter) this port adds **MATH zones** - `$...$`, `$$...$$`, `\(...\)` and
//! `\[...\]`. A dash, minus sign or special space inside an equation is NEVER
//! rewritten: math is frozen against everything except invisible watermarks.
//!
//! ## Three escalating modes
//! - [`PurifyMode::Audit`] - report only, never mutates.
//! - [`PurifyMode::Sanitize`] - strip watermarks / normalize encoding (safe).
//! - [`PurifyMode::Decontaminate`] - also remove LLM tics + optional French
//!   typography, on prose zones only (opt-in, so it never eats real prose).
//!
//! The four phases mirror the reference: (1) index artifacts over immutable
//! input, (2) segment into zones, (3) reconstruct applying artifacts where the
//! zone allows, (4) decontaminate prose.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

// ═════════════════════════════════════════════════════════════════════════════
// § 0 - Character tables
// ═════════════════════════════════════════════════════════════════════════════

/// Watermark codepoint ranges - always stripped, in every zone.
fn watermark_kind(cp: u32) -> Option<&'static str> {
    match cp {
        0xE0000..=0xE007F => Some("watermark_tag"),
        0xFE00..=0xFE0F => Some("watermark_varsel"),
        0xE0100..=0xE01EF => Some("watermark_varsel"),
        0xFFF9..=0xFFFB => Some("watermark_interlinear"),
        _ => None,
    }
}

/// Zero-width chars (functional in parsers) - stripped unless safelisted.
fn zero_width_label(ch: char) -> Option<&'static str> {
    match ch {
        '\u{200B}' => Some("zwsp"),
        '\u{200C}' => Some("zwnj"),
        '\u{200D}' => Some("zwj"),
        '\u{FEFF}' => Some("bom"),
        '\u{2060}' => Some("wj"),
        '\u{2061}' => Some("fa"),
        '\u{2062}' => Some("it"),
        '\u{2063}' => Some("is"),
        '\u{2064}' => Some("ip"),
        _ => None,
    }
}

/// Directional override / isolate chars - always stripped.
fn dir_override_label(ch: char) -> Option<&'static str> {
    match ch {
        '\u{200E}' => Some("lrm"),
        '\u{200F}' => Some("rlm"),
        '\u{202A}' => Some("lre"),
        '\u{202B}' => Some("rle"),
        '\u{202C}' => Some("pdf"),
        '\u{202D}' => Some("lro"),
        '\u{202E}' => Some("rlo"),
        '\u{2066}' => Some("lri"),
        '\u{2067}' => Some("rli"),
        '\u{2068}' => Some("fsi"),
        '\u{2069}' => Some("pdi"),
        _ => None,
    }
}

/// Confusable (homoglyph) -> ASCII. Cyrillic / Greek / full-width Latin.
fn homoglyph(ch: char) -> Option<&'static str> {
    Some(match ch {
        // Cyrillic lookalikes
        'а' => "a", 'е' => "e", 'о' => "o", 'р' => "p", 'с' => "c", 'х' => "x",
        'А' => "A", 'В' => "B", 'Е' => "E", 'К' => "K", 'М' => "M", 'Н' => "H",
        'О' => "O", 'Р' => "P", 'С' => "C", 'Т' => "T", 'Х' => "X", 'У' => "Y",
        // Greek lookalikes
        'ο' => "o", 'Ο' => "O", 'ν' => "v", 'κ' => "k",
        // Full-width Latin
        'ａ' => "a", 'ｂ' => "b", 'ｃ' => "c", 'ｄ' => "d", 'ｅ' => "e", 'ｆ' => "f",
        'ｇ' => "g", 'ｈ' => "h", 'ｉ' => "i", 'ｊ' => "j", 'ｋ' => "k", 'ｌ' => "l",
        'ｍ' => "m", 'ｎ' => "n", 'ｏ' => "o", 'ｐ' => "p", 'ｑ' => "q", 'ｒ' => "r",
        'ｓ' => "s", 'ｔ' => "t", 'ｕ' => "u", 'ｖ' => "v", 'ｗ' => "w", 'ｘ' => "x",
        'ｙ' => "y", 'ｚ' => "z",
        _ => return None,
    })
}

/// Non-standard space -> normalized. Narrow no-break space folds to NBSP
/// (French norm); the rest fold to an ordinary space. Ordinary NBSP is kept.
fn special_space(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\u{2009}' => " ",        // thin space
        '\u{200A}' => " ",        // hair space
        '\u{202F}' => "\u{00A0}", // narrow no-break -> NBSP
        '\u{3000}' => " ",        // ideographic space
        '\u{205F}' => " ",        // medium mathematical space
        '\u{2008}' => " ",        // punctuation space
        '\u{2007}' => " ",        // figure space
        '\u{2006}' => " ",        // six-per-em space
        '\u{2005}' => " ",        // four-per-em space
        '\u{2004}' => " ",        // three-per-em space
        '\u{2003}' => " ",        // em space
        '\u{2002}' => " ",        // en space
        '\u{2001}' => " ",        // em quad
        '\u{2000}' => " ",        // en quad
        _ => return None,
    })
}

/// Smart quote -> label. Audit-only: the replacement is the char itself, so a
/// sanitize pass reports them without substituting (they are typographically
/// valid, just not AZERTY-typeable).
fn smart_quote_label(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\u{2018}' => "sq_left_single",
        '\u{2019}' => "sq_right_single",
        '\u{201A}' => "sq_low_single",
        '\u{201B}' => "sq_rev_single",
        '\u{201C}' => "sq_left_double",
        '\u{201D}' => "sq_right_double",
        '\u{201E}' => "sq_low_double",
        '\u{201F}' => "sq_rev_double",
        '\u{2032}' => "sq_prime",
        '\u{2033}' => "sq_double_prime",
        '\u{2035}' => "sq_rev_prime",
        _ => return None,
    })
}

/// Unicode dash variant -> label. All fold to ASCII '-'. These are never valid
/// syntax, so they are normalized in EVERY zone except math (see `should_apply`).
fn dash_variant_label(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\u{2013}' => "dash_en",
        '\u{2014}' => "dash_em",
        '\u{2015}' => "dash_horiz_bar",
        '\u{2212}' => "dash_minus_sign",
        '\u{2011}' => "dash_nb_hyphen",
        '\u{FE58}' => "dash_small_em",
        '\u{FE63}' => "dash_small_hyphen",
        '\u{FF0D}' => "dash_fw_hyphen",
        _ => return None,
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// § 1 - Data structures
// ═════════════════════════════════════════════════════════════════════════════

/// A single transform located over the immutable input. Byte-offset based.
#[derive(Clone)]
struct Artifact {
    offset: usize,
    length: usize,
    kind: String,
    replacement: String, // "" = remove
    is_watermark: bool,
    /// A unicode dash variant: safe to normalize in every zone except math.
    is_code_safe: bool,
}

/// A contiguous byte region with a semantic zone.
#[derive(Clone, Copy)]
struct Segment {
    start: usize,
    end: usize,
    zone: Zone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Prose,
    FrozenHard,
    FrozenSoft,
    Math,
}

/// The escalating purification level.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PurifyMode {
    Audit,
    Sanitize,
    Decontaminate,
}

impl PurifyMode {
    fn name(self) -> &'static str {
        match self {
            PurifyMode::Audit => "audit",
            PurifyMode::Sanitize => "sanitize",
            PurifyMode::Decontaminate => "decontaminate",
        }
    }
}

/// Segmentation strategy for the input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Markdown,
    Prose,
    /// Structured data (YAML/JSON/TOML/XML/CSS/SQL): the whole file is frozen.
    Frozen,
    /// Source code / HTML: whole file frozen-soft (watermarks + unicode dashes
    /// still cleaned everywhere; nothing structural is touched).
    Code,
}

impl DocFormat {
    fn name(self) -> &'static str {
        match self {
            DocFormat::Markdown => "markdown",
            DocFormat::Prose => "prose",
            DocFormat::Frozen => "frozen",
            DocFormat::Code => "code",
        }
    }
}

/// Options for a purification run.
pub struct PurifyOptions {
    pub mode: PurifyMode,
    /// Forced format, or `None` to auto-detect from the path / content.
    pub format: Option<DocFormat>,
    pub apply_fr_typography: bool,
    pub apply_tic_removal: bool,
    /// Zero-width chars to preserve (functional markers).
    pub preserve_safelist: Vec<char>,
}

impl Default for PurifyOptions {
    fn default() -> Self {
        PurifyOptions {
            mode: PurifyMode::Audit,
            format: None,
            apply_fr_typography: true,
            apply_tic_removal: true,
            preserve_safelist: Vec::new(),
        }
    }
}

/// A structured, serializable report of what was found / changed.
#[derive(Serialize)]
pub struct PurifyReport {
    pub format: String,
    pub size_bytes: usize,
    pub lines: usize,
    pub mode: String,
    pub artifacts_found: usize,
    pub watermarks: usize,
    pub artifact_breakdown: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tic_changes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fr_changes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chars_before: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chars_after: Option<usize>,
}

/// The result of a purification run: the (possibly unchanged) text + a report.
pub struct PurifyOutcome {
    pub text: String,
    pub report: PurifyReport,
}

// ═════════════════════════════════════════════════════════════════════════════
// § 2 - Phase 1: artifact indexation (immutable byte scan)
// ═════════════════════════════════════════════════════════════════════════════

fn index_artifacts(raw: &str, safelist: &HashSet<char>) -> Vec<Artifact> {
    let mut arts: Vec<Artifact> = Vec::new();
    let b = raw.as_bytes();
    let n = raw.len();
    let mut i = 0;

    let push = |arts: &mut Vec<Artifact>, offset, length, kind: &str, repl: &str, wm: bool, cs: bool| {
        arts.push(Artifact {
            offset,
            length,
            kind: kind.to_string(),
            replacement: repl.to_string(),
            is_watermark: wm,
            is_code_safe: cs,
        });
    };

    while i < n {
        let ch = raw[i..].chars().next().unwrap();
        let clen = ch.len_utf8();
        let cp = ch as u32;

        // Watermark (always, ignores safelist).
        if let Some(t) = watermark_kind(cp) {
            push(&mut arts, i, clen, t, "", true, false);
            i += clen;
            continue;
        }

        // CRLF -> LF, lone CR -> LF.
        if ch == '\r' {
            if i + 1 < n && b[i + 1] == b'\n' {
                push(&mut arts, i, 2, "crlf", "\n", false, false);
                i += 2;
            } else {
                push(&mut arts, i, 1, "cr", "\n", false, false);
                i += 1;
            }
            continue;
        }

        // Zero-width: strip unless safelisted; leading BOM is silently skipped.
        if let Some(z) = zero_width_label(ch) {
            if !safelist.contains(&ch) {
                if ch == '\u{FEFF}' && i == 0 {
                    i += clen;
                    continue;
                }
                push(&mut arts, i, clen, &format!("zero_width_{z}"), "", false, false);
            }
            i += clen;
            continue;
        }

        // Directional override.
        if let Some(d) = dir_override_label(ch) {
            push(&mut arts, i, clen, &format!("dir_override_{d}"), "", false, false);
            i += clen;
            continue;
        }

        // Control codes (not TAB, LF, CR).
        if cp <= 0x08 || cp == 0x0B || cp == 0x0C || (0x0E..=0x1F).contains(&cp) || cp == 0x7F {
            push(&mut arts, i, clen, "control_code", "", false, false);
            i += clen;
            continue;
        }

        // Line / paragraph separator -> LF.
        if cp == 0x2028 || cp == 0x2029 {
            push(&mut arts, i, clen, "line_sep", "\n", false, false);
            i += clen;
            continue;
        }

        // Special spaces.
        if let Some(norm) = special_space(ch) {
            push(&mut arts, i, clen, "special_space", norm, false, false);
            i += clen;
            continue;
        }

        // Homoglyphs.
        if let Some(a) = homoglyph(ch) {
            push(&mut arts, i, clen, "homoglyph", a, false, false);
            i += clen;
            continue;
        }

        // Smart quotes (audit-only: replacement is the char itself).
        if let Some(l) = smart_quote_label(ch) {
            let s = ch.to_string();
            push(&mut arts, i, clen, l, &s, false, false);
            i += clen;
            continue;
        }

        // Double hyphen "--" (exactly 2) -> single. 3+ (thematic break /
        // YAML separator) is left alone.
        if ch == '-' {
            let mut j = i;
            while j < n && b[j] == b'-' {
                j += 1;
            }
            let run = j - i;
            if run == 2 {
                push(&mut arts, i, 2, "double_hyphen", "-", false, false);
            }
            i = j;
            continue;
        }

        // Unicode dash variants -> ASCII '-' (code-safe: every zone but math).
        if let Some(l) = dash_variant_label(ch) {
            push(&mut arts, i, clen, l, "-", false, true);
            i += clen;
            continue;
        }

        // NOTE: NFD combining-mark normalization is intentionally not yet ported
        // (needs a Unicode general-category source); tracked for a follow-up.

        i += clen;
    }

    arts
}

// ═════════════════════════════════════════════════════════════════════════════
// § 3 - Phase 2: segmentation (format-aware, over the immutable input)
// ═════════════════════════════════════════════════════════════════════════════

/// Detect the segmentation strategy from an optional path, then content.
pub fn detect_format(path: Option<&str>, content: &str) -> DocFormat {
    if let Some(p) = path {
        if let Some(ext) = p.rsplit('.').next().map(|e| e.to_ascii_lowercase()) {
            match ext.as_str() {
                "md" | "mdx" | "markdown" => return DocFormat::Markdown,
                "txt" | "text" => return DocFormat::Prose,
                "yaml" | "yml" | "json" | "toml" | "xml" | "css" | "scss" | "sql" => {
                    return DocFormat::Frozen
                }
                "html" | "htm" | "py" | "pyw" | "rs" | "go" | "js" | "jsx" | "ts" | "tsx"
                | "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "rb" | "sh" | "bash" | "zsh" => {
                    return DocFormat::Code
                }
                _ => {}
            }
        }
    }
    let s = content.trim_start();
    if s.starts_with('{') || s.starts_with('[') {
        DocFormat::Frozen
    } else if content.starts_with("---") && front_matter_end(content).is_some() {
        DocFormat::Markdown
    } else {
        DocFormat::Prose
    }
}

fn segment(raw: &str, fmt: DocFormat) -> Vec<Segment> {
    let n = raw.len();
    match fmt {
        DocFormat::Prose => vec![Segment { start: 0, end: n, zone: Zone::Prose }],
        DocFormat::Frozen => vec![Segment { start: 0, end: n, zone: Zone::FrozenHard }],
        DocFormat::Code => vec![Segment { start: 0, end: n, zone: Zone::FrozenSoft }],
        DocFormat::Markdown => segment_markdown(raw),
    }
}

/// End byte offset of a leading YAML front matter block, if present.
fn front_matter_end(raw: &str) -> Option<usize> {
    if !raw.starts_with("---") {
        return None;
    }
    let after = &raw[3..];
    let rest = after.trim_start_matches([' ', '\t']);
    let mut idx = 3 + (after.len() - rest.len());
    if rest.starts_with('\n') {
        idx += 1;
    } else if rest.starts_with("\r\n") {
        idx += 2;
    } else {
        return None;
    }
    let n = raw.len();
    let mut p = idx;
    while p < n {
        let le = raw[p..].find('\n').map(|k| p + k + 1).unwrap_or(n);
        let line = raw[p..le].trim_end_matches(['\n', '\r']);
        if line.trim_end_matches([' ', '\t']) == "---" {
            return Some(le);
        }
        p = le;
    }
    None
}

/// Whether a whole (already trimmed) line opens a code fence; returns (char, len).
fn fence_open(line: &str) -> Option<(u8, usize)> {
    let b = line.as_bytes();
    if b.len() >= 3 && (b[0] == b'`' || b[0] == b'~') {
        let mut k = 0;
        while k < b.len() && b[k] == b[0] {
            k += 1;
        }
        if k >= 3 {
            return Some((b[0], k));
        }
    }
    None
}

/// Whether a (trimmed) line closes a fence of the given char and min length.
fn fence_close(line: &str, fence_char: u8, min_len: usize) -> bool {
    let b = line.as_bytes();
    if b.len() < min_len {
        return false;
    }
    b.iter().all(|&c| c == fence_char)
}

/// Fenced code-block byte regions (open line start .. close line end), col-0.
fn find_fences(raw: &str, from: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let n = raw.len();
    let mut pos = from;
    while pos < n {
        let le = raw[pos..].find('\n').map(|k| pos + k + 1).unwrap_or(n);
        let line = raw[pos..le].trim_end_matches(['\n', '\r']);
        if let Some((fc, flen)) = fence_open(line) {
            let open_start = pos;
            let mut p = le;
            let mut close_end = n;
            while p < n {
                let le2 = raw[p..].find('\n').map(|k| p + k + 1).unwrap_or(n);
                let l2 = raw[p..le2].trim_end_matches(['\n', '\r']).trim();
                if fence_close(l2, fc, flen) {
                    close_end = le2;
                    break;
                }
                p = le2;
            }
            out.push((open_start, close_end));
            pos = close_end;
        } else {
            pos = le;
        }
    }
    out
}

/// Close offset (just past the closer) of an inline code span, or `None`.
fn inline_code_close(raw: &str, after: usize, ticks: usize) -> Option<usize> {
    let n = raw.len();
    let mut j = after;
    let mut content = false;
    while j < n {
        let ch = raw[j..].chars().next().unwrap();
        if ch == '\n' {
            return None;
        }
        if ch == '`' {
            let mut k = j;
            while k < n && raw.as_bytes()[k] == b'`' {
                k += 1;
            }
            if k - j == ticks && content {
                return Some(k);
            }
            return None;
        }
        content = true;
        j += ch.len_utf8();
    }
    None
}

/// Close offset of an inline `$...$` math span (same line, non-space edges).
fn inline_math_close(raw: &str, after: usize) -> Option<usize> {
    let n = raw.len();
    if after >= n {
        return None;
    }
    let first = raw[after..].chars().next().unwrap();
    if first.is_whitespace() || first == '$' {
        return None;
    }
    let mut prev_non_ws = false;
    let mut j = after;
    while j < n {
        let ch = raw[j..].chars().next().unwrap();
        let clen = ch.len_utf8();
        if ch == '\n' {
            return None;
        }
        if ch == '$' {
            if j + clen < n && raw.as_bytes()[j + clen] == b'$' {
                return None;
            }
            if prev_non_ws {
                return Some(j + clen);
            }
            return None;
        }
        prev_non_ws = !ch.is_whitespace();
        j += clen;
    }
    None
}

/// Close offset just past a literal delimiter, searching from `from`.
fn literal_close(raw: &str, from: usize, delim: &str) -> Option<usize> {
    raw[from..].find(delim).map(|k| from + k + delim.len())
}

fn segment_markdown(raw: &str) -> Vec<Segment> {
    let n = raw.len();
    let b = raw.as_bytes();
    let mut regions: Vec<(usize, usize, Zone)> = Vec::new();

    let fm_end = front_matter_end(raw).unwrap_or(0);
    if fm_end > 0 {
        regions.push((0, fm_end, Zone::FrozenHard));
    }

    let fences = find_fences(raw, fm_end);
    for &(s, e) in &fences {
        regions.push((s, e, Zone::FrozenHard));
    }
    let in_fence = |off: usize| fences.iter().find(|&&(s, e)| s <= off && off < e).map(|&(_, e)| e);

    // Inline code + math over the non-fence, non-front-matter body.
    let mut i = fm_end;
    while i < n {
        if let Some(fe) = in_fence(i) {
            i = fe;
            continue;
        }
        let ch = raw[i..].chars().next().unwrap();
        let clen = ch.len_utf8();

        // Inline code `...` / ``...``.
        if ch == '`' {
            let mut k = i;
            while k < n && b[k] == b'`' {
                k += 1;
            }
            let ticks = k - i;
            if ticks <= 2 {
                if let Some(close) = inline_code_close(raw, k, ticks) {
                    regions.push((i, close, Zone::FrozenHard));
                    i = close;
                    continue;
                }
            }
            i = k;
            continue;
        }

        // Display math $$...$$.
        if ch == '$' && i + 1 < n && b[i + 1] == b'$' {
            if let Some(close) = literal_close(raw, i + 2, "$$") {
                regions.push((i, close, Zone::Math));
                i = close;
                continue;
            }
            i += 2;
            continue;
        }
        // Inline math $...$.
        if ch == '$' {
            if let Some(close) = inline_math_close(raw, i + 1) {
                regions.push((i, close, Zone::Math));
                i = close;
                continue;
            }
            i += 1;
            continue;
        }
        // \[ ... \] and \( ... \).
        if ch == '\\' && i + 1 < n {
            match b[i + 1] {
                b'[' => {
                    if let Some(close) = literal_close(raw, i + 2, "\\]") {
                        regions.push((i, close, Zone::Math));
                        i = close;
                        continue;
                    }
                }
                b'(' => {
                    if let Some(close) = literal_close(raw, i + 2, "\\)") {
                        regions.push((i, close, Zone::Math));
                        i = close;
                        continue;
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }

        i += clen;
    }

    regions.sort_by_key(|r| r.0);
    build_segments(raw, &regions)
}

fn build_segments(raw: &str, regions: &[(usize, usize, Zone)]) -> Vec<Segment> {
    let n = raw.len();
    let mut segs: Vec<Segment> = Vec::new();
    let mut pos = 0;
    for &(s, e, z) in regions {
        if s < pos || e < s {
            continue; // overlap / degenerate guard
        }
        if pos < s {
            segs.push(Segment { start: pos, end: s, zone: Zone::Prose });
        }
        segs.push(Segment { start: s, end: e, zone: z });
        pos = e;
    }
    if pos < n {
        segs.push(Segment { start: pos, end: n, zone: Zone::Prose });
    }
    if segs.is_empty() {
        segs.push(Segment { start: 0, end: n, zone: Zone::Prose });
    }
    segs
}

// ═════════════════════════════════════════════════════════════════════════════
// § 4 - Phase 3: reconstruction (artifact x zone)
// ═════════════════════════════════════════════════════════════════════════════

/// Whether an artifact may be applied inside a segment.
fn should_apply(a: &Artifact, zone: Zone) -> bool {
    if a.is_watermark {
        return true; // invisible watermarks: strip in every zone, incl. math
    }
    if zone == Zone::Math {
        return false; // math: freeze everything else (dashes, minus, spaces)
    }
    if a.is_code_safe {
        return true; // unicode dashes: normalize everywhere but math
    }
    !matches!(zone, Zone::FrozenHard | Zone::FrozenSoft)
}

fn zone_at(segs: &[Segment], pos: usize) -> Zone {
    let idx = match segs.binary_search_by(|s| s.start.cmp(&pos)) {
        Ok(i) => i,
        Err(0) => 0,
        Err(i) => i - 1,
    };
    segs[idx.min(segs.len() - 1)].zone
}

fn reconstruct(raw: &str, arts: &[Artifact], segs: &[Segment]) -> (String, usize, usize) {
    if arts.is_empty() {
        return (raw.to_string(), 0, 0);
    }
    let n = raw.len();
    let mut out = String::with_capacity(n);
    let mut pos = 0;
    let mut ai = 0;
    let mut applied = 0;
    let mut skipped = 0;

    while pos < n {
        if ai < arts.len() && arts[ai].offset == pos {
            let a = &arts[ai];
            if should_apply(a, zone_at(segs, pos)) {
                out.push_str(&a.replacement);
                applied += 1;
            } else {
                out.push_str(&raw[pos..pos + a.length]);
                skipped += 1;
            }
            pos += a.length;
            ai += 1;
        } else {
            let next = if ai < arts.len() { arts[ai].offset } else { n };
            out.push_str(&raw[pos..next]);
            pos = next;
        }
    }
    (out, applied, skipped)
}

// ═════════════════════════════════════════════════════════════════════════════
// § 5 - Phase 4: prose decontamination (LLM tics + French typography)
// ═════════════════════════════════════════════════════════════════════════════

fn llm_tics() -> &'static [(Regex, &'static str)] {
    static TICS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    TICS.get_or_init(|| {
        let pats: &[&str] = &[
            // ── English opening fillers ──
            r"(?im)^(?:Certainly|Absolutely|Of course|Sure|Great|Excellent|Wonderful|Fantastic|Awesome|Perfect)[!,.]?\s+",
            r"(?im)^It(?:'s| is) worth noting(?: that)?[,.]?\s*",
            r"(?im)^It(?:'s| is) important to note(?: that)?[,.]?\s*",
            r"(?im)^Additionally[,.]?\s+",
            r"(?im)^Furthermore[,.]?\s+",
            r"(?im)^Moreover[,.]?\s+",
            r"(?im)^In conclusion[,.]?\s+",
            r"(?im)^In summary[,.]?\s+",
            r"(?im)^To summarize[,.]?\s+",
            // ── English closing hedges ──
            r"(?im)\s+I hope this helps?[!.]?\s*$",
            r"(?im)\s+Let me know if you(?:'d like| have) (?:more |any )?(?:questions?|clarifications?)\.?\s*$",
            r"(?im)\s+Feel free to (?:ask|reach out)(?:[^.]*)?\.?\s*$",
            // ── English robotic self-reference ──
            r"(?i)\bAs an AI(?: language model)?,?\s+",
            r"(?i)\bAs a large language model,?\s+",
            // ── French opening fillers ──
            r"(?im)^(?:Bien s[uû]r|Absolument|Certainement|Effectivement|Tout [àa] fait|Parfait|Excellent|Avec plaisir)[!,.]?\s+",
            r"(?im)^Il est important de (?:noter|souligner|pr[eé]ciser)(?: que)?[,.]?\s*",
            r"(?im)^Il convient de (?:noter|souligner|pr[eé]ciser|mentionner)(?: que)?[,.]?\s*",
            r"(?im)^Il est (?:int[eé]ressant|fascinant|important|essentiel)(?: de noter| de souligner)?(?: que)?[,.]?\s*",
            r"(?im)^(?:De plus|En outre|Par ailleurs|[ÀA] noter que?)[,.]?\s+",
            r"(?im)^(?:En conclusion|En r[eé]sum[eé]|Pour r[eé]sumer|En somme|Pour conclure)[,.]?\s+",
            // ── French closing hedges ──
            r"(?im)\s+J'esp[eè]re(?: que cela| que cette r[eé]ponse)(?: vous)?(?: aide| r[eé]pond [àa] votre question)?\.?\s*$",
            r"(?im)\s+N'h[eé]sitez pas [àa] (?:me demander|poser vos questions)(?:[^.]*)?\.?\s*$",
            r"(?im)\s+Si vous avez d'autres questions[^.]*\.?\s*$",
            // ── French robotic self-reference ──
            r"(?i)\bEn tant qu(?:'IA|e (?:mod[eè]le d'IA|assistant IA)),?\s+",
            r"(?i)\bEn tant que grand mod[eè]le de langage,?\s+",
        ];
        pats.iter().map(|p| (Regex::new(p).unwrap(), "")).collect()
    })
}

fn remove_llm_tics(text: &str) -> (String, usize) {
    let mut cur = text.to_string();
    let mut changes = 0;
    for (re, repl) in llm_tics() {
        let n = re.find_iter(&cur).count();
        if n > 0 {
            cur = re.replace_all(&cur, *repl).into_owned();
            changes += n;
        }
    }
    (cur, changes)
}

/// French Imprimerie Nationale spacing. Only fixes EXISTING ordinary spaces
/// (never injects one where absent), so it cannot corrupt intentional layout.
fn apply_french_typography(text: &str) -> (String, usize) {
    static RES: OnceLock<(Regex, Regex, Regex)> = OnceLock::new();
    let (before_punct, guil_open, guil_close) = RES.get_or_init(|| {
        (
            Regex::new(r"[ \t]+([;!?])").unwrap(),
            Regex::new(r"«[ \t]+").unwrap(),
            Regex::new(r"[ \t]+»").unwrap(),
        )
    });

    let mut cur = text.to_string();
    let mut changes = 0;

    let n = before_punct.find_iter(&cur).count();
    if n > 0 {
        cur = before_punct.replace_all(&cur, "\u{00A0}${1}").into_owned();
        changes += n;
    }
    let n = guil_open.find_iter(&cur).count();
    if n > 0 {
        cur = guil_open.replace_all(&cur, "«\u{00A0}").into_owned();
        changes += n;
    }
    let n = guil_close.find_iter(&cur).count();
    if n > 0 {
        cur = guil_close.replace_all(&cur, "\u{00A0}»").into_owned();
        changes += n;
    }
    (cur, changes)
}

fn decontaminate(intermediate: &str, fmt: DocFormat, opts: &PurifyOptions) -> (String, usize, usize) {
    let segs = segment(intermediate, fmt);
    let mut out = String::with_capacity(intermediate.len());
    let mut tic_changes = 0;
    let mut fr_changes = 0;

    for s in &segs {
        let mut chunk = intermediate[s.start..s.end].to_string();
        if s.zone == Zone::Prose {
            if opts.apply_tic_removal {
                let (c, n) = remove_llm_tics(&chunk);
                chunk = c;
                tic_changes += n;
            }
            if opts.apply_fr_typography {
                let (c, n) = apply_french_typography(&chunk);
                chunk = c;
                fr_changes += n;
            }
        }
        out.push_str(&chunk);
    }
    (out, tic_changes, fr_changes)
}

// ═════════════════════════════════════════════════════════════════════════════
// § 6 - Public entry point
// ═════════════════════════════════════════════════════════════════════════════

/// Run the full pipeline over a string. No file IO: the caller owns reading and
/// writing. For `Audit`, `text` equals the input.
pub fn purify_str(raw: &str, path: Option<&str>, opts: &PurifyOptions) -> PurifyOutcome {
    let fmt = opts.format.unwrap_or_else(|| detect_format(path, raw));
    let safelist: HashSet<char> = opts.preserve_safelist.iter().copied().collect();

    let arts = index_artifacts(raw, &safelist);
    let mut breakdown: BTreeMap<String, usize> = BTreeMap::new();
    for a in &arts {
        *breakdown.entry(a.kind.clone()).or_insert(0) += 1;
    }
    let watermarks = arts.iter().filter(|a| a.is_watermark).count();

    let mut report = PurifyReport {
        format: fmt.name().to_string(),
        size_bytes: raw.len(),
        lines: raw.matches('\n').count() + 1,
        mode: opts.mode.name().to_string(),
        artifacts_found: arts.len(),
        watermarks,
        artifact_breakdown: breakdown,
        segments: None,
        applied: None,
        skipped: None,
        tic_changes: None,
        fr_changes: None,
        chars_before: None,
        chars_after: None,
    };

    if opts.mode == PurifyMode::Audit {
        return PurifyOutcome { text: raw.to_string(), report };
    }

    let segs = segment(raw, fmt);
    report.segments = Some(segs.len());
    let (intermediate, applied, skipped) = reconstruct(raw, &arts, &segs);
    report.applied = Some(applied);
    report.skipped = Some(skipped);

    if opts.mode == PurifyMode::Sanitize {
        report.chars_before = Some(raw.chars().count());
        report.chars_after = Some(intermediate.chars().count());
        return PurifyOutcome { text: intermediate, report };
    }

    let (final_text, tic, fr) = decontaminate(&intermediate, fmt, opts);
    report.tic_changes = Some(tic);
    report.fr_changes = Some(fr);
    report.chars_before = Some(raw.chars().count());
    report.chars_after = Some(final_text.chars().count());
    PurifyOutcome { text: final_text, report }
}

/// Serialize a report to pretty JSON (used by the MCP / CLI surfaces).
pub fn report_json(report: &PurifyReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string())
}

// ═════════════════════════════════════════════════════════════════════════════
// § 7 - Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn md(mode: PurifyMode) -> PurifyOptions {
        PurifyOptions { mode, format: Some(DocFormat::Markdown), ..Default::default() }
    }

    #[test]
    fn audit_never_mutates_and_counts() {
        let raw = "He said \u{2014} nothing.\u{200B}";
        let out = purify_str(raw, None, &md(PurifyMode::Audit));
        assert_eq!(out.text, raw, "audit must not change the text");
        assert!(out.report.artifacts_found >= 2);
        assert_eq!(out.report.applied, None);
    }

    #[test]
    fn watermark_stripped_even_inside_code_fence() {
        let raw = "text\u{E0041}\n\n```\ncode\u{E0042}\n```\n";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(!out.text.contains('\u{E0041}'));
        assert!(!out.text.contains('\u{E0042}'), "watermark must go even in code");
        assert_eq!(out.report.watermarks, 2);
    }

    #[test]
    fn em_dash_normalized_in_prose_kept_in_math() {
        let raw = "a \u{2014} b and $x \u{2014} y$ end";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.starts_with("a - b"), "prose dash -> ascii: {}", out.text);
        assert!(out.text.contains("$x \u{2014} y$"), "math dash frozen: {}", out.text);
    }

    #[test]
    fn minus_sign_frozen_in_display_math() {
        let raw = "$$ a \u{2212} b $$";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert_eq!(out.text, raw, "display math must be untouched");
    }

    #[test]
    fn bracket_math_frozen() {
        let raw = "see \\[ a \u{2212} b \\] here";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.contains("\\[ a \u{2212} b \\]"), "\\[..\\] frozen: {}", out.text);
    }

    #[test]
    fn homoglyph_prose_only() {
        // Cyrillic 'а' (U+0430) in prose and inside inline code.
        let raw = "w\u{0430}rd and `c\u{0430}de`";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.starts_with("ward"), "prose homoglyph fixed: {}", out.text);
        assert!(out.text.contains("`c\u{0430}de`"), "code homoglyph kept: {}", out.text);
    }

    #[test]
    fn crlf_and_double_hyphen_and_zero_width() {
        let raw = "a\r\nb -- c\u{200B}\n";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.contains("a\nb - c"), "crlf+dbl-hyphen+zwsp: {:?}", out.text);
        assert!(!out.text.contains('\r'));
        assert!(!out.text.contains('\u{200B}'));
    }

    #[test]
    fn triple_hyphen_thematic_break_kept() {
        let raw = "para\n\n---\n\nnext";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.contains("\n---\n"), "thematic break kept: {:?}", out.text);
    }

    #[test]
    fn decontaminate_removes_tics_and_applies_fr_typography() {
        let raw = "Certainly! The result is 4 .\nBien s\u{fb}r, la valeur suit \u{ab} x \u{bb} ;\n";
        let out = purify_str(raw, None, &md(PurifyMode::Decontaminate));
        assert!(!out.text.contains("Certainly"), "EN filler removed: {:?}", out.text);
        assert!(!out.text.contains("Bien s\u{fb}r"), "FR filler removed: {:?}", out.text);
        assert!(out.text.contains("\u{00A0};"), "NBSP before ; : {:?}", out.text);
        assert!(out.report.tic_changes.unwrap_or(0) >= 2);
    }

    #[test]
    fn sanitize_keeps_smart_quotes_but_audits_them() {
        let raw = "she said \u{201c}hi\u{201d}";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.contains('\u{201c}'), "smart quote kept in sanitize");
        assert!(out.report.artifact_breakdown.contains_key("sq_left_double"));
    }

    #[test]
    fn front_matter_frozen_for_non_dash_artifacts() {
        // A homoglyph in YAML front matter is frozen (kept); in prose it is fixed.
        let raw = "---\ntitle: w\u{0430}rd\n---\n\nw\u{0430}rd here\n";
        let out = purify_str(raw, None, &md(PurifyMode::Sanitize));
        assert!(out.text.contains("title: w\u{0430}rd"), "front matter homoglyph kept: {:?}", out.text);
        assert!(out.text.contains("ward here"), "prose homoglyph fixed: {:?}", out.text);

        // By design (unicode dashes are never valid syntax) a dash is normalized
        // in EVERY zone, including frozen front matter. Math is the ONE exception.
        let raw2 = "---\ntitle: a \u{2014} b\n---\n\nx\n";
        let out2 = purify_str(raw2, None, &md(PurifyMode::Sanitize));
        assert!(out2.text.contains("title: a - b"), "front matter dash normalized by design: {:?}", out2.text);
    }
}
