pub(crate) mod parse;
pub(crate) mod paths;
mod store;

use crate::{commands::RecordingState, library::LibraryState};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::watch;

const MAX_CATALOG: u64 = 64 * 1024 * 1024;
struct Plan {
    path: String,
    source: String,
    revision: String,
    prepared: Vec<store::Prepared>,
    counts: store::Counts,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    phase: String,
    done: usize,
    total: usize,
    linked: usize,
    error: String,
}
struct Work {
    token: u64,
    plan: Option<Arc<Plan>>,
    progress: Progress,
}
pub struct ImportState {
    work: Mutex<Work>,
    cancel: AtomicBool,
    stopped: AtomicBool,
    done: AtomicUsize,
    idle: watch::Sender<bool>,
}
impl Default for ImportState {
    fn default() -> Self {
        Self {
            work: Mutex::new(Work {
                token: 0,
                plan: None,
                progress: Progress::default(),
            }),
            cancel: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            done: AtomicUsize::new(0),
            idle: watch::channel(true).0,
        }
    }
}
impl ImportState {
    pub(super) fn resume(&self) {
        let mut work = self.work.lock().unwrap();
        work.progress = Progress::default();
        work.token += 1;
        self.done.store(0, Ordering::Release);
        self.stopped.store(false, Ordering::Release);
    }
    pub fn is_idle(&self) -> bool {
        *self.idle.borrow()
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.cancel.store(true, Ordering::Release);
        self.work.lock().unwrap().plan = None;
    }
    pub async fn shutdown(&self) {
        self.stop();
        let mut idle = self.idle.subscribe();
        let _ = idle.wait_for(|v| *v).await;
    }
    fn begin(&self, phase: &str) -> Result<(), String> {
        let mut work = self.work.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) || !self.is_idle() {
            return Err("Finish or cancel the current Medal operation first.".into());
        }
        self.cancel.store(false, Ordering::Release);
        self.done.store(0, Ordering::Release);
        if phase == "preparing" {
            work.plan = None;
        }
        work.progress = Progress {
            phase: phase.into(),
            ..Default::default()
        };
        self.idle.send_replace(false);
        Ok(())
    }
    fn finish(&self, result: &Result<usize, String>) {
        let mut work = self.work.lock().unwrap();
        work.progress.done = self.done.load(Ordering::Acquire);
        match result {
            Ok(linked) => {
                work.progress.phase = "complete".into();
                work.progress.linked = *linked;
            }
            Err(message) => {
                work.progress.phase = if self.cancel.load(Ordering::Acquire) {
                    "cancelled".into()
                } else {
                    "failed".into()
                };
                work.progress.error = message.clone();
            }
        }
        self.idle.send_replace(true);
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    token: u64,
    catalog_entries: usize,
    total: usize,
    counts: store::Counts,
    rows: Vec<store::Row>,
    offset: usize,
}
fn preview(state: &ImportState, token: u64, offset: usize) -> Result<Preview, String> {
    let work = state.work.lock().unwrap();
    if token != work.token {
        return Err("This import preview is no longer current.".into());
    }
    let plan = work.plan.as_ref().ok_or("No Medal import is prepared.")?;
    let total = plan.prepared.iter().map(|p| p.references.len()).sum();
    if offset > total {
        return Err("Invalid preview page.".into());
    }
    Ok(Preview {
        token,
        catalog_entries: plan.prepared.len(),
        total,
        counts: plan.counts.clone(),
        offset,
        rows: plan
            .prepared
            .iter()
            .flat_map(|p| p.references.iter().map(|r| r.row.clone()))
            .skip(offset)
            .take(50)
            .collect(),
    })
}
fn catalog_file(path: &Path) -> Result<(PathBuf, String), String> {
    let path = paths::local_file(path.to_str().ok_or("Invalid catalog path")?)?;
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_none_or(|e| !e.eq_ignore_ascii_case("json"))
    {
        return Err("Select a Medal clips.json catalog.".into());
    }
    let meta = std::fs::metadata(&path).map_err(|_| "Catalog is unavailable")?;
    if meta.len() > MAX_CATALOG {
        return Err("Catalog exceeds the 64 MiB safety limit.".into());
    }
    let modified = meta
        .modified()
        .map_err(|_| "Catalog timestamp unavailable")?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Invalid catalog timestamp")?
        .as_nanos();
    let revision = format!(
        "{}:{modified}:{:?}",
        meta.len(),
        crate::library::files::identity(&path)
    );
    Ok((path, revision))
}

