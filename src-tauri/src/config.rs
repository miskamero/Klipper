use std::path::PathBuf;
use tauri::Manager;

// Portable releases can place the vetted binary under resources/bin.
// Development uses PATH. No arbitrary executable path comes from the frontend.
pub fn ffprobe(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("bin")
        .join(name);
    if bundled.is_file() {
        return Ok(bundled);
    }
    Ok(PathBuf::from(name))
}
