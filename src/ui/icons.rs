//! Monochrome vector icons drawn directly with the egui `Painter`.
//!
//! Why painter geometry instead of emoji or bundled SVG files: it stays fully
//! offline (no asset files, no loaders), tints to any color for light/dark
//! themes, and renders crisply at any size. The drawing grid is a 24x24 viewBox
//! (Lucide convention); each glyph maps that viewBox into the target rect.
//!
//! Symbolic icons (open, save, link, search, ...) are stroked paths. A few
//! letterform icons (bold, underline, strikethrough, sigma) use a centered text
//! glyph, since hand-stroking legible letters is not worth the geometry.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke};

use crate::theme;

/// The available toolbar / UI icons.
#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)] // full icon set; Save/Sigma not placed in the toolbar yet
pub enum Icon {
    Open,
    Save,
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
    Link,
    Image,
    Search,
    ListBullet,
    ListNumber,
    Quote,
    Rule,
    Table,
    Sigma,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
    Settings,
    Sun,
    Moon,
    Close,
    ChevronDown,
}

/// Stroke width (screen px) used for all stroked icons.
const W: f32 = 1.6;

/// An icon button: transparent at rest, subtle warm fill + gold tint on hover.
/// Returns the `Response` (with the tooltip attached when non-empty).
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    let size = egui::vec2(30.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());

    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::same(6.0),
            ui.visuals().widgets.hovered.bg_fill,
        );
    }
    let color = if resp.hovered() {
        theme::ACCENT
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    paint_icon(ui.painter(), icon, rect.shrink(7.0), color);

    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}

/// Editing-toolbar icon style (accessibility option). Sober is the default;
/// Colored groups icons by family for faster scanning (colour + shape, never
/// colour alone, per WCAG 1.4.1); HighContrast maximises legibility.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconSet {
    Sober,
    Colored,
    HighContrast,
}

impl IconSet {
    pub fn as_key(self) -> &'static str {
        match self {
            IconSet::Sober => "sober",
            IconSet::Colored => "colored",
            IconSet::HighContrast => "high_contrast",
        }
    }
    pub fn from_key(s: &str) -> Self {
        match s {
            "colored" => IconSet::Colored,
            "high_contrast" => IconSet::HighContrast,
            _ => IconSet::Sober,
        }
    }
}

