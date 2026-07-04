//! Heimdall Design System (MD -> ALL lite) - violet + magenta + silver theme (light mode).
//! Derived from the lite logo: violet ink + magenta accent + silver (the chrome "A")
//! on a light lavender desk. Gold and cream belong to the classic editor, not lite.
//! Dark theme: derive by inverting HSL lightness while preserving hue/saturation.

use eframe::egui::{self, Color32, Stroke, Rounding};

// ── Color tokens ─────────────────────────────────────────────────────────
pub const BG:           Color32 = Color32::from_rgb(244, 239, 251); // #F4EFFB lavender-white
pub const SURFACE:      Color32 = Color32::WHITE;
pub const SURFACE_SOFT: Color32 = Color32::from_rgb(236, 227, 247); // #ECE3F7 soft lavender
pub const SURFACE_ALT:  Color32 = Color32::from_rgb(225, 212, 243); // #E1D4F3
pub const TEXT:         Color32 = Color32::from_rgb(36, 23, 52);    // #241734 deep violet ink
pub const TEXT_2:       Color32 = Color32::from_rgb(78, 61, 102);   // #4E3D66
pub const TEXT_MUTED:   Color32 = Color32::from_rgb(139, 123, 166); // #8B7BA6
pub const ACCENT:       Color32 = Color32::from_rgb(160, 32, 192);  // #A020C0 logo magenta
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(123, 24, 156);  // #7B189C
pub const ACCENT_PALE:  Color32 = Color32::from_rgb(236, 214, 245); // #ECD6F5 pale lavender
pub const BORDER:       Color32 = Color32::from_rgb(231, 220, 246); // #E7DCF6 lavender border
pub const SELECTION:    Color32 = Color32::from_rgb(231, 204, 247); // #E7CCF7 pale purple
pub const DESKTOP:      Color32 = Color32::from_rgb(212, 193, 236); // #D4C1EC lavender-violet desk
pub const EQ_BG:        Color32 = Color32::from_rgb(243, 236, 251); // #F3ECFB lavender
pub const SUCCESS:      Color32 = Color32::from_rgb(45, 122, 69);   // #2D7A45
pub const ERROR:        Color32 = Color32::from_rgb(181, 61, 42);   // #B53D2A

/// Modern spacing/elevation pass - applied once at startup after the visuals.
/// Gives the interface "air" (comfortable padding, vertical rhythm, thin scrollbars,
/// consistent rounding) so it reads 2024, not 2010. The dense toolbar overrides its own
/// local item_spacing, so this does not break its layout.
pub fn apply_modern_style(ctx: &egui::Context) {
    let mut s = (*ctx.style()).clone();
    // Comfortable, modern padding + rhythm
    s.spacing.item_spacing      = egui::vec2(8.0, 7.0);   // more vertical air
    s.spacing.button_padding    = egui::vec2(10.0, 6.0);  // generous click targets
    s.spacing.interact_size.y   = 24.0;
    s.spacing.menu_margin       = egui::Margin::same(8.0);
    s.spacing.window_margin     = egui::Margin::same(10.0);
    s.spacing.indent            = 18.0;
    // Thin modern scrollbars
    s.spacing.scroll.bar_width        = 8.0;
    s.spacing.scroll.floating         = true;
    s.spacing.scroll.bar_inner_margin = 2.0;
    // Consistent corner rounding across all widget states
    let r = egui::Rounding::same(6.0);
    for w in [
        &mut s.visuals.widgets.noninteractive,
        &mut s.visuals.widgets.inactive,
        &mut s.visuals.widgets.hovered,
        &mut s.visuals.widgets.active,
        &mut s.visuals.widgets.open,
    ] { w.rounding = r; w.expansion = 1.0; }  // tiny hover/press expansion = subtle motion
    s.visuals.menu_rounding = egui::Rounding::same(8.0);
    ctx.set_style(s);
}

