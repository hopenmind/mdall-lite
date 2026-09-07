//! LaTeX sanitization and custom-macro expansion.
//!
//! This module is the SHARED preprocessing layer that every equation render
//! path (Unicode export, egui LayoutJob fallback, Typst rasterizer) runs first.
//! It is pure logic with no UI dependency.
//!
//! Two responsibilities:
//!  1. `sanitize_latex`: strip non-visual commands (`\label`, `\tag`, `\notag`,
//!     `\nonumber`) and normalize spacing macros (`\,` `\;` `\quad` ...) to plain
//!     spaces, so downstream renderers never choke on or echo these tokens.
//!  2. Custom macro expansion: scan a document for `\newcommand`, `\renewcommand`
//!     and `\def` definitions (`MacroTable::collect`), then expand uses of those
//!     macros inside an equation (`expand_macros`). Undefined macros are left for
//!     the downstream renderer to degrade gracefully (it strips the backslash and
//!     renders the name upright), so a raw `\name` is never shown to the user.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// The macro table for the document currently being rendered on this thread.
    /// The UI sets this once per frame (`set_active_macros`) so free functions
    /// like `render::latex_to_unicode` - used for inline math inside the
    /// content-keyed block cache - can expand custom macros without threading a
    /// table through every call site. Empty by default (a no-op).
    static ACTIVE_MACROS: RefCell<MacroTable> = RefCell::new(MacroTable::new());
}

/// Install the macro table used by `expand_active` for the current thread.
/// Call once per render frame after collecting macros from the document source.
pub fn set_active_macros(table: MacroTable) {
    ACTIVE_MACROS.with(|cell| *cell.borrow_mut() = table);
}

/// Collect the macros defined in `source` and install them as the thread-active
/// table. Use this at an export entry point (which may run on a different
/// document than the editor, e.g. batch conversion) so custom macros expand in
/// headless / batch renders, not only in the live editor.
pub fn install_from_source(source: &str) {
    set_active_macros(MacroTable::collect(source));
}

/// Expand custom macros in `input` using the thread-active macro table.
/// Returns the input unchanged when no macros are installed.
pub fn expand_active(input: &str) -> String {
    ACTIVE_MACROS.with(|cell| {
        let table = cell.borrow();
        if table.is_empty() {
            input.to_string()
        } else {
            expand_macros(input, &table)
        }
    })
}

/// A captured custom-macro definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroDef {
    /// Number of arguments the macro takes (0, 1 or 2 supported for expansion).
    pub argc: usize,
    /// Replacement body, with `#1` / `#2` placeholders for arguments.
    pub body: String,
}

/// A table of custom macros collected from a document source.
#[derive(Clone, Debug, Default)]
pub struct MacroTable {
    map: HashMap<String, MacroDef>,
}

impl MacroTable {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Look up a macro by its name (without the leading backslash).
    pub fn get(&self, name: &str) -> Option<&MacroDef> {
        self.map.get(name)
    }

    /// Insert or replace a macro definition.
    pub fn insert(&mut self, name: impl Into<String>, def: MacroDef) {
        self.map.insert(name.into(), def);
    }

    /// Scan an entire document source for macro definitions.
    ///
    /// Recognizes (anywhere in the text, including inside code/preamble):
    ///  - `\newcommand{\name}[argc][optional default]{body}`
    ///  - `\newcommand\name{body}`
    ///  - `\renewcommand{\name}[argc]{body}` (same forms)
    ///  - `\providecommand` (same forms)
    ///  - `\def\name{body}` (no arg count; `#1`/`#2` in the body still expand)
    ///
    /// Later definitions override earlier ones (matching LaTeX `\renewcommand`).
    pub fn collect(source: &str) -> Self {
        let mut table = MacroTable::new();
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            if bytes[i] != b'\\' {
                i += 1;
                continue;
            }
            // Read the control-word name following the backslash.
            let kw_start = i + 1;
            let mut j = kw_start;
            while j < len && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            let kw = &source[kw_start..j];

            match kw {
                "newcommand" | "renewcommand" | "providecommand" => {
                    if let Some((name, def, next)) = parse_newcommand(source, j) {
                        table.map.insert(name, def);
                        i = next;
                        continue;
                    }
                    i = j;
                }
                "def" => {
                    if let Some((name, def, next)) = parse_def(source, j) {
                        table.map.insert(name, def);
                        i = next;
                        continue;
                    }
                    i = j;
                }
                _ => {
                    i = if j > i { j } else { i + 1 };
                }
            }
        }

