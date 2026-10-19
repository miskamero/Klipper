mod encoders;
mod export;
mod media;
pub mod queue;
mod recovery;
use crate::{
    commands::RecordingState,
    config,
    library::{files, thumbnails::cache::Cache},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::watch;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    output: Output,
    version: u32,
    token: u64,
    recording_id: u64,
    revision: String,
    start: f64,
    end: f64,
    info: media::Info,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    container: &'static str,
    video_codec: &'static str,
    audio_codec: &'static str,
    crf: u8,
    encoder: encoders::Encoder,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            container: "mp4",
            video_codec: "h264",
            audio_codec: "aac",
            crf: 18,
            encoder: encoders::Encoder::Auto,
        }
    }
}
struct Session {
    token: u64,
    source: files::File,
    project: Option<Project>,
    cancel: watch::Sender<bool>,
    destination: Option<export::Destination>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[cfg(test)]
    #[serde(skip)]
    process_id: Option<u32>,
    phase: String,
    seconds: f64,
    duration: f64,
    eta: Option<f64>,
    path: Option<PathBuf>,
    error: String,
    encoder: encoders::Encoder,
    notice: String,
}
pub struct Editor {
    session: Mutex<Option<Session>>,
    serial: AtomicU64,
    preview: Arc<tokio::sync::Mutex<()>>,
    picking: AtomicBool,
    stopped: AtomicBool,
}
pub struct EditorState(pub Arc<Editor>);
impl Default for EditorState {
    fn default() -> Self {
        Self(Arc::new(Editor {
            session: Mutex::new(None),
            serial: AtomicU64::new(0),
            preview: Arc::new(tokio::sync::Mutex::new(())),
            picking: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        }))
    }
}
impl Editor {
    pub fn has_session(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }
    pub fn stop(&self) {
        let session = self.session.lock().unwrap();
        self.stopped.store(true, Ordering::Release);
        if let Some(s) = session.as_ref() {
            s.cancel.send_replace(true);
        }
    }
    pub fn is_idle(&self) -> bool {
        self.preview.try_lock().is_ok()
    }
    pub async fn shutdown(&self) {
        self.stop();
        let _permit = self.preview.lock().await;
    }
    fn session<T>(
        &self,
        token: u64,
        run: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.stopped.load(Ordering::Acquire) {
            return Err("Editor is shutting down".into());
        }
        let mut state = self.session.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) {
            return Err("Editor is shutting down".into());
        }
        let session = state
            .as_mut()
            .filter(|s| s.token == token && !*s.cancel.borrow())
            .ok_or("Editor session has ended")?;
        run(session)
    }
}

