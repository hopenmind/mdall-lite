fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        // The installer exe carries the app logo (favicon lives at the repo root
        // under assets/; the build script's cwd is the installer/ package dir).
        res.set_icon("../assets/favicon.ico");
        res.set("ProductName", "MD -> ALL");
        res.set("FileDescription", "MD -> ALL Installer");
        res.set("LegalCopyright", "Hope n Mind SASU");
        res.set("ProductVersion", "3.0.3");
        let _ = res.compile();
    }
}