        table
    }
}

/// Parse a `\newcommand` form starting right after the keyword.
/// `pos` points just past `newcommand` / `renewcommand`.
/// Returns `(name, def, byte_index_after_definition)`.
fn parse_newcommand(s: &str, pos: usize) -> Option<(String, MacroDef, usize)> {
    let mut i = skip_ws(s, pos);
    // The macro name: either `{\name}` or `\name`.
    let name;
    if byte_at(s, i) == Some(b'{') {
        let end = brace_end(s, i + 1)?;
        let inner = s[i + 1..end].trim();
        name = control_word(inner)?;
        i = end + 1;
    } else if byte_at(s, i) == Some(b'\\') {
        let (n, next) = control_word_at(s, i)?;
        name = n;
        i = next;
    } else {
        return None;
    }

    i = skip_ws(s, i);

    // Optional `[argc]`.
    let mut argc = 0usize;
    if byte_at(s, i) == Some(b'[') {
        let end = s[i..].find(']').map(|o| i + o)?;
        argc = s[i + 1..end].trim().parse().unwrap_or(0);
        i = end + 1;
        i = skip_ws(s, i);
        // Optional default value for first arg: another `[...]` - skip it.
        if byte_at(s, i) == Some(b'[') {
            let end = s[i..].find(']').map(|o| i + o)?;
            i = end + 1;
            i = skip_ws(s, i);
        }
    }

    // Body: `{...}`.
    if byte_at(s, i) != Some(b'{') {
        return None;
    }
    let end = brace_end(s, i + 1)?;
    let body = s[i + 1..end].to_string();
    Some((name, MacroDef { argc, body }, end + 1))
}

/// Parse a `\def\name{body}` form starting right after `def`.
fn parse_def(s: &str, pos: usize) -> Option<(String, MacroDef, usize)> {
    let mut i = skip_ws(s, pos);
    if byte_at(s, i) != Some(b'\\') {
        return None;
    }
    let (name, next) = control_word_at(s, i)?;
    i = next;
    // `\def` may declare a parameter text before the body; we only support the
    // common `\def\name{...}` shape. If anything other than `{` follows, bail.
    i = skip_ws(s, i);
    if byte_at(s, i) != Some(b'{') {
        return None;
    }
    let end = brace_end(s, i + 1)?;
    let body = s[i + 1..end].to_string();
    // Infer argc from the highest `#n` used in the body (def has no [argc]).
    let argc = max_arg_placeholder(&body);
    Some((name, MacroDef { argc, body }, end + 1))
}

/// Strip non-visual commands and normalize spacing macros.
/// Leaves all standard math intact.
pub fn sanitize_latex(input: &str) -> String {
    let mut s = input.to_string();

    // Strip commands that carry no visual math: `\label{...}`, `\tag{...}`,
    // `\eqref{...}`, `\ref{...}`. These take one braced argument.
    for cmd in &["\\label", "\\tag", "\\eqref", "\\ref", "\\cite", "\\index"] {
        s = strip_command_with_arg(&s, cmd);
    }

    // Strip flag commands that take no argument.
    for cmd in &["\\nonumber", "\\notag", "\\displaystyle", "\\textstyle",
                 "\\scriptstyle", "\\limits", "\\nolimits"] {
        s = strip_command_word(&s, cmd);
    }

    // Spacing macros → a single space (or nothing for negative thin space).
    // Order matters: longer tokens first so `\quad` is not eaten by `\q`.
    let space_repls: &[(&str, &str)] = &[
        ("\\qquad", "  "),
        ("\\quad", " "),
        ("\\thinspace", " "),
        ("\\negthinspace", ""),
        ("\\,", " "),
        ("\\;", " "),
        ("\\:", " "),
        ("\\ ", " "),
        ("\\!", ""),
    ];
    for (from, to) in space_repls {
        s = s.replace(from, to);
    }

    // Strip pure display-mode environment wrappers that carry no math structure
    // (the renderer is already in display mode). Structural environments
    // (align, aligned, gather, cases, split, ...) are intentionally kept so the
    // renderer can attempt their layout.
    for env in &["equation*", "equation", "displaymath", "math"] {
        s = s.replace(&format!("\\begin{{{env}}}"), "");
        s = s.replace(&format!("\\end{{{env}}}"), "");
    }

    s.trim().to_string()
}

