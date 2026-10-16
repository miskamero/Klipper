use crate::{analysis::Analysis, config, media};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
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
    pub(crate) analysis: Arc<Analysis>,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            selected: Mutex::new(None),
            picking: AtomicBool::new(false),
            analysis: Arc::new(Analysis::default()),
        }
    }
}

impl RecordingState {
    pub(crate) fn begin_picker(&self) -> Result<(), String> {
        if self.picking.swap(true, Ordering::SeqCst) {
            Err("A file picker is already open.".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn end_picker(&self) {
        self.picking.store(false, Ordering::SeqCst);
    }
    pub(crate) fn authorize(
        &self,
        app: &tauri::AppHandle,
        path: PathBuf,
    ) -> Result<Recording, String> {
        let verified = validate_file(&path)?;
        if verified != path {
            return Err("The selected file's location changed. Select it again.".into());
        }
        let path = verified;
        app.asset_protocol_scope()
            .allow_file(&path)
            .map_err(|e| e.to_string())?;
        let mut selected = self
            .selected
            .lock()
            .map_err(|_| "Recording state unavailable.")?;
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
        self.analysis.supersede();
        Ok(recording)
    }
    #[cfg(windows)]
    pub(crate) fn with_selected<T>(
        &self,
        id: u64,
        action: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<T, String> {
        let selected = self
            .selected
            .lock()
            .map_err(|_| "Recording state unavailable")?;
        let recording = selected
            .as_ref()
            .filter(|recording| recording.id == id)
            .ok_or("Recording selection has changed")?;
        action(&recording.path)
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
    Ok(Some(state.authorize(&app, path)?))
}

#[tauri::command]
pub async fn analyze_recording(
    app: tauri::AppHandle,
    state: State<'_, RecordingState>,
    id: u64,
) -> Result<Option<media::MediaInfo>, String> {
    let response = {
        let selected = state
            .selected
            .lock()
            .map_err(|_| "Recording state unavailable.".to_string())?;
        let Some(recording) = selected.as_ref().filter(|value| value.id == id) else {
            // Delayed IPC for an older selection is a normal supersession, not a failure.
            return Ok(None);
        };
        // Selection validation and scheduling must be atomic relative to another selection.
        state
            .analysis
            .request(config::ffprobe(&app)?, recording.path.clone())
    };
    response
        .await
        .map_err(|_| "Analysis worker stopped unexpectedly.".to_string())?
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
