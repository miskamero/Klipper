use std::path::PathBuf;
use tauri::Manager;

// Portable releases can place the vetted binary under resources/bin.
// Development uses PATH. No arbitrary executable path comes from the frontend.
pub fn ffprobe(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    helper(app, "ffprobe")
}

pub fn ffmpeg(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    helper(app, "ffmpeg")
}

fn helper(app: &tauri::AppHandle, tool: &str) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.into()
    };
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("bin")
        .join(&name);
    if bundled.is_file() {
        return Ok(bundled);
    }
    Ok(PathBuf::from(name))
}
