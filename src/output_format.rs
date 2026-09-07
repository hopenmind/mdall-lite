//! The output formats offered by the Conversion Hub, with their file extension,
//! label, icon and brand color (adapted to the warm Heimdall palette).

use eframe::egui;

/// All supported output formats in the conversion hub.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum OutputFormat {
    Pdf, Docx, Html, Epub, Md, Odt, Txt, Tex, Rtf,
    Org, Rst, Adoc, Ipynb,
}

impl OutputFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Pdf  => "pdf",  Self::Docx => "docx",
            Self::Html => "html", Self::Epub => "epub",
            Self::Md   => "md",   Self::Odt  => "odt",
            Self::Txt  => "txt",  Self::Tex  => "tex",
            Self::Rtf   => "rtf",
            Self::Org   => "org",
            Self::Rst   => "rst",
            Self::Adoc  => "adoc",
            Self::Ipynb => "ipynb",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf  => "PDF",  Self::Docx => "DOCX",
            Self::Html => "HTML", Self::Epub => "EPUB",
            Self::Md   => "MD",   Self::Odt  => "ODT",
            Self::Txt  => "TXT",  Self::Tex  => "TEX",
            Self::Rtf  => "RTF",  Self::Org  => "ORG",
            Self::Rst  => "RST",  Self::Adoc => "ADOC",
            Self::Ipynb => "IPYNB",
        }
    }
    #[allow(dead_code)] // per-format icon, for the future format grid
    pub fn icon(self) -> &'static str {
        match self {
            Self::Pdf  => "📄", Self::Docx => "📝",
            Self::Html => "🌐", Self::Epub => "📚",
            Self::Md   => "📋", Self::Odt  => "📄",
            Self::Txt  => "📃", Self::Tex  => "🔤",
            Self::Rtf  => "📑", Self::Org  => "🌿",
            Self::Rst  => "📰", Self::Adoc => "📒",
            Self::Ipynb => "🔬",
        }
    }
    /// Brand color - adapted to warm Heimdall palette while preserving format identity.
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Pdf   => egui::Color32::from_rgb(196, 43,  26),  // Adobe red  #C42B1A
            Self::Docx  => egui::Color32::from_rgb(43,  85,  168), // Word blue  #2B55A8
            Self::Html  => egui::Color32::from_rgb(196, 102, 14),  // Web orange #C4660E
            Self::Epub  => egui::Color32::from_rgb(42,  112, 64),  // Forest     #2A7040
            Self::Md    => egui::Color32::from_rgb(92,  82,  69),  // Ink grey   #5C5245
            Self::Odt   => egui::Color32::from_rgb(21,  132, 122), // Teal       #15847A
            Self::Txt   => egui::Color32::from_rgb(122, 106, 85),  // Warm grey  #7A6A55
            Self::Tex   => egui::Color32::from_rgb(90,  53,  144), // LaTeX mauve#5A3590
            Self::Rtf   => egui::Color32::from_rgb(123, 64,  16),  // Bronze     #7B4010
            Self::Org   => egui::Color32::from_rgb(58,  120, 32),  // Emacs green#3A7820
            Self::Rst   => egui::Color32::from_rgb(42,  78,  122), // Slate      #2A4E7A
            Self::Adoc  => egui::Color32::from_rgb(139, 46,  16),  // Brick      #8B2E10
            Self::Ipynb => egui::Color32::from_rgb(208, 96,  10),  // Jupyter    #D0600A
        }
    }
}