#[tauri::command]
pub async fn medal_select(
    app: tauri::AppHandle,
    library: State<'_, LibraryState>,
    recordings: State<'_, RecordingState>,
) -> Result<Option<Preview>, String> {
    let library = library.get()?;
    let epoch = library.epoch();
    library.check_epoch(epoch)?;
    if !library.is_idle() {
        return Err("Finish or cancel the current library operation first.".into());
    }
    recordings.begin_picker()?;
    let dialog = app.clone();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        dialog
            .dialog()
            .file()
            .set_title("Import Medal clips.json — read-only preview")
            .add_filter("Medal catalog", &["json"])
            .blocking_pick_file()
    })
    .await;
    recordings.end_picker();
    let Some(selected) = selected.map_err(|_| "Catalog picker failed")? else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|_| "Select a local catalog")?;
    let operation = library.operation().await?;
    library.check_epoch(epoch)?;
    library.medal.begin("preparing")?;
    let worker = library.clone();
    // Owned task completes cleanup even if the webview drops its response.
    let task = tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let result = async {
            let (path, revision) =
                tauri::async_runtime::spawn_blocking(move || catalog_file(&path))
                    .await
                    .map_err(|_| "Catalog validation failed")??;
            let path_text = path.to_str().ok_or("Invalid catalog path")?.to_owned();
            let source = paths::key(&path_text).ok_or("Invalid catalog path")?;
            let source_key = source.clone();
            let file_revision = revision.clone();
            let source_bytes = revision
                .split(':')
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(MAX_CATALOG);
            let (index, cached) = worker
                .database(move |db| {
                    Ok((
                        db.medal_index(&source_key)?,
                        db.cached_medal_catalog(&source_key, &file_revision, source_bytes)?,
                    ))
                })
                .await?;
            let state = worker.medal.clone();
            let (prepared, mut counts, skipped) = tauri::async_runtime::spawn_blocking(move || {
                let catalog = if let Some(cached) = cached {
                    cached
                } else {
                    let file =
                        std::fs::File::open(&path).map_err(|_| "Catalog could not be read")?;
                    let mut bytes = Vec::new();
                    file.take(MAX_CATALOG + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "Catalog could not be read")?;
                    if bytes.len() as u64 > MAX_CATALOG {
                        return Err("Catalog exceeds the 64 MiB safety limit".into());
                    }
                    parse::parse(&bytes)?
                };
                state.work.lock().unwrap().progress.total = catalog.entries.len();
                let (prepared, counts) =
                    store::prepare(catalog.entries, index, &state.cancel, &state.done)?;
                Ok::<_, String>((prepared, counts, catalog.skipped))
            })
            .await
            .map_err(|_| "Catalog preparation failed")??;
            counts.skipped = skipped;
            if worker.medal.cancel.load(Ordering::Acquire) {
                return Err("Import cancelled".into());
            }
            let mut work = worker.medal.work.lock().unwrap();
            work.token += 1;
            work.plan = Some(Arc::new(Plan {
                path: path_text,
                source,
                revision,
                prepared,
                counts,
            }));
            Ok(0)
        }
        .await;
        worker.medal.finish(&result);
        if result.is_ok() {
            worker.medal.work.lock().unwrap().progress.phase = "preview".into();
        }
        result
    });
    task.await.map_err(|_| "Catalog preparation stopped")??;
    let token = library.medal.work.lock().unwrap().token;
    Ok(Some(preview(&library.medal, token, 0)?))
}

#[tauri::command]
pub async fn medal_preview(
    library: State<'_, LibraryState>,
    token: u64,
    offset: usize,
) -> Result<Preview, String> {
    let library = library.get()?;
    let _operation = library.operation().await?;
    preview(&library.medal, token, offset)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Confirmation {
    token: u64,
    additions: Vec<usize>,
    #[serde(default)]
    include_new: bool,
}
#[tauri::command]
pub async fn medal_commit(
    app: tauri::AppHandle,
    recordings: State<'_, RecordingState>,
    library: State<'_, LibraryState>,
    confirmation: Confirmation,
) -> Result<Progress, String> {
    let library = library.get()?;
    let operation = library.operation().await?;
    let plan = {
        let work = library.medal.work.lock().unwrap();
        if work.token != confirmation.token {
            return Err("This preview has expired.".into());
        }
        work.plan.clone().ok_or("Prepare an import first")?
    };
    let mut additions: HashSet<_> = confirmation.additions.into_iter().collect();
    let allowed: HashSet<_> = plan
        .prepared
        .iter()
        .flat_map(|p| &p.references)
        .filter(|r| r.row.status == "new")
        .map(|r| r.row.index)
        .collect();
    if confirmation.include_new {
        additions.extend(allowed.iter().copied());
    }
    if additions.len() > 100_000 || !additions.is_subset(&allowed) {
        return Err("Invalid new-recording approval.".into());
    }
    library.medal.begin("importing")?;
    library.medal.work.lock().unwrap().progress.total = plan.prepared.len();
    let owner = library.clone();
    let task = tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let state = owner.medal.clone();
        let result = owner
            .database(move |db| {
                if catalog_file(Path::new(&plan.path))?.1 != plan.revision {
                    return Err("Catalog changed after preview. Select it again.".into());
                }
                let source_revision = serde_json::to_string(&(
                    parse::NORMALIZATION_VERSION,
                    &plan.revision,
                    plan.counts.skipped,
                    plan.prepared
                        .iter()
                        .map(|p| &p.entry.id)
                        .collect::<Vec<_>>(),
                ))
                .map_err(|e| e.to_string())?;
                db.commit_medal(store::Commit {
                    source: &plan.source,
                    path: &plan.path,
                    revision: &source_revision,
                    prepared: &plan.prepared,
                    additions: &additions,
                    cancel: &state.cancel,
                    done: &state.done,
                })
            })
            .await;
        owner.medal.finish(&result);
        if result.is_ok() {
            owner.medal.work.lock().unwrap().plan = None;
        }
        result
    });
    task.await.map_err(|_| "Import stopped")??;
    library
        .start_provider(crate::config::ffprobe(&app)?, recordings.analysis.clone())
        .await?;
    let progress = library.medal.work.lock().unwrap().progress.clone();
    Ok(progress)
}
#[tauri::command]
pub async fn medal_status(library: State<'_, LibraryState>) -> Result<Progress, String> {
    let library = library.get()?;
    let mut progress = library.medal.work.lock().unwrap().progress.clone();
    progress.done = library.medal.done.load(Ordering::Acquire);
    Ok(progress)
}
#[tauri::command]
pub async fn medal_cancel(library: State<'_, LibraryState>) -> Result<(), String> {
    let library = library.get()?;
    library.medal.cancel.store(true, Ordering::Release);
    library.medal.work.lock().unwrap().plan = None;
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/medal/mod.rs"]
mod tests;