/// Expand custom-macro uses in a LaTeX string using a collected table.
/// Performs iterative expansion (bounded) so macros defined in terms of other
/// macros resolve. Undefined `\name` tokens are left untouched for the
/// downstream renderer to degrade gracefully.
pub fn expand_macros(input: &str, table: &MacroTable) -> String {
    if table.is_empty() {
        return input.to_string();
    }
    let mut current = input.to_string();
    // Bounded passes prevent infinite loops on recursive macros.
    for _ in 0..8 {
        let (next, changed) = expand_once(&current, table);
        current = next;
        if !changed {
            break;
        }
    }
    current
}

/// Convenience: sanitize then expand macros (the full shared preprocessing).
pub fn expand_and_sanitize(input: &str, table: &MacroTable) -> String {
    let expanded = expand_macros(input, table);
    sanitize_latex(&expanded)
}

fn expand_once(s: &str, table: &MacroTable) -> (String, bool) {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut changed = false;

    while i < len {
        if bytes[i] != b'\\' {
            // Copy one full UTF-8 char.
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // Read control word.
        let name_start = i + 1;
        let mut j = name_start;
        while j < len && bytes[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == name_start {
            // Control symbol (e.g. `\\`, `\,`) - not a named macro; copy `\` + next char.
            out.push('\\');
            i += 1;
            continue;
        }
        let name = &s[name_start..j];
        let def = match table.get(name) {
            Some(d) => d,
            None => {
                // Not a custom macro - copy verbatim and continue.
                out.push_str(&s[i..j]);
                i = j;
                continue;
            }
        };

        // Collect `def.argc` braced (or single-token) arguments after the name.
        let mut k = j;
        let mut args: Vec<String> = Vec::with_capacity(def.argc);
        let mut ok = true;
        for _ in 0..def.argc {
            let ws = skip_ws(s, k);
            if byte_at(s, ws) == Some(b'{') {
                match brace_end(s, ws + 1) {
                    Some(end) => {
                        args.push(s[ws + 1..end].to_string());
                        k = end + 1;
                    }
                    None => { ok = false; break; }
                }
            } else if ws < len {
                // Single-token argument (e.g. `\sket x`): take one char or control word.
                if bytes[ws] == b'\\' {
                    let (cw, next) = match control_word_at(s, ws) {
                        Some(v) => v,
                        None => { ok = false; break; }
                    };
                    args.push(format!("\\{}", cw));
                    k = next;
                } else {
                    let ch = s[ws..].chars().next().unwrap();
                    args.push(ch.to_string());
                    k = ws + ch.len_utf8();
                }
            } else {
                ok = false;
                break;
            }
        }

        if !ok {
            // Could not gather arguments - leave the macro untouched.
            out.push_str(&s[i..j]);
            i = j;
            continue;
        }

        out.push_str(&substitute_body(&def.body, &args));
        changed = true;
        i = k;
    }

    (out, changed)
}

/// Replace `#1`, `#2`, ... in a macro body with the provided arguments.
fn substitute_body(body: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'#' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
            let idx = (bytes[i + 1] - b'0') as usize;
            if idx >= 1 && idx <= args.len() {
                out.push_str(&args[idx - 1]);
            }
            i += 2;
            continue;
        }
        let ch = body[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

// ── small parsing helpers ──────────────────────────────────────────────────

fn byte_at(s: &str, i: usize) -> Option<u8> {
    s.as_bytes().get(i).copied()
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' || bytes[i] == b'\n') {
        i += 1;
    }
    i
}

/// Find the index of the `}` matching the `{` whose content begins at `start`.
fn brace_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract a control-word name from a `\name` token at the START of `s`.
fn control_word(s: &str) -> Option<String> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'\\') {
        return None;
    }
    let mut j = 1;
    while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j == 1 {
        return None;
    }
    Some(s[1..j].to_string())
}

/// Read a `\name` control word at byte index `i` (which must be `\`).
/// Returns `(name, index_after_name)`.
fn control_word_at(s: &str, i: usize) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    if bytes.get(i) != Some(&b'\\') {
        return None;
    }
    let mut j = i + 1;
    while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j == i + 1 {
        return None;
    }
    Some((s[i + 1..j].to_string(), j))
}