pub fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();

    // Surfaces
    v.panel_fill       = BG;
    v.window_fill      = SURFACE;
    v.faint_bg_color   = SURFACE_SOFT;
    v.extreme_bg_color = SURFACE_ALT;
    v.code_bg_color    = EQ_BG;

    // Selection - warm gold
    v.selection.bg_fill = SELECTION;
    // egui colors a SELECTED selectable_label's text with selection.stroke.color
    // (via interact_selectable). With Stroke::NONE that color is transparent, so
    // the label text vanished into the sand fill. Use dark ink at width 0 → readable
    // selected text, and width 0 means no outline is drawn over text selections.
    v.selection.stroke  = Stroke::new(0.0, TEXT);
    v.hyperlink_color   = ACCENT;

    // Window chrome
    v.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 3.0), blur: 10.0, spread: 0.0,
        color: Color32::from_rgba_unmultiplied(42, 23, 88, 28),
    };
    v.window_stroke  = Stroke::new(1.0, BORDER);
    v.window_rounding = Rounding::same(8.0);

    // Widgets - NonInteractive
    v.widgets.noninteractive.bg_fill   = SURFACE_SOFT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.rounding  = Rounding::same(5.0);

    // Widgets - Inactive - warm grey (replaces egui default cold grey)
    v.widgets.inactive.bg_fill      = Color32::from_rgb(228, 220, 243); // lavender grey
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(237, 231, 250); // lighter lavender grey
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_2);
    v.widgets.inactive.rounding  = Rounding::same(5.0);

    // Widgets - Hovered
    v.widgets.hovered.bg_fill   = ACCENT_PALE;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    v.widgets.hovered.rounding  = Rounding::same(5.0);

    // Widgets - Active / Pressed
    // ACCENT magenta-violet bg (#A020C0) is dark, so WHITE text (not dark ink)
    // keeps the pressed label readable (white on #A020C0 ~ 5.6:1, AA).
    v.widgets.active.bg_fill   = ACCENT;
    v.widgets.active.bg_stroke = Stroke::new(1.5, ACCENT_HOVER);
    v.widgets.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    v.widgets.active.rounding  = Rounding::same(5.0);

    // Widgets - Open (ComboBox, menus)
    v.widgets.open.bg_fill   = ACCENT_PALE;
    v.widgets.open.bg_stroke = Stroke::new(1.5, ACCENT);
    v.widgets.open.fg_stroke = Stroke::new(1.5, TEXT);
    v.widgets.open.rounding  = Rounding::same(5.0);

    v
}

// ── Theme selection + dark mode (warm negative) ──────────────────────────────

/// Resolve the visuals for the active theme.
/// Light warm Heimdall is the default identity; dark is an opt-in option.
pub fn current_visuals(dark: bool) -> egui::Visuals {
    if dark { build_dark_visuals() } else { light_visuals() }
}

// ── Theme-aware surface / text accessors ─────────────────────────────────────
// Light returns the canonical Heimdall tokens (the identity). Dark returns the
// warm-inverted variant. The A4 "paper" intentionally stays light in both themes
// so black Typst equation images and ink stay legible; only the surrounding
// desktop and the chrome (toolbar / status / popups) go dark.

/// Toolbar / status chrome background.
pub fn panel_bg(dark: bool) -> Color32 {
    if dark { invert_lightness(BG) } else { BG }
}
/// The desktop area surrounding the page (margins).
pub fn desktop_bg(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(20, 17, 14) } else { DESKTOP }
}
/// Soft warm surface (status bar, inset panels).
pub fn surface_soft_c(dark: bool) -> Color32 {
    if dark { invert_lightness(SURFACE_SOFT) } else { SURFACE_SOFT }
}
/// Inactive button fill (warm grey).
pub fn btn_fill(dark: bool) -> Color32 {
    let light = Color32::from_rgb(230, 222, 244);
    if dark { invert_lightness(light) } else { light }
}
/// Primary text.
pub fn text_strong(dark: bool) -> Color32 {
    if dark { invert_lightness(TEXT) } else { TEXT }
}
/// Secondary text.
pub fn text_soft(dark: bool) -> Color32 {
    if dark { invert_lightness(TEXT_2) } else { TEXT_2 }
}
/// Muted / hint text.
pub fn text_faint(dark: bool) -> Color32 {
    if dark { invert_lightness(TEXT_MUTED) } else { TEXT_MUTED }
}

