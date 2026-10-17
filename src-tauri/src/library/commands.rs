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
    navigation: super::navigation::Navigation,
}
#[derive(Serialize)]
pub struct Opened {
    recording: Recording,
    info: Option<MediaInfo>,
    metadata: Vec<super::providers::Imported>,
    display_title: Option<String>,
}

#[tauri::command]
pub async fn library_status(library: State<'_, LibraryState>) -> Result<Overview, String> {
    let library = library.get()?;
    let _operation = library.operation().await?;
    let (roots, navigation) = library
        .database(|db| Ok((db.roots()?, db.navigation()?)))
        .await?;
    Ok(Overview {
        roots,
        progress: library.progress(),
        navigation,
    })
}

#[tauri::command]
pub async fn library_query(library: State<'_, LibraryState>, query: Query) -> Result<Page, String> {
    let library = library.get()?;
    let _operation = library.operation().await?;
    library.database(move |db| db.query(query)).await
}

#[tauri::command]
pub async fn library_add_folder(
    app: tauri::AppHandle,
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
) -> Result<Option<Root>, String> {
    let library = library.get()?;
    let epoch = library.epoch();
    library.check_epoch(epoch)?;
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
    let _operation = library.operation().await?;
    library.check_epoch(epoch)?;
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
    let _operation = library.operation().await?;
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
    let _operation = library.operation().await?;
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
    let _operation = library.operation().await?;
    // Only IDs enter IPC. Recheck a selected folder or an exact-file grant confirmed on import.
    let (path, info, metadata, display_title) = library
        .database(move |db| {
            let (file, _root) = db.resolve_file(file_id)?;
            let title:Option<String>=db.connection.query_row("SELECT coalesce(o.title,p.title) FROM recordings f LEFT JOIN provider_display p ON p.file_id=f.id LEFT JOIN recording_overrides o ON o.file_id=f.id WHERE f.id=?1",[file_id],|r|r.get(0)).map_err(|e|e.to_string())?;
            Ok((
                PathBuf::from(&file.path),
                db.cached(file_id, &file)?,
                db.provider_metadata(file_id)?,
                title,
            ))
        })
        .await?;
    // Use exactly the same temporary selection IDs and asset grants as the native file picker.
    let selected_app = app.clone();
    let selected_path = path.clone();
    // Metadata validation above already ran off the UI thread. The final authorization rechecks it.
    let recording = recordings.authorize(
        &selected_app,
        selected_path,
        crate::commands::Context::Library,
    )?;
    Ok(Opened {
        recording,
        info,
        metadata,
        display_title,
    })
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

#[tauri::command]
pub async fn library_reset(
    window: tauri::WebviewWindow,
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
    confirmed: bool,
) -> Result<super::reset::ResetResult, String> {
    if !confirmed {
        return Err("Confirm Reset Library before continuing.".into());
    }
    let library = library.get()?;
    let analysis = recordings.analysis.clone();
    // The operation owns cleanup even if the caller/webview goes away.
    tauri::async_runtime::spawn(async move {
        library
            .reset(analysis, move || async move {
                window
                    .state::<RecordingState>()
                    .forget_library(&window)
                    .await
            })
            .await
    })
    .await
    .map_err(|_| "Library reset stopped unexpectedly")?
}
