//! Module system: the shared scaffolding for "installable capability packs".
//!
//! The spell engine (dictionaries) is the first live module. Citation styles,
//! editor themes and export templates are reserved categories that will plug in
//! the same way. This module owns the ONE reusable piece every such pack needs:
//! the download/use row convention (green tick once installed, a green Use to
//! activate, an in-use marker), so each category renders identically.

use eframe::egui;
use crate::i18n::t;

/// Convention colour for "installed / available locally".
pub const DL_GREEN: egui::Color32 = egui::Color32::from_rgb(46, 160, 90);

/// State of one downloadable resource (e.g. a dictionary language).
pub enum DlState {
    NotInstalled,
    Downloading,
    Installed,
    Active,
}

/// What the user asked for on a downloadable row this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlAction {
    None,
    Download,
    Use,
    Redownload,
}

/// Render one downloadable-resource row with the shared convention and return
/// the action the user took. Pure UI: the caller maps the action onto its own
/// state, so this works for dictionaries today and CSL/themes/templates later.
pub fn downloadable_row(ui: &mut egui::Ui, label: &str, state: DlState) -> DlAction {
    let mut action = DlAction::None;
    ui.horizontal(|ui| {
        ui.label(label);
        match state {
            DlState::Downloading => {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(egui::RichText::new(t("module.downloading")).small().weak());
            }
            DlState::Active => {
                ui.label(egui::RichText::new(t("module.in_use")).color(DL_GREEN).strong());
            }
            DlState::Installed => {
                ui.label(egui::RichText::new("\u{2713}").color(DL_GREEN).strong());
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new(t("module.use")).color(egui::Color32::WHITE),
                    ).fill(DL_GREEN))
                    .clicked()
                {
                    action = DlAction::Use;
                }
                if ui.small_button("\u{21BB}").on_hover_text(t("module.redownload")).clicked() {
                    action = DlAction::Redownload;
                }
            }
            DlState::NotInstalled => {
                if ui.button(t("module.download")).clicked() {
                    action = DlAction::Download;
                }
            }
        }
    });
    action
}

/// The categories the Module window exposes, in tab order. Only `Dictionaries`
/// is live today; the rest are reserved slots that the registry already knows
/// about so the UI and future packs share one source of truth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleCategory {
    Dictionaries,
    Language,
    Citations,
    Themes,
    Templates,
}

impl ModuleCategory {
    /// All categories in tab order.
    pub fn all() -> [ModuleCategory; 5] {
        [
            Self::Dictionaries,
            Self::Language,
            Self::Citations,
            Self::Themes,
            Self::Templates,
        ]
    }

    /// i18n key for the tab title.
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Dictionaries => "module.tab.dictionaries",
            Self::Language => "module.tab.language",
            Self::Citations => "module.tab.citations",
            Self::Themes => "module.tab.themes",
            Self::Templates => "module.tab.templates",
        }
    }

    /// True when the category is implemented (vs a reserved placeholder).
    /// Part of the module registry surface; consumed once CSL/themes/templates
    /// packs wire into the panel.
    #[allow(dead_code)]
    pub fn is_live(self) -> bool {
        matches!(self, Self::Dictionaries | Self::Language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_cover_tabs_and_live_flags() {
        let all = ModuleCategory::all();
        assert_eq!(all.len(), 5);
        assert!(ModuleCategory::Dictionaries.is_live());
        assert!(ModuleCategory::Language.is_live());
        assert!(!ModuleCategory::Citations.is_live());
        // Every category has a non-empty title key.
        for c in all {
            assert!(c.title_key().starts_with("module.tab."));
        }
    }
}
