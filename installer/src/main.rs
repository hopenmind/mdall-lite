// MD -> ALL Self-Extracting Installer Stub
//
// Trailer format appended by make-installer.ps1:
//   [stub_exe_bytes][zip_payload_bytes][b"MD2ALLST" 8 bytes][zip_size u64 LE 8 bytes]
//
// On first run  : extracts payload ZIP alongside self, then launches mdall.exe
// On re-run     : mdall.exe already present → just launches it

#![windows_subsystem = "windows"]

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"MD2ALLST";
const TRAILER_SIZE: u64 = 16; // magic(8) + zip_size(8)

fn main() {
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => fatal(&format!("Cannot locate self: {}", e)),
    };

    let install_dir = exe_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let app_exe = install_dir.join("mdall.exe");

    if !app_exe.exists() {
        extract_payload(&exe_path, &install_dir);
    }

    launch(&app_exe);
}

fn extract_payload(exe_path: &Path, install_dir: &Path) {
    let mut file = match std::fs::File::open(exe_path) {
        Ok(f) => f,
        Err(e) => fatal(&format!("Cannot open installer: {}", e)),
    };

    let file_size = match file.seek(SeekFrom::End(0)) {
        Ok(s) => s,
        Err(e) => fatal(&format!("Seek error: {}", e)),
    };

    if file_size < TRAILER_SIZE {
        fatal("Installer has no embedded payload.");
    }

    // Read trailer
    file.seek(SeekFrom::End(-(TRAILER_SIZE as i64)))
        .unwrap_or_else(|e| fatal(&format!("Seek to trailer failed: {}", e)));

    let mut trailer = [0u8; 16];
    file.read_exact(&mut trailer)
        .unwrap_or_else(|e| fatal(&format!("Read trailer failed: {}", e)));

    if &trailer[0..8] != MAGIC {
        fatal("No payload found in installer binary.");
    }

    let zip_size = u64::from_le_bytes(trailer[8..16].try_into().unwrap());

    if zip_size == 0 || zip_size > file_size - TRAILER_SIZE {
        fatal("Corrupted payload size in installer.");
    }

    let zip_offset = file_size - TRAILER_SIZE - zip_size;
    file.seek(SeekFrom::Start(zip_offset))
        .unwrap_or_else(|e| fatal(&format!("Seek to payload failed: {}", e)));

    let mut zip_bytes = vec![0u8; zip_size as usize];
    file.read_exact(&mut zip_bytes)
        .unwrap_or_else(|e| fatal(&format!("Read payload failed: {}", e)));

    drop(file);

    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .unwrap_or_else(|e| fatal(&format!("Invalid ZIP payload: {}", e)));

    let engine_dir = engine_app_dir();

    let total = archive.len();
    for i in 0..total {
        let mut entry = archive
            .by_index(i)
            .unwrap_or_else(|e| fatal(&format!("ZIP entry {} error: {}", i, e)));

        let name = entry.name().to_string();
        // Route the rendering engine into a private per-user folder so it is never
        // placed next to the application; everything else installs in place.
        let out_path = match (name.strip_prefix("engine/"), &engine_dir) {
            (Some(rest), Some(dir)) if !rest.is_empty() => sanitize_path(dir, rest),
            _ => sanitize_path(install_dir, &name),
        };

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).ok();
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut out_file = std::fs::File::create(&out_path)
                .unwrap_or_else(|e| fatal(&format!("Cannot create {}: {}", out_path.display(), e)));
            std::io::copy(&mut entry, &mut out_file)
                .unwrap_or_else(|e| fatal(&format!("Extraction failed for {}: {}", out_path.display(), e)));
        }
    }

    // Hide the private engine folder from casual view.
    if let Some(dir) = &engine_dir {
        hide_dir(dir);
    }
}

/// The private per-user folder the rendering engine is extracted into.
fn engine_app_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("MD-ALL").join("engine"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support/MD-ALL/engine"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|p| p.join("md-all").join("engine"))
    }
}

#[cfg(target_os = "windows")]
fn hide_dir(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn SetFileAttributesW(path: *const u16, attrs: u32) -> i32;
    }
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN); }
}
#[cfg(not(target_os = "windows"))]
fn hide_dir(_dir: &Path) {}

// Prevent path traversal attacks in ZIP entries
fn sanitize_path(base: &Path, entry_name: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    for component in Path::new(entry_name).components() {
        use std::path::Component;
        match component {
            Component::Normal(c) => path.push(c),
            Component::CurDir => {}
            _ => {} // skip ParentDir (..) and absolute roots
        }
    }
    path
}

fn launch(app: &Path) {
    std::process::Command::new(app)
        .spawn()
        .unwrap_or_else(|e| fatal(&format!("Cannot launch {}: {}", app.display(), e)));
}

fn fatal(msg: &str) -> ! {
    // On Windows GUI subsystem, show a message box
    #[cfg(target_os = "windows")]
    unsafe {
        let title: Vec<u16> = "MD -> ALL Installer\0".encode_utf16().collect();
        let mut text: String = msg.to_string();
        text.push('\0');
        let text_w: Vec<u16> = text.encode_utf16().collect();
        windows_messagebox(text_w.as_ptr(), title.as_ptr());
    }
    #[cfg(not(target_os = "windows"))]
    eprintln!("Error: {}", msg);
    std::process::exit(1);
}

#[cfg(target_os = "windows")]
unsafe fn windows_messagebox(text: *const u16, caption: *const u16) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
    }
    MessageBoxW(std::ptr::null_mut(), text, caption, 0x10); // MB_ICONERROR
}
