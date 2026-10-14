use crate::{config, media};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Serialize)]
pub struct Recording {
    id: u64,
    name: String,
    path: PathBuf,
}

pub struct RecordingState {
    selected: Mutex<Option<Recording>>,
    picking: AtomicBool,
    analysis: tokio::sync::Semaphore,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            selected: Mutex::new(None),
            picking: AtomicBool::new(false),
            analysis: tokio::sync::Semaphore::new(1),
        }
    }
}

fn validate_file(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|_| "The selected file is unavailable.".to_string())?;
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !path.is_file() || !["mp4", "m4v", "mov", "mkv", "webm"].contains(&extension.as_str()) {
        return Err("Choose a local MP4, M4V, MOV, MKV or WebM recording.".into());
    }
    // Network shares would defeat the offline local-file boundary.
    let text = path.to_string_lossy();
    if text.starts_with(r"\\?\UNC\") || (text.starts_with(r"\\") && !text.starts_with(r"\\?\")) {
        return Err("Choose a recording on a local disk, rather than a network share.".into());
    }
    Ok(path)
}

#[tauri::command]
pub async fn select_recording(
    app: tauri::AppHandle,
    state: State<'_, RecordingState>,
) -> Result<Option<Recording>, String> {
    if state.picking.swap(true, Ordering::SeqCst) {
        return Err("A file picker is already open.".into());
    }
    let picker_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        picker_app
            .dialog()
            .file()
            .set_title("Open a recording")
            .add_filter("Video recordings", &["mp4", "m4v", "mov", "mkv", "webm"])
            .blocking_pick_file()
    })
    .await;
    state.picking.store(false, Ordering::SeqCst);
    let Some(file) = picked.map_err(|_| "The file picker could not open.".to_string())? else {
        return Ok(None);
    };
    let path = file
        .into_path()
        .map_err(|_| "Select a local file.".to_string())?;
    let path = tauri::async_runtime::spawn_blocking(move || validate_file(&path))
        .await
        .map_err(|_| "File validation failed.".to_string())??;
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| e.to_string())?;
    let mut selected = state
        .selected
        .lock()
        .map_err(|_| "Recording state unavailable.".to_string())?;
    let recording = Recording {
        id: selected.as_ref().map_or(1, |previous| previous.id + 1),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path,
    };
    *selected = Some(recording.clone());
    Ok(Some(recording))
}

#[tauri::command]
pub async fn analyze_recording(
    app: tauri::AppHandle,
    state: State<'_, RecordingState>,
    id: u64,
) -> Result<media::MediaInfo, String> {
    // Fail fast instead of allowing an unbounded queue of probe processes.
    let _permit = state.analysis.try_acquire().map_err(|_| {
        "Another recording is still being analyzed. Retry analysis in a moment.".to_string()
    })?;
    let path = {
        let selected = state
            .selected
            .lock()
            .map_err(|_| "Recording state unavailable.".to_string())?;
        selected
            .as_ref()
            .filter(|value| value.id == id)
            .map(|value| value.path.clone())
            .ok_or_else(|| "Select a recording before analyzing it.".to_string())?
    };
    media::probe(&config::ffprobe(&app)?, &path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_requires_existing_video_file() {
        let directory = tempfile::tempdir().unwrap();
        assert!(validate_file(directory.path()).is_err());
        assert!(validate_file(&directory.path().join("missing.mp4")).is_err());
        let file = directory.path().join("synthetic.txt");
        std::fs::write(&file, b"fixture").unwrap();
        assert!(validate_file(&file).is_err());
        let file = directory.path().join("synthetic [test] & unicode-ä.MP4");
        std::fs::write(&file, b"fixture").unwrap();
        assert!(validate_file(&file).is_ok());
    }
}
