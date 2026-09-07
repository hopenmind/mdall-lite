//! Persisted user preferences (UI language, theme). Stored as JSON under
//! `%APPDATA%/MD-ALL/settings.json` (falls back next to the executable). Loaded
//! once at startup; saved automatically when a preference changes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_true() -> bool { true }
fn default_icon_set() -> String { "sober".into() }
fn default_scale() -> f32 { 1.0 }
fn default_page_color() -> [u8; 3] { [255, 255, 255] }

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Settings {
    pub app_lang: String,
    pub dark_mode: bool,
    /// PDF engine: true = Native converter (pure-Rust), false = General converter.
    #[serde(default)]
    pub pdf_native: bool,
    /// Source code editor: line-number gutter (default on).
    #[serde(default = "default_true")]
    pub source_line_numbers: bool,
    /// Source code editor: soft-wrap long lines (default off).
    #[serde(default)]
    pub source_wrap: bool,
    /// WYSIWYG editing toolbar style: true = minified (default), false = full.
    #[serde(default = "default_true")]
    pub toolbar_minified: bool,
    /// Editing-toolbar icon style: "sober" (default), "colored", "high_contrast".
    #[serde(default = "default_icon_set")]
    pub icon_set: String,
    /// Accessibility: global interface scale (WCAG 1.4.4), 1.0 = 100%.
    #[serde(default = "default_scale")]
    pub ui_scale: f32,
    /// Accessibility: maximum-contrast text and edges (WCAG 1.4.6).
    #[serde(default)]
    pub a11y_high_contrast: bool,
    /// Accessibility: disable UI animations (WCAG 2.3.3).
    #[serde(default)]
    pub a11y_reduced_motion: bool,
    /// Accessibility: larger click/touch targets (WCAG 2.5.5).
    #[serde(default)]
    pub a11y_large_targets: bool,
    /// Print layout: show a page number in each sheet's footer (default on).
    #[serde(default = "default_true")]
    pub show_page_numbers: bool,
    /// Print layout: optional header text repeated at the top of every page.
    #[serde(default)]
    pub header_text: String,
    /// Print layout: optional footer text repeated at the bottom of every page.
    #[serde(default)]
    pub footer_text: String,
    /// Print layout: A4 sheet fill colour (RGB); default white.
    #[serde(default = "default_page_color")]
    pub page_color: [u8; 3],
    /// Print layout: draw a thin frame/border around each sheet.
    #[serde(default)]
    pub page_frame: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            app_lang: "en".into(),
            dark_mode: false,
            pdf_native: false,
            source_line_numbers: true,
            source_wrap: false,
            toolbar_minified: true,
            icon_set: "sober".into(),
            ui_scale: 1.0,
            a11y_high_contrast: false,
            a11y_reduced_motion: false,
            a11y_large_targets: false,
            show_page_numbers: true,
            header_text: String::new(),
            footer_text: String::new(),
            page_color: [255, 255, 255],
            page_frame: false,
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        })?;
    Some(base.join("MD-ALL").join("settings.json"))
}

/// Load saved preferences, or defaults if absent/corrupt.
pub fn load() -> Settings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist preferences (best effort; failures are silent - prefs are not critical).
pub fn save(s: &Settings) {
    let Some(path) = settings_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let s = Settings { app_lang: "fr".into(), dark_mode: true, pdf_native: true,
                           source_line_numbers: false, source_wrap: true, toolbar_minified: false,
                           icon_set: "colored".into(), ui_scale: 1.25, a11y_high_contrast: true,
                           a11y_reduced_motion: true, a11y_large_targets: true,
                           show_page_numbers: false, header_text: "Draft".into(),
                           footer_text: " confidential".into(),
                           page_color: [240, 240, 230], page_frame: true };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn default_is_english_light() {
        let d = Settings::default();
        assert_eq!(d.app_lang, "en");
        assert!(!d.dark_mode);
    }
}
