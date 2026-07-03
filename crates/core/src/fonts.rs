//! Embedded UI font access.
//!
//! Returns font bytes from Typst's bundled assets so the egui GUI has a real
//! serif and a math face on platforms without a system serif: Linux, macOS, or
//! Windows without Cambria. This is pure data with no UI dependency; the binary
//! feeds the bytes into its own egui font stack and layers system fonts (when
//! present) on top.

use typst::foundations::Bytes;
use typst::text::{Font, FontStyle};

/// Bytes of an embedded upright serif (New Computer Modern Regular, with
/// Libertinus Serif as a secondary), or `None` if Typst ships neither.
pub fn embedded_ui_serif() -> Option<&'static [u8]> {
    find_regular(&["New Computer Modern", "Libertinus Serif"])
}

/// Bytes of the embedded math font (New Computer Modern Math), or `None`.
/// Carries the OpenType MATH table needed for symbol/operator coverage in the
/// inline-math fallback when no system math font is available.
pub fn embedded_ui_math() -> Option<&'static [u8]> {
    for data in typst_assets::fonts() {
        if let Some(f) = Font::new(Bytes::from_static(data), 0) {
            if f.info().family == "New Computer Modern Math" {
                return Some(data);
            }
        }
    }
    None
}

/// First upright, regular-weight face whose family matches one of `families`,
/// scanned in priority order.
fn find_regular(families: &[&str]) -> Option<&'static [u8]> {
    for &want in families {
        for data in typst_assets::fonts() {
            if let Some(f) = Font::new(Bytes::from_static(data), 0) {
                let info = f.info();
                if info.family == want
                    && info.variant.style == FontStyle::Normal
                    && info.variant.weight.to_number() <= 450
                {
                    return Some(data);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_serif_and_math_are_available() {
        // Typst's bundled assets must provide both faces, otherwise the GUI
        // would fall back to bare egui fonts on Linux/macOS.
        assert!(embedded_ui_serif().is_some(), "no embedded serif found");
        assert!(embedded_ui_math().is_some(), "no embedded math font found");
    }
}