/// Highest `#n` placeholder referenced in a body (used to infer `\def` argc).
fn max_arg_placeholder(body: &str) -> usize {
    let bytes = body.as_bytes();
    let mut max = 0usize;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'#' && bytes[i + 1].is_ascii_digit() {
            let n = (bytes[i + 1] - b'0') as usize;
            if n > max {
                max = n;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    max
}

/// Remove a command together with its single braced argument: `\cmd{...}` → "".
fn strip_command_with_arg(s: &str, cmd: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(cmd) {
        // Ensure the match is the full control word, not a prefix
        // (e.g. `\ref` must not match inside `\reflect`).
        let after_cmd = pos + cmd.len();
        let boundary_ok = rest.as_bytes().get(after_cmd)
            .map_or(true, |b| !b.is_ascii_alphabetic());
        if !boundary_ok {
            // Copy up to and including this char, keep scanning.
            let take = after_cmd;
            out.push_str(&rest[..take]);
            rest = &rest[take..];
            continue;
        }
        out.push_str(&rest[..pos]);
        // Skip optional whitespace, then a braced argument if present.
        let mut k = skip_ws(rest, after_cmd);
        if byte_at(rest, k) == Some(b'{') {
            if let Some(end) = brace_end(rest, k + 1) {
                k = end + 1;
            }
        }
        rest = &rest[k..];
    }
    out.push_str(rest);
    out
}

/// Remove a no-argument command word: `\nonumber` → "".
fn strip_command_word(s: &str, cmd: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(cmd) {
        let after_cmd = pos + cmd.len();
        let boundary_ok = rest.as_bytes().get(after_cmd)
            .map_or(true, |b| !b.is_ascii_alphabetic());
        if !boundary_ok {
            out.push_str(&rest[..after_cmd]);
            rest = &rest[after_cmd..];
            continue;
        }
        out.push_str(&rest[..pos]);
        rest = &rest[after_cmd..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_label_and_tag() {
        let s = sanitize_latex("\\label{eq:schrodinger} i\\hbar x");
        assert!(!s.contains("\\label"), "label not stripped: {}", s);
        assert!(!s.contains("schrodinger"), "label arg leaked: {}", s);
        assert!(s.contains("\\hbar"), "standard math removed: {}", s);
    }

    #[test]
    fn sanitize_strips_display_env_wrappers() {
        let s = sanitize_latex("\\begin{equation}E = mc^2\\end{equation}");
        assert_eq!(s, "E = mc^2", "display wrapper not stripped: {s:?}");
        let s2 = sanitize_latex("\\begin{equation*}x\\end{equation*}");
        assert_eq!(s2, "x");
        // Structural environments are KEPT (renderer attempts their layout).
        let s3 = sanitize_latex("\\begin{align}a &= b\\end{align}");
        assert!(s3.contains("\\begin{align}"), "align must be kept: {s3:?}");
    }

    #[test]
    fn sanitize_keeps_ref_prefix_words() {
        // `\ref` must not eat the start of `\reflect`.
        let s = sanitize_latex("\\reflect{x}");
        assert!(s.contains("\\reflect{x}"), "prefix command corrupted: {}", s);
    }

    #[test]
    fn sanitize_spacing_macros() {
        let s = sanitize_latex("a\\,b\\;c\\quad d\\!e");
        assert!(!s.contains("\\,"));
        assert!(!s.contains("\\quad"));
        // \, \; -> space, \quad -> space (+ literal space after), \! -> nothing.
        assert_eq!(s, "a b c  de");
    }

    #[test]
    fn sanitize_strips_flags() {
        let s = sanitize_latex("x = y \\nonumber");
        assert!(!s.contains("\\nonumber"), "{}", s);
    }

    #[test]
    fn collect_newcommand_one_arg() {
        let t = MacroTable::collect("\\newcommand{\\sket}[1]{|#1\\rangle}");
        let d = t.get("sket").expect("sket captured");
        assert_eq!(d.argc, 1);
        assert_eq!(d.body, "|#1\\rangle");
    }

    #[test]
    fn collect_newcommand_no_braces() {
        let t = MacroTable::collect("\\newcommand\\ii{\\mathrm{i}}");
        let d = t.get("ii").expect("ii captured");
        assert_eq!(d.argc, 0);
        assert_eq!(d.body, "\\mathrm{i}");
    }

    #[test]
    fn collect_def_form() {
        let t = MacroTable::collect("\\def\\R{\\mathbb{R}}");
        let d = t.get("R").expect("R captured");
        assert_eq!(d.argc, 0);
        assert_eq!(d.body, "\\mathbb{R}");
    }

    #[test]
    fn collect_renewcommand_overrides() {
        let t = MacroTable::collect(
            "\\newcommand{\\x}{a}\nlater \\renewcommand{\\x}{b}",
        );
        assert_eq!(t.get("x").unwrap().body, "b");
    }

    #[test]
    fn expand_sket_ket() {
        let t = MacroTable::collect("\\newcommand{\\sket}[1]{|#1\\rangle}");
        let out = expand_macros("i\\hbar \\sket{\\psi(t)} = H\\sket{\\psi(t)}", &t);
        assert_eq!(out, "i\\hbar |\\psi(t)\\rangle = H|\\psi(t)\\rangle");
    }

    #[test]
    fn expand_two_args() {
        let t = MacroTable::collect("\\newcommand{\\br}[2]{\\langle #1 | #2 \\rangle}");
        let out = expand_macros("\\br{a}{b}", &t);
        assert_eq!(out, "\\langle a | b \\rangle");
    }

    #[test]
    fn expand_zero_arg() {
        let t = MacroTable::collect("\\newcommand{\\R}{\\mathbb{R}}");
        let out = expand_macros("x \\in \\R", &t);
        assert_eq!(out, "x \\in \\mathbb{R}");
    }

    #[test]
    fn expand_nested_macros() {
        let t = MacroTable::collect(
            "\\newcommand{\\half}{\\frac{1}{2}}\n\\newcommand{\\hbar}{h}",
        );
        // `\hbar` is also a standard symbol, but a user redefinition wins here.
        let out = expand_macros("\\half", &t);
        assert_eq!(out, "\\frac{1}{2}");
    }

    #[test]
    fn expand_undefined_left_intact() {
        let t = MacroTable::collect("\\newcommand{\\foo}{bar}");
        let out = expand_macros("\\baz + \\foo", &t);
        assert_eq!(out, "\\baz + bar");
    }

    #[test]
    fn expand_single_token_arg() {
        let t = MacroTable::collect("\\newcommand{\\vec}[1]{\\mathbf{#1}}");
        let out = expand_macros("\\vec x", &t);
        assert_eq!(out, "\\mathbf{x}");
    }

    #[test]
    fn empty_table_is_noop() {
        let t = MacroTable::new();
        assert_eq!(expand_macros("\\sket{\\psi}", &t), "\\sket{\\psi}");
    }

    #[test]
    fn tex_preamble_macros_survive_import() {
        let tex = "\\documentclass{article}\n\
                   \\newcommand{\\sket}[1]{|#1\\rangle}\n\
                   \\begin{document}\n\
                   $$ \\sket{\\psi} $$\n\
                   \\end{document}\n";
        let md = crate::import::tex_to_md(tex).expect("tex import");
        // The macro definition survives into the Markdown (as a hidden comment).
        let table = MacroTable::collect(&md);
        let d = table.get("sket").expect("sket survived import");
        assert_eq!(d.argc, 1);
        let out = expand_macros("\\sket{\\psi}", &table);
        assert_eq!(out, "|\\psi\\rangle");
    }

    #[test]
    fn full_schrodinger_pipeline() {
        let src = "\\newcommand{\\sket}[1]{|#1\\rangle}";
        let t = MacroTable::collect(src);
        let eq = "\\label{eq:schrodinger} i\\hbar \\frac{d}{dt}\\sket{\\psi(t)} = H\\sket{\\psi(t)}";
        let out = expand_and_sanitize(eq, &t);
        assert!(!out.contains("\\label"), "label leaked: {}", out);
        assert!(!out.contains("\\sket"), "sket not expanded: {}", out);
        assert!(out.contains("|\\psi(t)\\rangle"), "ket not formed: {}", out);
        assert!(out.contains("\\frac{d}{dt}"), "frac lost: {}", out);
    }
}
