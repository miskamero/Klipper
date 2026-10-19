//! Session-only FIFO. Jobs own source revisions and destinations independently of Editor/Player.
use super::{
    encoders::{self, Encoder, Support},
    export, recovery, Project, Status,
};
use crate::{
    commands::{Context, Recording, RecordingState},
    config,
    library::files,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, State};
use tokio::sync::{watch, OnceCell};

struct Spec {
    source: files::File,
    project: Project,
    destination: export::Destination,
    executable: PathBuf,
    probe: PathBuf,
}
struct Entry {
    id: u64,
    name: String,
    destination_key: String,
    spec: Option<Spec>,
    status: Arc<Mutex<Status>>,
    cancel: watch::Sender<bool>,
    output: Option<files::File>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    id: u64,
    name: String,
    #[serde(flatten)]
    status: Status,
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    revision: u64,
    jobs: Vec<Job>,
}
#[derive(Default)]
struct Inner {
    entries: Vec<Entry>,
    serial: u64,
    revision: u64,
    worker: bool,
    stopped: bool,
}
type Notify = Arc<dyn Fn(Snapshot) + Send + Sync>;
pub struct Queue {
    inner: Mutex<Inner>,
    idle: watch::Sender<bool>,
    stop: watch::Sender<bool>,
    support: OnceCell<Support>,
    probe: tokio::sync::Mutex<()>,
    notify: Mutex<Option<Notify>>,
}
pub struct ExportState(pub Arc<Queue>);
impl Default for ExportState {
    fn default() -> Self {
        Self(Arc::new(Queue {
            inner: Mutex::new(Inner::default()),
            idle: watch::channel(true).0,
            stop: watch::channel(false).0,
            support: OnceCell::new(),
            probe: tokio::sync::Mutex::new(()),
            notify: Mutex::new(None),
        }))
    }
}
fn unfinished(phase: &str) -> bool {
    matches!(phase, "queued" | "preparing" | "encoding" | "verifying")
}
fn key(destination: &export::Destination) -> String {
    // Canonical parent plus case-folded filename; parent identity also catches Windows aliases.
    let parent = destination.path.parent().unwrap();
    let parent = files::identity(parent).unwrap_or_else(|| parent.to_string_lossy().into_owned());
    let key = format!(
        "{parent}/{}",
        destination.path.file_name().unwrap().to_string_lossy()
    );
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}
impl Queue {
    pub fn connect(&self, app: tauri::AppHandle) {
        *self.notify.lock().unwrap() = Some(Arc::new(move |snapshot| {
            let _ = app.emit_to("main", "exports-changed", snapshot);
        }));
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut inner = self.inner.lock().unwrap();
        inner.revision += 1;
        Snapshot {
            revision: inner.revision,
            jobs: inner
                .entries
                .iter()
                .map(|entry| Job {
                    id: entry.id,
                    name: entry.name.clone(),
                    status: entry.status.lock().unwrap().clone(),
                })
                .collect(),
        }
    }
    fn emit(&self) {
        if let Some(notify) = self.notify.lock().unwrap().as_ref() {
            notify(self.snapshot());
        }
    }
    pub fn has_unfinished(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .entries
            .iter()
            .any(|entry| unfinished(&entry.status.lock().unwrap().phase))
    }
    pub fn is_idle(&self) -> bool {
        *self.idle.borrow() && self.probe.try_lock().is_ok()
    }
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.stopped = true;
        self.stop.send_replace(true);
        for entry in &mut inner.entries {
            entry.cancel.send_replace(true);
            let mut status = entry.status.lock().unwrap();
            if status.phase == "queued" {
                status.phase = "cancelled".into();
                entry.spec = None;
            }
        }
    }
    pub async fn shutdown(&self) {
        self.stop();
        let mut idle = self.idle.subscribe();
        let _ = idle.wait_for(|v| *v).await;
        let _probe = self.probe.lock().await;
    }
    async fn capabilities(&self, executable: &Path) -> Support {
        self.support
            .get_or_init(|| async {
                let _probe = self.probe.lock().await;
                encoders::detect(executable, &mut self.stop.subscribe()).await
            })
            .await
            .clone()
    }
    fn enqueue(self: &Arc<Self>, spec: Spec) -> Result<u64, String> {
        spec.destination.verify()?;
        let destination_key = key(&spec.destination);
        let mut inner = self.inner.lock().unwrap();
        if inner.stopped {
            return Err("Exports are shutting down".into());
        }
        if inner.entries.len() >= 64 {
            return Err(
                "Dismiss finished exports before adding more jobs (64-job session limit).".into(),
            );
        }
        if inner
            .entries
            .iter()
            .filter(|e| unfinished(&e.status.lock().unwrap().phase))
            .count()
            >= 16
        {
            return Err("The export queue is full (16 unfinished jobs).".into());
        }
        if inner.entries.iter().any(|e| {
            e.destination_key == destination_key && unfinished(&e.status.lock().unwrap().phase)
        }) {
            return Err("Another export already reserves this output filename.".into());
        }
        inner.serial += 1;
        let id = inner.serial;
        let status = Status {
            phase: "queued".into(),
            path: Some(spec.destination.path.clone()),
            duration: spec.project.end - spec.project.start,
            encoder: spec.project.output.encoder,
            ..Default::default()
        };
        inner.entries.push(Entry {
            id,
            name: spec.source.name.clone(),
            destination_key,
            spec: Some(spec),
            status: Arc::new(Mutex::new(status)),
            cancel: watch::channel(false).0,
            output: None,
        });
        let start = !inner.worker;
        if start {
            inner.worker = true;
            self.idle.send_replace(false);
        }
        drop(inner);
        self.emit();
        if start {
            let queue = self.clone();
            tauri::async_runtime::spawn(async move {
                queue.run().await;
            });
        }
        Ok(id)
    }
    async fn run(self: Arc<Self>) {
        loop {
            let next = {
                let mut inner = self.inner.lock().unwrap();
                let next = inner.entries.iter_mut().find_map(|e| {
                    e.spec.take().map(|spec| {
                        e.status.lock().unwrap().phase = "preparing".into();
                        (e.id, spec, e.status.clone(), e.cancel.subscribe())
                    })
                });
                if next.is_none() {
                    inner.worker = false;
                    self.idle.send_replace(true);
                }
                next
            };
            let Some((id, mut spec, status, mut cancel)) = next else {
                break;
            };
            self.emit();
            let requested = spec.project.output.encoder;
            let support = if requested != Encoder::Cpu {
                Some(self.capabilities(&spec.executable).await)
            } else {
                None
            };
            spec.project.output.encoder = match requested {
                Encoder::Auto if support.as_ref().is_some_and(|s| s.nvenc) => Encoder::Nvenc,
                Encoder::Auto => Encoder::Cpu,
                other => other,
            };
            status.lock().unwrap().encoder = spec.project.output.encoder;
            let result = {
                let execute = async {
                    if *cancel.borrow() {
                        return Err("Export cancelled".into());
                    }
                    if requested == Encoder::Nvenc && !support.as_ref().is_some_and(|s| s.nvenc) {
                        return Err(
                            "NVIDIA encoding is unavailable. Choose CPU or Automatic.".into()
                        );
                    }
                    if let Some(directory) = &spec.destination.recovery_directory {
                        recovery::recover(directory)?;
                    }
                    let mut result = export::run(
                        &spec.executable,
                        &spec.probe,
                        &spec.source,
                        spec.destination.clone(),
                        &spec.project,
                        &status,
                        &mut cancel,
                    )
                    .await;
                    if requested == Encoder::Auto
                        && spec.project.output.encoder == Encoder::Nvenc
                        && !*cancel.borrow()
                        && result
                            .as_ref()
                            .is_err_and(|e| e.starts_with("FFmpeg export failed"))
                    {
                        spec.project.output.encoder = Encoder::Cpu;
                        {
                            let mut progress = status.lock().unwrap();
                            progress.encoder = Encoder::Cpu;
                            progress.phase = "preparing".into();
                            progress.seconds = 0.0;
                            progress.eta = None;
                            progress.notice =
                                "NVIDIA export failed; retrying with CPU encoding.".into();
                        }
                        result = export::run(
                            &spec.executable,
                            &spec.probe,
                            &spec.source,
                            spec.destination.clone(),
                            &spec.project,
                            &status,
                            &mut cancel,
                        )
                        .await;
                    }
                    result
                };
                tokio::pin!(execute);
                loop {
                    tokio::select! { result = &mut execute => break result, _ = tokio::time::sleep(Duration::from_millis(250)) => self.emit() }
                }
            };
            let output = if result.is_ok() {
                files::describe(&spec.destination.path).ok()
            } else {
                None
            };
            {
                let mut inner = self.inner.lock().unwrap();
                let entry = inner.entries.iter_mut().find(|e| e.id == id).unwrap();
                entry.output = output;
                let mut progress = status.lock().unwrap();
                progress.phase = if result.is_ok() {
                    "completed"
                } else if *entry.cancel.borrow() {
                    "cancelled"
                } else {
                    "error"
                }
                .into();
                progress.error = if *entry.cancel.borrow() && result.is_err() {
                    String::new()
                } else {
                    result.err().unwrap_or_default()
                };
                progress.eta = None;
            }
            self.emit();
        }
    }
    pub fn cancel(&self, id: u64) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or("Export job not found")?;
        entry.cancel.send_replace(true);
        if entry.status.lock().unwrap().phase == "queued" {
            entry.spec = None;
            entry.status.lock().unwrap().phase = "cancelled".into();
        }
        drop(inner);
        self.emit();
        Ok(())
    }
    pub fn dismiss(&self, id: u64) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        if inner
            .entries
            .iter()
            .any(|e| e.id == id && unfinished(&e.status.lock().unwrap().phase))
        {
            return Err("Cancel this export before dismissing it".into());
        }
        inner.entries.retain(|e| e.id != id);
        drop(inner);
        self.emit();
        Ok(())
    }
}

