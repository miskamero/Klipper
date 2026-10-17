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
    selected: Mutex<Selections>,
    picking: AtomicBool,
    pub(crate) analysis: Arc<Analysis>,
}

#[derive(Clone, Copy)]
pub(crate) enum Context {
    Standalone,
    Library,
}

#[derive(Default)]
struct Selections {
    active: Option<Recording>,
    standalone: Option<Recording>,
    library: Option<Recording>,
    next_id: u64,
}
impl Selections {
    fn remembered(&self, id: u64) -> Option<(Context, &Recording)> {
        self.standalone
            .as_ref()
            .filter(|r| r.id == id)
            .map(|r| (Context::Standalone, r))
            .or_else(|| {
                self.library
                    .as_ref()
                    .filter(|r| r.id == id)
                    .map(|r| (Context::Library, r))
            })
    }
    fn insert(&mut self, path: PathBuf, context: Context) -> Recording {
        // Activation gets a new ID: the native worker can continue rejecting delayed older loads.
        self.next_id += 1;
        let recording = Recording {
            id: self.next_id,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path,
        };
        match context {
            Context::Standalone => self.standalone = Some(recording.clone()),
            Context::Library => self.library = Some(recording.clone()),
        }
        self.active = Some(recording.clone());
        recording
    }
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            selected: Mutex::default(),
            picking: AtomicBool::new(false),
            analysis: Arc::new(Analysis::default()),
        }
    }
}

impl RecordingState {
    pub(crate) async fn forget_library(&self, window: &tauri::WebviewWindow) -> Result<(), String> {
        let active = {
            let selected = self
                .selected
                .lock()
                .map_err(|_| "Recording state unavailable")?;
            selected
                .library
                .as_ref()
                .filter(|r| selected.active.as_ref().is_some_and(|a| a.id == r.id))
                .map(|r| r.id)
        };
        #[cfg(windows)]
        if let Some(id) = active {
            crate::native_player::deactivate(window, id).await?;
        }
        #[cfg(not(windows))]
        let _ = window;
        let mut selected = self
            .selected
            .lock()
            .map_err(|_| "Recording state unavailable")?;
        if active.is_some_and(|id| selected.active.as_ref().is_some_and(|r| r.id == id)) {
            selected.active = None;
            self.analysis.supersede();
        }
        selected.library = None;
        Ok(())
    }
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
        context: Context,
    ) -> Result<Recording, String> {
        self.authorize_remembered(app, path, context, None)
    }
    fn authorize_remembered(
        &self,
        app: &tauri::AppHandle,
        path: PathBuf,
        context: Context,
        expected_id: Option<u64>,
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
        if expected_id.is_some_and(|id| selected.remembered(id).is_none()) {
            return Err("Recording selection has changed".into());
        }
        let recording = selected.insert(path, context);
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
            .active
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
    Ok(Some(state.authorize(&app, path, Context::Standalone)?))
}

// Only the two in-session selections are retained. No paths enter IPC and no history is persisted.
#[tauri::command]
pub async fn activate_recording(
    app: tauri::AppHandle,
    state: State<'_, RecordingState>,
    id: u64,
) -> Result<Recording, String> {
    let (context, path) = {
        let selections = state
            .selected
            .lock()
            .map_err(|_| "Recording state unavailable")?;
        let (context, recording) = selections
            .remembered(id)
            .ok_or("Recording selection has changed")?;
        (context, recording.path.clone())
    };
    // Recheck existence/local-file restrictions; a remembered ID is not a permanent filesystem grant.
    let path = tauri::async_runtime::spawn_blocking(move || {
        let verified = validate_file(&path)?;
        if verified != path {
            return Err("The selected file's location changed. Select it again.".to_string());
        }
        Ok(verified)
    })
    .await
    .map_err(|_| "File validation failed")??;
    state.authorize_remembered(&app, path, context, Some(id))
}

#[tauri::command]
pub async fn deactivate_recording(
    window: tauri::WebviewWindow,
    state: State<'_, RecordingState>,
    id: u64,
) -> Result<(), String> {
    #[cfg(windows)]
    crate::native_player::deactivate(&window, id).await?;
    #[cfg(not(windows))]
    let _ = window;
    let mut selections = state
        .selected
        .lock()
        .map_err(|_| "Recording state unavailable")?;
    if selections.active.as_ref().is_some_and(|r| r.id == id) {
        selections.active = None;
        state.analysis.supersede();
    }
    Ok(())
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
        let Some(recording) = selected.active.as_ref().filter(|value| value.id == id) else {
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

    #[test]
    fn sessions_retain_only_current_context_selections_and_issue_monotonic_activation_ids() {
        let mut selections = Selections::default();
        let one = selections.insert(PathBuf::from("one-off.mp4"), Context::Standalone);
        let two = selections.insert(PathBuf::from("indexed.mp4"), Context::Library);
        assert_eq!(selections.active.as_ref().unwrap().id, two.id);
        let (context, remembered) = selections.remembered(one.id).unwrap();
        let path = remembered.path.clone();
        let restored = selections.insert(path, context);
        assert!(restored.id > two.id);
        assert!(selections.remembered(one.id).is_none());
        assert!(selections.remembered(two.id).is_some());
        assert!(selections.remembered(999).is_none());
        selections.active = None;
        assert!(selections.remembered(restored.id).is_some());
        selections.insert(PathBuf::from("replacement.mp4"), Context::Standalone);
        assert!(selections.remembered(restored.id).is_none());
        assert!(selections.remembered(two.id).is_some());
    }

    #[cfg(windows)]
    #[test]
    fn remembered_inactive_recordings_are_not_authorized_for_native_playback() {
        let state = RecordingState::default();
        let first = state
            .selected
            .lock()
            .unwrap()
            .insert(PathBuf::from("one-off.mp4"), Context::Standalone);
        assert!(state.with_selected(first.id, |_| Ok(())).is_ok());
        let other = state
            .selected
            .lock()
            .unwrap()
            .insert(PathBuf::from("indexed.mp4"), Context::Library);
        assert!(state.with_selected(first.id, |_| Ok(())).is_err());
        assert!(state.with_selected(other.id, |_| Ok(())).is_ok());
        state.selected.lock().unwrap().active = None;
        assert!(state.with_selected(other.id, |_| Ok(())).is_err());
    }
}