#[tauri::command]
pub async fn editor_begin(
    app: tauri::AppHandle,
    recordings: State<'_, RecordingState>,
    state: State<'_, EditorState>,
    id: u64,
) -> Result<Project, String> {
    let path = recordings.with_selected(id, |p| Ok(p.to_path_buf()))?;
    let editor = state.0.clone();
    let permit = editor
        .preview
        .clone()
        .try_lock_owned()
        .map_err(|_| "Another editor operation is finishing")?;
    if editor.has_session() {
        return Err("Close the existing editor first".into());
    }
    let exe = config::ffprobe(&app)?;
    let source = tauri::async_runtime::spawn_blocking(move || files::describe(&path))
        .await
        .map_err(|_| "Source inspection stopped")??;
    let token = editor.serial.fetch_add(1, Ordering::AcqRel) + 1;
    let (cancel, mut cancelled) = watch::channel(false);
    {
        let mut slot = editor.session.lock().unwrap();
        if slot.is_some() || editor.stopped.load(Ordering::Acquire) {
            return Err("Close the existing editor first".into());
        }
        *slot = Some(Session {
            token,
            source: source.clone(),
            project: None,
            cancel,
            destination: None,
        });
    }
    // The task owns cancellation/reaping even if an IPC caller disappears.
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        let result = media::inspect(&exe, Path::new(&source.path), &mut cancelled).await;
        match result {
            Ok(info) => {
                let project = Project {
                    output: Output::default(),
                    version: 1,
                    token,
                    recording_id: id,
                    revision: serde_json::to_string(&(
                        &source.identity,
                        source.size,
                        &source.modified_ns,
                    ))
                    .map_err(|e| e.to_string())?,
                    start: 0.0,
                    end: info.duration,
                    info,
                };
                editor.session(token, |s| {
                    s.project = Some(project.clone());
                    Ok(())
                })?;
                Ok(project)
            }
            Err(error) => {
                let mut slot = editor.session.lock().unwrap();
                if slot.as_ref().is_some_and(|s| s.token == token) {
                    *slot = None;
                }
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| "Editor inspection stopped")?
}

#[tauri::command]
pub async fn editor_close(
    state: State<'_, EditorState>,
    token: u64,
    cancel: Option<bool>,
) -> Result<(), String> {
    let editor = &state.0;
    editor.session(token, |s| {
        if !cancel.unwrap_or(false) && editor.picking.load(Ordering::Acquire) {
            return Err("Finish or cancel the export before closing the editor".into());
        }
        s.cancel.send_replace(true);
        Ok(())
    })?;
    let _permit = editor.preview.lock().await;
    let mut slot = editor.session.lock().unwrap();
    if slot.as_ref().is_some_and(|s| s.token == token) {
        *slot = None;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct Range {
    start: f64,
    end: f64,
}
fn range(project: &Project, range: &Range) -> Result<(), String> {
    if !range.start.is_finite()
        || !range.end.is_finite()
        || range.start < 0.0
        || range.end > project.info.duration + 0.000001
        || range.end - range.start < 0.001
    {
        return Err("Choose a nonempty trim range inside the recording.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn editor_range(
    app: tauri::AppHandle,
    state: State<'_, EditorState>,
    token: u64,
    selection: Range,
) -> Result<Project, String> {
    let editor = state.0.clone();
    let permit = editor
        .preview
        .clone()
        .try_lock_owned()
        .map_err(|_| "Editor is busy; try again")?;
    let (mut project, source, mut cancel) = editor.session(token, |s| {
        Ok((
            s.project.clone().ok_or("Editor is preparing")?,
            s.source.clone(),
            s.cancel.subscribe(),
        ))
    })?;
    range(&project, &selection)?;
    let exe = config::ffprobe(&app)?;
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        if files::describe(Path::new(&source.path))? != source {
            return Err("Source changed. Reopen the editor.".into());
        }
        project.start = media::boundary(
            &exe,
            Path::new(&source.path),
            &project.info,
            selection.start,
            &mut cancel,
        )
        .await?;
        project.end = media::boundary(
            &exe,
            Path::new(&source.path),
            &project.info,
            selection.end,
            &mut cancel,
        )
        .await?;
        range(
            &project,
            &Range {
                start: project.start,
                end: project.end,
            },
        )?;
        editor.session(token, |s| {
            s.project = Some(project.clone());
            Ok(project)
        })
    })
    .await
    .map_err(|_| "Frame timing inspection stopped")?
}

#[tauri::command]
pub async fn editor_frame(
    app: tauri::AppHandle,
    state: State<'_, EditorState>,
    recordings: State<'_, RecordingState>,
    token: u64,
    seconds: f64,
) -> Result<tauri::ipc::Response, String> {
    let editor = state.0.clone();
    let permit = editor
        .preview
        .clone()
        .try_lock_owned()
        .map_err(|_| "Editor is busy")?;
    let (project, source, mut cancel) = editor.session(token, |s| {
        Ok((
            s.project.clone().ok_or("Editor is preparing")?,
            s.source.clone(),
            s.cancel.subscribe(),
        ))
    })?;
    if files::describe(Path::new(&source.path))? != source {
        return Err("Storyboard source changed; reopen the editor".into());
    }
    if !seconds.is_finite() || seconds < 0.0 || seconds > project.info.duration {
        return Err("Invalid storyboard time".into());
    }
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("editor-storyboards");
    let key = serde_json::to_string(&(
        "storyboard-v1",
        &source.path,
        &project.revision,
        format!("{seconds:.3}"),
    ))
    .map_err(|e| e.to_string())?;
    let lookup = key.clone();
    let directory_copy = directory.clone();
    let cached = tauri::async_runtime::spawn_blocking(move || {
        Cache::with_limits(&directory_copy, 16 * 1024 * 1024, 512)?.get(&lookup)
    })
    .await
    .map_err(|_| "Storyboard cache stopped")??;
    if let Some(bytes) = cached {
        return Ok(tauri::ipc::Response::new(bytes));
    }
    let analysis = recordings.analysis.clone();
    let exe = config::ffmpeg(&app)?;
    tauri::async_runtime::spawn(async move {
        let _permit=permit;
        let request=format!("editor-{token}");
        let response=loop {
            if *cancel.borrow() {return Err("Storyboard cancelled".to_string());}
            if let Some(response)=analysis.try_thumbnail(exe.clone(),PathBuf::from(&source.path),seconds,request.clone()) {break response;}
            tokio::select! {_=cancel.wait_for(|v|*v)=>return Err("Storyboard cancelled".into()),_=tokio::time::sleep(std::time::Duration::from_millis(100))=>{}}
        };
        tokio::pin!(response);
        let bytes=tokio::select! {
            value=&mut response=>value,
            _=async { let _ = cancel.wait_for(|v|*v).await; }=>{analysis.cancel_thumbnail(&request);response.await}
        }.map_err(|_|"Storyboard worker stopped")??.unwrap_or_default();
        if *cancel.borrow() || bytes.is_empty() {return Ok(tauri::ipc::Response::new(Vec::<u8>::new()));}
        if files::describe(Path::new(&source.path))?!=source {return Err("Storyboard source changed".into());}
        tauri::async_runtime::spawn_blocking(move||Cache::with_limits(&directory,16*1024*1024,512)?.put(&key,&bytes).map(|_|bytes)).await.map_err(|_|"Storyboard cache stopped")?.map(tauri::ipc::Response::new)
    }).await.map_err(|_|"Storyboard stopped")?
}

#[tauri::command]
pub async fn editor_destination(
    app: tauri::AppHandle,
    state: State<'_, EditorState>,
    token: u64,
) -> Result<Option<PathBuf>, String> {
    let editor = state.0.clone();
    let name = editor.session(token, |s| {
        Ok(format!(
            "{}-trim.mp4",
            Path::new(&s.source.name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
        ))
    })?;
    if editor.picking.swap(true, Ordering::AcqRel) {
        return Err("An export or picker is already active".into());
    }
    let dialog_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Export a new H.264 recording")
            .add_filter("H.264 MP4", &["mp4"])
            .set_file_name(name)
            .blocking_save_file()
    })
    .await;
    editor.picking.store(false, Ordering::Release);
    let Some(file) = result.map_err(|_| "Save dialog failed")? else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|_| "Choose a local output path")?;
    let mut destination = export::Destination::new(&path)?;
    destination.recovery_directory = Some(
        app.path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("editor-exports"),
    );
    let path = destination.path.clone();
    editor.session(token, |s| {
        s.destination = Some(destination);
        Ok(())
    })?;
    Ok(Some(path))
}

#[tauri::command]
pub fn editor_export(
    app: tauri::AppHandle,
    state: State<'_, EditorState>,
    exports: State<'_, queue::ExportState>,
    token: u64,
    encoder: Option<encoders::Encoder>,
) -> Result<u64, String> {
    let exe = config::ffmpeg(&app)?;
    let probe = config::ffprobe(&app)?;
    state.0.session(token, |session| {
        if state.0.picking.load(Ordering::Acquire) {
            return Err("Wait for the Save dialog to close".into());
        }
        let mut project = session.project.clone().ok_or("Editor is preparing")?;
        project.output.encoder = encoder.unwrap_or_default();
        let destination = session
            .destination
            .as_ref()
            .ok_or("Choose an output destination first")?
            .clone();
        let id = queue::submit(
            &exports.0,
            session.source.clone(),
            project,
            destination,
            exe,
            probe,
        )?;
        session.destination = None;
        Ok(id)
    })
}
#[cfg(test)]
#[path = "../../tests/editor/mod.rs"]
mod tests;
