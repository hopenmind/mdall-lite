//! UI-side equation rendering: LaTeX -> egui LayoutJob.
//!
//! This lives in the binary (UI), NOT in the `mdall_core` core library, because
//! it returns an egui type. It reuses the pure LaTeX-normalization helpers from
//! the core (`mdall_core::render`) so the core stays 100% egui-free.

use mdall_core::latex_macros::{expand_active, sanitize_latex};
use mdall_core::render::{find_brace_end, normalize_latex_escapes, GREEKS, SYMBOLS};

/// Renders a LaTeX expression as an egui LayoutJob with proper subscript/superscript
/// positioning using font-size scaling + valign - no Unicode char substitution needed.
/// Words like "left", "right" in _{...} render cleanly regardless of Unicode coverage.
pub fn latex_to_layout_job(
    latex: &str,
    base_size: f32,
    wrap_width: f32,
    color: eframe::egui::Color32,
) -> eframe::egui::text::LayoutJob {
    use eframe::egui::{Align, FontFamily, FontId, text::LayoutJob, TextFormat};

    // ── Pre-process: same pipeline as latex_to_unicode, stopping before
    //    unicode subscript/superscript conversion ──────────────────────

    // Defensive: expand custom macros + drop \label/\tag/spacing macros even if
    // the caller passed a raw string. Idempotent when already prepared.
    let prepared = sanitize_latex(&expand_active(latex));
    let mut s = normalize_latex_escapes(&prepared);
    s = s.replace("\r\n", " ").replace('\n', " ");
    s = s.replace("\\{", "{").replace("\\}", "}").replace("\\_", "_");
    // Line break in display math → space (we render a single visual line).
    s = s.replace("\\\\", " ");

    for env in &["align", "aligned", "equation", "gather", "gathered", "split",
                 "cases", "matrix", "pmatrix", "bmatrix", "vmatrix", "array"] {
        s = s.replace(&format!("\\begin{{{}}}", env), "");
        s = s.replace(&format!("\\end{{{}}}", env), "");
    }
    s = s.replace("&=", " = ").replace("&", " ");

    // Passthrough: \text{...} → inner, \mathrm{...} → inner, etc.
    let passthrough_cmds = [
        "\\text{", "\\textbf{", "\\textit{", "\\textrm{", "\\texttt{",
        "\\mathrm{", "\\mathit{", "\\mathbf{", "\\mathbb{", "\\mathcal{",
        "\\mathsf{", "\\mathfrak{", "\\operatorname{", "\\boldsymbol{",
        "\\hat{", "\\tilde{", "\\bar{", "\\vec{", "\\dot{", "\\ddot{",
        "\\overline{", "\\underline{", "\\widehat{", "\\widetilde{",
        "\\bm{", "\\rm{", "\\color{",
    ];
    for cmd in &passthrough_cmds {
        loop {
            if let Some(pos) = s.find(cmd) {
                let cs = pos + cmd.len();
                if let Some(end) = find_brace_end(&s, cs) {
                    let inner = s[cs..end].to_string();
                    s = format!("{}{}{}", &s[..pos], inner, &s[end + 1..]);
                    continue;
                }
            }
            break;
        }
    }

    // \frac{a}{b} → (a)/(b)
    loop {
        if let Some(pos) = s.find("\\frac{") {
            let ns = pos + 6;
            if let Some(ne) = find_brace_end(&s, ns) {
                let num = s[ns..ne].to_string();
                let rest = &s[ne + 1..];
                if rest.starts_with('{') {
                    if let Some(de) = find_brace_end(rest, 1) {
                        let den = rest[1..de].to_string();
                        let ns = if num.len() > 1 { format!("({})", num) } else { num };
                        let ds = if den.len() > 1 { format!("({})", den) } else { den };
                        s = format!("{}{}/{}{}", &s[..pos], ns, ds, &rest[de + 1..]);
                        continue;
                    }
                }
            }
        }
        break;
    }

    // \sqrt{x} → √(x)
    loop {
        if let Some(pos) = s.find("\\sqrt{") {
            let cs = pos + 6;
            if let Some(end) = find_brace_end(&s, cs) {
                let inner = s[cs..end].to_string();
                s = format!("{}√({}){}", &s[..pos], inner, &s[end + 1..]);
                continue;
            }
        }
        break;
    }

    // Greeks and math symbols
    for (cmd, uni) in GREEKS { s = s.replace(cmd, uni); }
    for (cmd, uni) in SYMBOLS { s = s.replace(cmd, uni); }

    // Remove remaining \commands (keep name, drop backslash)
    {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let mut cmd = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphabetic() { cmd.push(chars.next().unwrap()); } else { break; }
                }
                if !cmd.is_empty() { out.push_str(&cmd); }
            } else {
                out.push(ch);
            }
        }
        s = out;
    }
    while s.contains("  ") { s = s.replace("  ", " "); }

    // ── Build LayoutJob ────────────────────────────────────────────────
    // Subscripts/superscripts use valign + 65% font size - works for ANY
    // word content, no Unicode mapping needed for "left", "right", etc.

    let mk = |size: f32, italic: bool, valign: Align| TextFormat {
        font_id: FontId::new(size, FontFamily::Proportional),
        color,
        italics: italic,
        valign,
        ..Default::default()
    };
    let math_fmt = mk(base_size, true,  Align::Center);
    let op_fmt   = mk(base_size, false, Align::Center);
    let sub_fmt  = mk(base_size * 0.65, false, Align::Max);  // bottom of line
    let sup_fmt  = mk(base_size * 0.65, false, Align::Min);  // top of line

    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let mut chars = s.chars().peekable();
    let mut buf   = String::new();
    let mut italic_buf = true; // current buf mode

    macro_rules! flush {
        ($fmt:expr) => {
            if !buf.is_empty() {
                job.append(&buf, 0.0, $fmt.clone());
                buf.clear();
            }
        };
    }

    while let Some(ch) = chars.next() {
        match ch {
            '_' | '^' => {
                flush!(if italic_buf { &math_fmt } else { &op_fmt });
                let is_sub = ch == '_';
                let fmt = if is_sub { &sub_fmt } else { &sup_fmt };
                let mut scripted = false;

                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let mut depth = 1usize;
                    let mut inner = String::new();
                    for c in chars.by_ref() {
                        match c {
                            '{' => { depth += 1; inner.push(c); }
                            '}' => { depth -= 1; if depth == 0 { break; } inner.push(c); }
                            _   => { inner.push(c); }
                        }
                    }
                    if !inner.is_empty() {
                        job.append(&inner, 0.0, fmt.clone());
                    }
                    scripted = true;
                } else if let Some(&nc) = chars.peek() {
                    // Single-char script: digits, lowercase letters, common math punctuation.
                    // Uppercase alone (e.g. _State) → treat as literal separator
                    // to avoid splitting compound words like Center_State.
                    let should_script = nc.is_ascii_digit()
                        || nc.is_lowercase()
                        || matches!(nc, '+' | '-' | '=' | '*' | '\'' | ',' | '.');
                    if should_script {
                        chars.next();
                        job.append(&nc.to_string(), 0.0, fmt.clone());
                        scripted = true;
                    }
                }

                if scripted {
                    italic_buf = true;
                } else {
                    // Literal '_' or '^' - push as operator (upright)
                    let sep = if is_sub { '_' } else { '^' };
                    buf.push(sep);
                    italic_buf = false;
                }
            }
            '{' | '}' => {
                // Bare brace leftover - skip
                flush!(if italic_buf { &math_fmt } else { &op_fmt });
                italic_buf = true;
            }
            _ if ch.is_alphabetic() => {
                if !italic_buf {
                    flush!(&op_fmt);
                    italic_buf = true;
                }
                buf.push(ch);
            }
            _ => {
                if italic_buf {
                    flush!(&math_fmt);
                    italic_buf = false;
                }
                buf.push(ch);
            }
        }
    }
    flush!(if italic_buf { &math_fmt } else { &op_fmt });

    job
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    fn job_text(latex: &str) -> String {
        let job = latex_to_layout_job(latex, 16.0, 400.0, Color32::BLACK);
        job.text
    }

    #[test]
    fn no_backslash_leaks_for_known_commands() {
        let t = job_text("i\\hbar \\frac{d}{dt} = H");
        assert!(!t.contains('\\'), "raw backslash leaked: {:?}", t);
        assert!(t.contains('ℏ'), "hbar not converted: {:?}", t);
        assert!(t.contains('/'), "frac not flattened: {:?}", t);
    }

    #[test]
    fn label_is_stripped_in_layout() {
        let t = job_text("\\label{eq:x} a = b");
        assert!(!t.contains("label"), "label leaked: {:?}", t);
        assert!(!t.contains("eq:x"), "label arg leaked: {:?}", t);
        assert!(t.contains("a"), "{:?}", t);
    }

    #[test]
    fn unknown_command_drops_backslash() {
        let t = job_text("\\foobar x");
        assert!(!t.contains('\\'), "backslash shown for unknown cmd: {:?}", t);
        assert!(t.contains("foobar"), "unknown name dropped entirely: {:?}", t);
    }

    #[test]
    fn frac_numerator_denominator() {
        let t = job_text("\\frac{a}{b+c}");
        // Single-char numerator stays bare, multi-char denominator gets parens.
        assert!(t.contains("a/(b+c)"), "frac structure wrong: {:?}", t);
    }

    #[test]
    fn sqrt_renders_radical() {
        let t = job_text("\\sqrt{x}");
        assert!(t.contains('√'), "sqrt not rendered: {:?}", t);
    }

    #[test]
    fn never_panics_on_garbage() {
        // Must not panic on malformed / multi-byte input.
        let _ = job_text("\\frac{a}{ é \\left( \\sum_{");
        let _ = job_text("ψ \\rangle \\langle φ |");
        let _ = job_text("");
    }

    #[test]
    fn greek_and_symbols_convert() {
        let t = job_text("\\alpha + \\beta \\leq \\gamma \\times \\delta");
        assert!(t.contains('α') && t.contains('β'), "greek missing: {:?}", t);
        assert!(t.contains('≤') && t.contains('×'), "symbols missing: {:?}", t);
    }
}
