// Headless rendering engine PDF export (bundled engine, CDP protocol)
// No dependency on system browsers; fully controlled, isolated binary.

use crate::export::PdfMetadata;
use std::path::Path;

const ISOLATION_FLAGS: &[&str] = &[
    "--no-sandbox",
    "--disable-gpu",
    "--no-first-run",
    "--disable-default-apps",
    "--disable-sync",
    "--disable-translate",
    "--disable-extensions",
    "--disable-component-update",
    "--disable-background-networking",
    "--disable-client-side-phishing-detection",
    "--safebrowsing-disable-auto-update",
    "--metrics-recording-only",
    "--disable-features=ChromeWhatsNewUI,TranslateUI",
    "--no-default-browser-check",
    "--disable-popup-blocking",
];

/// The private per-user folder the installer extracts the rendering engine into,
/// so it is never placed next to the application.
fn engine_app_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|p| std::path::PathBuf::from(p).join("MD-ALL").join("engine"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|p| std::path::PathBuf::from(p).join("Library/Application Support/MD-ALL/engine"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
            .map(|p| p.join("md-all").join("engine"))
    }
}

/// Locate the bundled headless rendering engine. Search order:
///   1. private app folder (where the installer hides it)
///   2. <exe_dir>/engine/<binary>   - portable / dev build
///   3. <exe_dir>/chromium/<binary> - legacy layout (backward compatibility)
///   4. MD2ALL_ENGINE env var       - operator override
fn find_bundled_engine() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let names: &[&str] = &["chrome.exe"];
    #[cfg(target_os = "macos")]
    let names: &[&str] = &[
        "Chromium.app/Contents/MacOS/Chromium",
        "Chrome for Testing.app/Contents/MacOS/Chrome for Testing",
        "chrome",
    ];
    #[cfg(all(unix, not(target_os = "macos")))]
    let names: &[&str] = &["chrome", "chromium", "chrome-wrapper"];

    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Some(base) = engine_app_dir() {
        roots.push(base);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("engine"));
            roots.push(dir.join("chromium")); // legacy layout
        }
    }
    for root in &roots {
        for name in names {
            let candidate = root.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // Explicit override (no recompilation needed).
    if let Ok(val) = std::env::var("MD2ALL_ENGINE").or_else(|_| std::env::var("MD2ALL_CHROMIUM")) {
        let p = std::path::PathBuf::from(val);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

/// Synchronous entry point - wraps async CDP in a blocking Tokio runtime.
pub fn export_pdf_engine(
    markdown: &str,
    output_path: &Path,
    metadata: &PdfMetadata,
    source_dir: Option<&Path>,
) -> Result<(), String> {
    let engine = find_bundled_engine()
        .ok_or_else(|| "Bundled rendering engine not found".to_string())?;

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Tokio runtime error: {}", e))?;

    rt.block_on(run_cdp_export(markdown, output_path, metadata, source_dir, engine))
}

async fn run_cdp_export(
    markdown: &str,
    output_path: &Path,
    metadata: &PdfMetadata,
    source_dir: Option<&Path>,
    engine_path: std::path::PathBuf,
) -> Result<(), String> {
    use chromiumoxide::browser::{Browser, BrowserConfig};
    use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;
    use futures_util::StreamExt;

    // Write HTML to temp file - reuses the identical pipeline as HTML export
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let tmp_path = std::env::temp_dir().join(format!("md2all_{}.html", ts));
    crate::export::export_html(markdown, &tmp_path, metadata, source_dir)?;

    let tmp_url = format!(
        "file:///{}",
        tmp_path.display().to_string().replace('\\', "/")
    );

    // Build browser config with isolation flags
    let config = BrowserConfig::builder()
        .chrome_executable(engine_path)
        .args(ISOLATION_FLAGS.iter().copied())
        .build()
        .map_err(|e| format!("Engine config error: {}", e))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("Engine launch failed: {}", e))?;

    // Drive the handler in a background task - required for CDP to work
    tokio::spawn(async move {
        while let Some(_event) = handler.next().await {}
    });

    // Open page (navigates and waits for load event)
    let page = browser
        .new_page(&tmp_url)
        .await
        .map_err(|e| format!("Page navigation failed: {}", e))?;

    // Generate PDF with print-quality settings
    let pdf_params = PrintToPdfParams {
        print_background: Some(true),
        paper_width: Some(8.5),
        paper_height: Some(11.0),
        margin_top: Some(0.4),
        margin_bottom: Some(0.4),
        margin_left: Some(0.5),
        margin_right: Some(0.5),
        prefer_css_page_size: Some(true),
        ..Default::default()
    };

    let pdf_bytes = page
        .pdf(pdf_params)
        .await
        .map_err(|e| format!("PDF generation failed: {}", e))?;

    let _ = std::fs::remove_file(&tmp_path);

    if pdf_bytes.is_empty() {
        return Err("Engine produced an empty PDF".to_string());
    }

    std::fs::write(output_path, &pdf_bytes)
        .map_err(|e| format!("PDF write error: {}", e))?;

    browser.close().await.ok();
    Ok(())
}
