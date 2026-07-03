fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/favicon.ico");
        res.set("ProductName", "MD -> ALL");
        res.set("FileDescription", "Markdown Editor with KaTeX and PDF Export");
        res.set("ProductVersion", "3.0.3");
        if let Err(e) = res.compile() {
            eprintln!("winresource warning: {}", e);
        }
    }

    // Warn at build time if the bundled rendering engine is absent. PDF export
    // still works via the Typst / genpdf fallbacks, but the highest-quality tier
    // (CDP) needs the engine binary fetched into the local runtime folder.
    // Run scripts/setup-engine.ps1 to download and install it.
    let engine_candidate = std::path::Path::new("chromium").join("chrome.exe");
    if !engine_candidate.exists() {
        println!(
            "cargo:warning=Bundled rendering engine not found. High-quality PDF \
             export (tier 1) will be unavailable at runtime. \
             Run: .\\scripts\\setup-engine.ps1"
        );
    }
}
