use super::{
    files,
    query::{Page, Query},
    LibraryState, Progress, Root,
};
use crate::{
    commands::{Recording, RecordingState},
    config,
    media::MediaInfo,
};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct Overview {
    roots: Vec<Root>,
    progress: Progress,
}
#[derive(Serialize)]
pub struct Opened {
    recording: Recording,
    info: Option<MediaInfo>,
}

#[tauri::command]
pub async fn library_status(library: State<'_, LibraryState>) -> Result<Overview, String> {
    let library = library.get()?;
    let roots = library.database(|db| db.roots()).await?;
    Ok(Overview {
        roots,
        progress: library.progress(),
    })
}

#[tauri::command]
pub async fn library_query(library: State<'_, LibraryState>, query: Query) -> Result<Page, String> {
    library.get()?.database(move |db| db.query(query)).await
}

#[tauri::command]
pub async fn library_add_folder(
    app: tauri::AppHandle,
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
) -> Result<Option<Root>, String> {
    let library = library.get()?;
    recordings.begin_picker()?;
    let dialog = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog
            .dialog()
            .file()
            .set_title("Add a local recording folder")
            .blocking_pick_folder()
    })
    .await;
    recordings.end_picker();
    let Some(folder) = picked.map_err(|_| "Folder picker failed")? else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|_| "Choose a local folder")?;
    let root = library
        .database(move |db| db.add_root(&files::local_directory(&path)?))
        .await?;
    // If another scan is active, retain the new root for its explicit Rescan action.
    if library.is_idle() {
        library
            .start(root.id, config::ffprobe(&app)?, recordings.analysis.clone())
            .await?;
    }
    Ok(Some(root))
}

#[tauri::command]
pub async fn library_rescan(
    app: tauri::AppHandle,
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
    root_id: i64,
) -> Result<(), String> {
    let library = library.get()?;
    library.database(move |db| db.root(root_id)).await?;
    library
        .start(root_id, config::ffprobe(&app)?, recordings.analysis.clone())
        .await
}

#[tauri::command]
pub async fn library_cancel(
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
) -> Result<(), String> {
    library.get()?.cancel(&recordings.analysis);
    Ok(())
}

#[tauri::command]
pub async fn library_remove_folder(
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
    root_id: i64,
) -> Result<(), String> {
    let library = library.get()?;
    if !library.is_idle() && library.progress().root_id == Some(root_id) {
        library.cancel(&recordings.analysis);
        let mut idle = library.idle.subscribe();
        let _ = idle.wait_for(|value| *value).await;
    }
    library.database(move |db| db.remove_root(root_id)).await
}

#[tauri::command]
pub async fn library_open(
    app: tauri::AppHandle,
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
    file_id: i64,
) -> Result<Opened, String> {
    let library = library.get()?;
    // Only IDs enter IPC. Paths come from SQLite and are reauthorized against a selected root.
    let (path, info) = library
        .database(move |db| {
            let (file, _root) = db.resolve_file(file_id)?;
            Ok((PathBuf::from(&file.path), db.cached(file_id, &file)?))
        })
        .await?;
    // Use exactly the same temporary selection IDs and asset grants as the native file picker.
    let selected_app = app.clone();
    let selected_path = path.clone();
    // Metadata validation above already ran off the UI thread. The final authorization rechecks it.
    let recording = recordings.authorize(&selected_app, selected_path)?;
    Ok(Opened { recording, info })
}

pub fn initialize(app: &tauri::AppHandle) -> Result<std::sync::Arc<super::Library>, String> {
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("thumbnails");
    super::Library::open(&directory.join("library.sqlite3"), &cache)
}