#[tauri::command]
pub async fn export_encoders(
    app: tauri::AppHandle,
    state: State<'_, ExportState>,
) -> Result<Support, String> {
    Ok(state.0.capabilities(&config::ffmpeg(&app)?).await)
}
#[tauri::command]
pub fn exports_status(state: State<'_, ExportState>) -> Snapshot {
    state.0.snapshot()
}
#[tauri::command]
pub fn exports_cancel(state: State<'_, ExportState>, id: u64) -> Result<(), String> {
    state.0.cancel(id)
}
#[tauri::command]
pub fn exports_dismiss(state: State<'_, ExportState>, id: u64) -> Result<(), String> {
    state.0.dismiss(id)
}
#[tauri::command]
pub fn exports_close(app: tauri::AppHandle) {
    app.exit(0);
}
#[tauri::command]
pub fn exports_open(
    app: tauri::AppHandle,
    state: State<'_, ExportState>,
    recordings: State<'_, RecordingState>,
    id: u64,
) -> Result<Recording, String> {
    let output = {
        let inner = state.0.inner.lock().unwrap();
        inner
            .entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.output.clone())
            .ok_or("No completed output is available")?
    };
    if files::describe(Path::new(&output.path))? != output {
        return Err("The exported file changed or is unavailable".into());
    }
    recordings.authorize(&app, output.path.into(), Context::Standalone)
}

pub(super) fn submit(
    queue: &Arc<Queue>,
    source: files::File,
    project: Project,
    destination: export::Destination,
    executable: PathBuf,
    probe: PathBuf,
) -> Result<u64, String> {
    queue.enqueue(Spec {
        source,
        project,
        destination,
        executable,
        probe,
    })
}

#[cfg(test)]
#[path = "../../tests/editor/queue.rs"]
mod tests;