// Process-global current icon set (a single app preference, like the PDF engine).
static ICON_SET: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Select the editing-toolbar icon style.
pub fn set_icon_set(s: IconSet) {
    ICON_SET.store(s as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The currently selected editing-toolbar icon style.
pub fn icon_set() -> IconSet {
    match ICON_SET.load(std::sync::atomic::Ordering::Relaxed) {
        1 => IconSet::Colored,
        2 => IconSet::HighContrast,
        _ => IconSet::Sober,
    }
}

enum Family { Text, Block, Media, Math, Other }

fn icon_family(icon: Icon) -> Family {
    match icon {
        Icon::Bold | Icon::Italic | Icon::Underline | Icon::Strikethrough => Family::Text,
        Icon::ListBullet | Icon::ListNumber | Icon::Quote | Icon::Table | Icon::Rule => Family::Block,
        Icon::Link | Icon::Image => Family::Media,
        Icon::Sigma | Icon::Code => Family::Math,
        _ => Family::Other,
    }
}

/// (rest glyph, hover glyph, pale hover background) for a family in the Colored set.
fn family_colors(icon: Icon) -> (egui::Color32, egui::Color32, egui::Color32) {
    use egui::Color32 as C;
    match icon_family(icon) {
        Family::Text => (C::from_rgb(0x18, 0x5F, 0xA5), C::from_rgb(0x0C, 0x44, 0x7C), C::from_rgb(0xE6, 0xF1, 0xFB)),
        Family::Block => (C::from_rgb(0x0F, 0x6E, 0x56), C::from_rgb(0x08, 0x50, 0x41), C::from_rgb(0xE1, 0xF5, 0xEE)),
        Family::Media => (C::from_rgb(0x53, 0x4A, 0xB7), C::from_rgb(0x3C, 0x34, 0x89), C::from_rgb(0xEE, 0xED, 0xFE)),
        Family::Math => (C::from_rgb(0x99, 0x3C, 0x1D), C::from_rgb(0x71, 0x2B, 0x13), C::from_rgb(0xFA, 0xEC, 0xE7)),
        Family::Other => (theme::TEXT_2, theme::ACCENT_HOVER, theme::ACCENT_PALE),
    }
}

/// Resolve (rest, hover glyph, hover/active background, active glyph) for the
/// current icon set. `active_bg` is the fill used when a toggle is on.
fn lively_palette(icon: Icon) -> (egui::Color32, egui::Color32, egui::Color32, egui::Color32, egui::Color32) {
    match icon_set() {
        IconSet::Colored => {
            let (rest, hover, pale) = family_colors(icon);
            (rest, hover, pale, rest, egui::Color32::WHITE)
        }
        IconSet::HighContrast => {
            (theme::TEXT, theme::ACCENT_HOVER, theme::ACCENT_PALE, theme::ACCENT, theme::TEXT)
        }
        IconSet::Sober => {
            (theme::TEXT_2, theme::ACCENT_HOVER, theme::ACCENT_PALE, theme::ACCENT, theme::TEXT)
        }
    }
}

/// Larger editing-toolbar icon button: a bigger glyph that washes on hover, in
/// the style of the selected icon set, so the bar reads as "easy editing".
pub fn lively_icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    let size = egui::vec2(38.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hov = resp.hovered();
    let (rest, hover_c, pale, _active_bg, _active_fg) = lively_palette(icon);
    if hov {
        ui.painter().rect_filled(rect, egui::Rounding::same(8.0), pale);
    }
    let color = if hov { hover_c } else { rest };
    paint_icon(ui.painter(), icon, rect.shrink(8.0), color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// Toggle variant of [`lively_icon_button`]: fills with the set's active colour
/// when on (gold in Sober/HighContrast, the family hue in Colored).
pub fn lively_icon_toggle(ui: &mut egui::Ui, icon: Icon, active: bool, tooltip: &str) -> egui::Response {
    let size = egui::vec2(38.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hov = resp.hovered();
    let (rest, hover_c, pale, active_bg, active_fg) = lively_palette(icon);
    if active {
        ui.painter().rect_filled(rect, egui::Rounding::same(8.0), active_bg);
    } else if hov {
        ui.painter().rect_filled(rect, egui::Rounding::same(8.0), pale);
    }
    let color = if active { active_fg } else if hov { hover_c } else { rest };
    paint_icon(ui.painter(), icon, rect.shrink(8.0), color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// A toggle-style icon button: shows the active (gold) state when `active`.
pub fn icon_toggle(ui: &mut egui::Ui, icon: Icon, active: bool, tooltip: &str) -> egui::Response {
    let size = egui::vec2(30.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());

    if active {
        ui.painter().rect_filled(rect, egui::Rounding::same(6.0), theme::ACCENT);
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::same(6.0),
            ui.visuals().widgets.hovered.bg_fill,
        );
    }
    let color = if active {
        theme::TEXT
    } else if resp.hovered() {
        theme::ACCENT
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    paint_icon(ui.painter(), icon, rect.shrink(7.0), color);

    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}

/// Draw `icon` inside `rect`, tinted `color`. Coordinates use a 0..24 viewBox.
pub fn paint_icon(p: &egui::Painter, icon: Icon, rect: egui::Rect, color: Color32) {
    let s = Stroke::new(W, color);
    // Map a viewBox point (0..24) into the target rect.
    let m = |x: f32, y: f32| -> Pos2 {
        egui::pos2(
            rect.left() + x / 24.0 * rect.width(),
            rect.top() + y / 24.0 * rect.height(),
        )
    };
    // Polyline from viewBox points.
    let line = |p: &egui::Painter, pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| m(x, y)).collect();
        p.add(egui::Shape::line(v, s));
    };
    // Single segment.
    let seg = |p: &egui::Painter, a: (f32, f32), b: (f32, f32)| {
        p.line_segment([m(a.0, a.1), m(b.0, b.1)], s);
    };
    // Centered text glyph (for letterforms).
    let glyph = |p: &egui::Painter, ch: &str| {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            ch,
            FontId::proportional(rect.height() * 0.92),
            color,
        );
    };

    match icon {
        Icon::Open => {
            // Open folder outline.
            line(p, &[(3.0, 7.0), (9.0, 7.0), (11.0, 9.0), (21.0, 9.0), (21.0, 19.0), (3.0, 19.0), (3.0, 7.0)]);
        }
        Icon::Save => {
            // Floppy disk: body, top slot, label.
            line(p, &[(4.0, 4.0), (16.0, 4.0), (20.0, 8.0), (20.0, 20.0), (4.0, 20.0), (4.0, 4.0)]);
            p.rect_stroke(egui::Rect::from_min_max(m(8.0, 4.0), m(15.0, 9.0)), egui::Rounding::ZERO, s);
            p.rect_stroke(egui::Rect::from_min_max(m(7.0, 13.0), m(17.0, 20.0)), egui::Rounding::ZERO, s);
        }
        Icon::Bold => glyph(p, "B"),
        Icon::Italic => {
            // Slanted I (top serif, bottom serif, diagonal).
            seg(p, (9.0, 5.0), (16.0, 5.0));
            seg(p, (8.0, 19.0), (15.0, 19.0));
            seg(p, (14.0, 5.0), (10.0, 19.0));
        }
        Icon::Underline => {
            glyph(p, "U");
            seg(p, (5.0, 21.0), (19.0, 21.0));
        }
        Icon::Strikethrough => {
            glyph(p, "S");
            seg(p, (4.0, 12.0), (20.0, 12.0));
        }
        Icon::Code => {
            // Two chevrons </>.
            line(p, &[(10.0, 8.0), (6.0, 12.0), (10.0, 16.0)]);
            line(p, &[(14.0, 8.0), (18.0, 12.0), (14.0, 16.0)]);
        }
        Icon::Link => {
            // Two overlapping rounded link halves.
            let r = egui::Rounding::same((rect.height() / 24.0 * 2.5).max(2.0));
            p.rect_stroke(egui::Rect::from_min_max(m(3.0, 9.0), m(13.0, 15.0)), r, s);
            p.rect_stroke(egui::Rect::from_min_max(m(11.0, 9.0), m(21.0, 15.0)), r, s);
        }
        Icon::Image => {
            p.rect_stroke(egui::Rect::from_min_max(m(3.0, 4.0), m(21.0, 20.0)), egui::Rounding::same(2.0), s);
            p.circle_stroke(m(8.5, 9.0), rect.height() / 24.0 * 1.6, s);
            line(p, &[(4.0, 18.0), (9.0, 13.0), (13.0, 17.0), (16.0, 13.0), (20.0, 18.0)]);
        }
        Icon::Search => {
            p.circle_stroke(m(10.5, 10.5), rect.height() / 24.0 * 6.0, s);
            seg(p, (15.0, 15.0), (20.0, 20.0));
        }
        Icon::ListBullet => {
            for &y in &[7.0_f32, 12.0, 17.0] {
                p.circle_filled(m(5.0, y), W, color);
                seg(p, (9.0, y), (20.0, y));
            }
        }
        Icon::ListNumber => {
            let small = FontId::proportional(rect.height() * 0.30);
            for (i, &y) in [7.0_f32, 12.0, 17.0].iter().enumerate() {
                p.text(m(5.0, y), Align2::CENTER_CENTER, format!("{}", i + 1), small.clone(), color);
                seg(p, (9.0, y), (20.0, y));
            }
        }
        Icon::Quote => {
            // Blockquote bar + lines.
            let bar = Stroke::new(W * 1.8, color);
            p.line_segment([m(5.0, 5.0), m(5.0, 19.0)], bar);
            seg(p, (9.0, 9.0), (19.0, 9.0));
            seg(p, (9.0, 15.0), (16.0, 15.0));
        }
        Icon::Rule => {
            seg(p, (4.0, 12.0), (20.0, 12.0));
        }
        Icon::Table => {
            p.rect_stroke(egui::Rect::from_min_max(m(3.0, 5.0), m(21.0, 19.0)), egui::Rounding::same(1.5), s);
            seg(p, (9.0, 5.0), (9.0, 19.0));
            seg(p, (15.0, 5.0), (15.0, 19.0));
            seg(p, (3.0, 12.0), (21.0, 12.0));
        }
        Icon::Sigma => glyph(p, "\u{03A3}"), // Σ
        Icon::AlignLeft => {
            seg(p, (4.0, 7.0), (20.0, 7.0));
            seg(p, (4.0, 12.0), (13.0, 12.0));
            seg(p, (4.0, 17.0), (17.0, 17.0));
        }
        Icon::AlignCenter => {
            seg(p, (4.0, 7.0), (20.0, 7.0));
            seg(p, (7.0, 12.0), (17.0, 12.0));
            seg(p, (5.0, 17.0), (19.0, 17.0));
        }
        Icon::AlignRight => {
            seg(p, (4.0, 7.0), (20.0, 7.0));
            seg(p, (11.0, 12.0), (20.0, 12.0));
            seg(p, (7.0, 17.0), (20.0, 17.0));
        }
        Icon::AlignJustify => {
            seg(p, (4.0, 7.0), (20.0, 7.0));
            seg(p, (4.0, 12.0), (20.0, 12.0));
            seg(p, (4.0, 17.0), (20.0, 17.0));
        }
        Icon::Settings => {
            let c = m(12.0, 12.0);
            let unit = rect.height() / 24.0;
            p.circle_stroke(c, unit * 3.0, s);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (sa, ca) = (a.sin(), a.cos());
                p.line_segment(
                    [
                        egui::pos2(c.x + ca * unit * 4.0, c.y + sa * unit * 4.0),
                        egui::pos2(c.x + ca * unit * 6.5, c.y + sa * unit * 6.5),
                    ],
                    s,
                );
            }
        }
        Icon::Sun => {
            let c = m(12.0, 12.0);
            let unit = rect.height() / 24.0;
            p.circle_filled(c, unit * 3.7, s.color);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (sa, ca) = (a.sin(), a.cos());
                p.line_segment(
                    [
                        egui::pos2(c.x + ca * unit * 5.7, c.y + sa * unit * 5.7),
                        egui::pos2(c.x + ca * unit * 8.4, c.y + sa * unit * 8.4),
                    ],
                    s,
                );
            }
        }
        Icon::Moon => {
            // A filled-looking crescent (lune): outer disc arc minus an offset arc,
            // drawn as one closed outline so it clearly reads as a moon.
            let c = m(12.0, 12.0);
            let u = rect.height() / 24.0;
            let (rr, ri, d) = (7.0 * u, 6.0 * u, 4.2 * u);
            let ix = (d * d + rr * rr - ri * ri) / (2.0 * d);
            let iy = (rr * rr - ix * ix).max(0.0).sqrt();
            let a_up = iy.atan2(ix);
            let bi_up = iy.atan2(ix - d);
            let inner = egui::pos2(c.x + d, c.y);
            let pi = std::f32::consts::PI;
            let n = 24;
            let mut pts: Vec<Pos2> = Vec::with_capacity(2 * n + 2);
            // Outer arc: the far (left) side of the outer circle. Screen y is flipped.
            for i in 0..=n {
                let a = a_up + (i as f32 / n as f32) * (2.0 * pi - 2.0 * a_up);
                pts.push(egui::pos2(c.x + a.cos() * rr, c.y - a.sin() * rr));
            }
            // Carving arc traced back along the offset circle's left side.
            let lo = 2.0 * pi - bi_up;
            for i in 0..=n {
                let a = lo - (i as f32 / n as f32) * (lo - bi_up);
                pts.push(egui::pos2(inner.x + a.cos() * ri, inner.y - a.sin() * ri));
            }
            p.add(egui::Shape::closed_line(pts, s));
        }
        Icon::Close => {
            seg(p, (6.0, 6.0), (18.0, 18.0));
            seg(p, (18.0, 6.0), (6.0, 18.0));
        }
        Icon::ChevronDown => {
            line(p, &[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]);
        }
    }
}