/// Dark "warm negative" theme, derived from the light Heimdall palette by
/// inverting HSL lightness while preserving the warm hue/saturation. The gold
/// accent (#C9920A) is intentionally kept unchanged so the brand reads the same.
pub fn build_dark_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();

    // Warm-negative surfaces and text, derived from the light tokens.
    let bg           = invert_lightness(BG);
    let surface      = invert_lightness(SURFACE);
    let surface_soft = invert_lightness(SURFACE_SOFT);
    let surface_alt  = invert_lightness(SURFACE_ALT);
    let text         = invert_lightness(TEXT);
    let text_2       = invert_lightness(TEXT_2);
    let border       = invert_lightness(BORDER);
    let eq_bg        = invert_lightness(EQ_BG);
    // Dark gold-brown derived from the pale gold, used for hover/open fills.
    let accent_dim   = invert_lightness(ACCENT_PALE);

    // Surfaces
    v.panel_fill       = bg;
    v.window_fill      = surface;
    v.faint_bg_color   = surface_soft;
    v.extreme_bg_color = surface_alt;
    v.code_bg_color    = eq_bg;

    // Selection - translucent accent violet (accent hue unchanged)
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(160, 32, 192, 110);
    // Readable selected-label text (see light_visuals note); width 0 = no outline.
    v.selection.stroke  = Stroke::new(0.0, text);
    v.hyperlink_color   = ACCENT;

    // Window chrome - deeper shadow on dark
    v.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 3.0), blur: 12.0, spread: 0.0,
        color: Color32::from_rgba_unmultiplied(0, 0, 0, 90),
    };
    v.window_stroke   = Stroke::new(1.0, border);
    v.window_rounding = Rounding::same(8.0);

    // Widgets - NonInteractive
    v.widgets.noninteractive.bg_fill   = surface_soft;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.noninteractive.rounding  = Rounding::same(5.0);

    // Widgets - Inactive - warm dark grey
    v.widgets.inactive.bg_fill      = surface_alt;
    v.widgets.inactive.weak_bg_fill = surface_soft;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, text_2);
    v.widgets.inactive.rounding  = Rounding::same(5.0);

    // Widgets - Hovered - dark gold-brown + gold edge
    v.widgets.hovered.bg_fill   = accent_dim;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, text);
    v.widgets.hovered.rounding  = Rounding::same(5.0);

    // Widgets - Active / Pressed - accent bg + white ink (same as light, AA contrast)
    v.widgets.active.bg_fill   = ACCENT;
    v.widgets.active.bg_stroke = Stroke::new(1.5, ACCENT_HOVER);
    v.widgets.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    v.widgets.active.rounding  = Rounding::same(5.0);

    // Widgets - Open (ComboBox, menus)
    v.widgets.open.bg_fill   = accent_dim;
    v.widgets.open.bg_stroke = Stroke::new(1.5, ACCENT);
    v.widgets.open.fg_stroke = Stroke::new(1.5, text);
    v.widgets.open.rounding  = Rounding::same(5.0);

    v
}

/// Push the given visuals to a high-contrast variant (WCAG 1.4.6): force all
/// text to near-pure ink/parchment and strengthen widget edges. Reuses the warm
/// extremes so the identity is kept, just at maximum legibility.
pub fn apply_high_contrast(v: &mut egui::Visuals, dark: bool) {
    let ink  = if dark { Color32::from_rgb(246, 243, 236) } else { Color32::from_rgb(15, 11, 5) };
    let edge = if dark { Color32::from_rgb(205, 194, 178) } else { Color32::from_rgb(38, 28, 16) };
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.fg_stroke.color = ink;
        w.bg_stroke = Stroke::new(w.bg_stroke.width.max(1.5), edge);
    }
    v.override_text_color = Some(ink);
    v.window_stroke = Stroke::new(1.5, edge);
}

/// Invert the HSL lightness of a color, preserving hue and saturation.
/// Extremes are slightly compressed so surfaces are not pure black and text is
/// not pure white, keeping the warm tone readable. NOT applied to the gold accent.
fn invert_lightness(c: Color32) -> Color32 {
    let (h, s, l) = rgb_to_hsl(c);
    let nl = (1.0 - l) * 0.92 + 0.04;
    hsl_to_rgb(h, s, nl)
}

/// Convert sRGB (0-255) to HSL with hue in degrees [0,360), s/l in [0,1].
fn rgb_to_hsl(c: Color32) -> (f32, f32, f32) {
    let r = c.r() as f32 / 255.0;
    let g = c.g() as f32 / 255.0;
    let b = c.b() as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    (h, s, l)
}

/// Convert HSL (hue degrees, s/l in [0,1]) back to sRGB Color32.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp.floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to_u8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(to_u8(r1), to_u8(g1), to_u8(b1))
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn selected_label_text_is_opaque_and_contrasts_fill() {
        // Regression: selection.stroke was Stroke::NONE, so egui colored a SELECTED
        // selectable_label's text with a transparent stroke -> invisible "sand on
        // sand". The selected-text color must be opaque and differ from the fill.
        for dark in [false, true] {
            let v = current_visuals(dark);
            let c = v.selection.stroke.color;
            assert!(c.a() > 0, "selected label text is transparent (dark={dark})");
            assert_ne!(c, v.selection.bg_fill, "selected text == fill (dark={dark})");
        }
    }
}
