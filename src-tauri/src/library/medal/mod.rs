pub(crate) mod parse;
pub(crate) mod paths;
mod selection;
mod sqlite;
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
    source_format: String,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    phase: String,
    done: usize,
    total: usize,
    linked: usize,
    error: String,
    warning: String,
    committed: bool,
}
struct Work {
    token: u64,
    plan: Option<Arc<Plan>>,
    progress: Progress,
}
pub struct ImportState {
    work: Mutex<Work>,
    cancel: Arc<AtomicBool>,
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
            cancel: Arc::new(AtomicBool::new(false)),
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
    source_format: String,
    source_path: String,
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
        source_format: plan.source_format.clone(),
        source_path: plan.path.clone(),
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
        .is_none_or(|e| !e.eq_ignore_ascii_case("json") && !e.eq_ignore_ascii_case("db"))
    {
        return Err("Select Medal clips.json or a supported SQLite .db database.".into());
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
    let mut revision = format!(
        "{}:{modified}:{:?}",
        meta.len(),
        crate::library::files::identity(&path)
    );
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("db"))
    {
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        if std::fs::symlink_metadata(&wal).is_ok() {
            paths::local_file(wal.to_str().ok_or("Invalid WAL path")?)?;
        }
        let stamp = std::fs::metadata(&wal)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        revision.push_str(&format!(":wal:{stamp:?}"));
    }
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
            .set_title("Import Medal metadata — read-only preview")
            .add_filter("Medal JSON or SQLite", &["json", "db"])
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
            let source_format = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("db"))
            {
                "sqlite"
            } else {
                "json"
            }
            .to_owned();
            let sqlite_source = source_format == "sqlite";
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
                        if sqlite_source {
                            None
                        } else {
                            db.cached_medal_catalog(&source_key, &file_revision, source_bytes)?
                        },
                    ))
                })
                .await?;
            let state = worker.medal.clone();
            let (prepared, counts) = tauri::async_runtime::spawn_blocking(move || {
                let catalog = if sqlite_source {
                    sqlite::load(&path, state.cancel.clone())?
                } else if let Some(cached) = cached {
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
                let (prepared, mut counts) =
                    store::prepare(catalog.entries, index, &state.cancel, &state.done)?;
                counts.skipped = catalog.skipped;
                counts.invalid_identities = catalog.invalid_identities;
                Ok::<_, String>((prepared, counts))
            })
            .await
            .map_err(|_| "Catalog preparation failed")??;
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
                source_format,
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
    let analysis = recordings.analysis.clone();
    let owner = library.clone();
    let task =
        tauri::async_runtime::spawn(async move {
            let _operation = operation;
            let state = owner.medal.clone();
            let source_for_previews = plan.source.clone();
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
            if result.is_ok() {
                {
                    let mut work = owner.medal.work.lock().unwrap();
                    work.progress.committed = true;
                    work.progress.phase = "previews".into();
                }
                let warning = match crate::config::ffmpeg(&app) {
                Ok(ffmpeg) => {
                    owner
                        .materialize_medal_previews(source_for_previews, ffmpeg, analysis.clone())
                        .await
                }
                Err(_) => Some(
                    "Metadata saved. FFmpeg is unavailable, so provider previews were not cached."
                        .into(),
                ),
            };
                if let Some(warning) = warning {
                    owner.medal.work.lock().unwrap().progress.warning = warning;
                }
            }
            if result.is_ok() {
                owner.medal.work.lock().unwrap().plan = None;
            }
            owner.medal.finish(&result);
            if result.is_ok() {
                match crate::config::ffprobe(&app) {
                    Ok(executable) => {
                        if owner
                            .start_provider(executable, analysis.clone())
                            .await
                            .is_err()
                        {
                            owner.medal.work.lock().unwrap().progress.warning.push_str(
                                " Metadata saved; background inspection could not start.",
                            );
                        }
                    }
                    Err(_) => owner.medal.work.lock().unwrap().progress.warning.push_str(
                        " Metadata saved; FFprobe is unavailable for pending inspections.",
                    ),
                }
            }
            // Capture the reply before releasing the operation guard: reset may clear
            // shared progress as soon as this owned task has finished.
            result.map(|_| owner.medal.work.lock().unwrap().progress.clone())
        });
    task.await.map_err(|_| "Import stopped")?
}

impl crate::library::Library {
    async fn materialize_medal_previews(
        self: &Arc<Self>,
        source: String,
        executable: PathBuf,
        analysis: Arc<crate::analysis::Analysis>,
    ) -> Option<String> {
        let ids=self.database(move|db|{
            let mut s=db.connection.prepare("SELECT DISTINCT l.file_id FROM provider_links l JOIN sources s ON s.id=l.source_id JOIN provider_previews p ON p.source_id=l.source_id AND p.external_id=l.external_id AND p.relation=l.relation WHERE s.kind='medal' AND s.external_key=?1 ORDER BY l.file_id").map_err(|e|e.to_string())?;
            let ids=s.query_map([source],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
            Ok(ids)
        }).await;
        let Ok(ids) = ids else {
            return Some("Metadata saved; preview caching could not start.".into());
        };
        let mut pending = ids.len().saturating_sub(2000);
        self.medal.done.store(0, Ordering::Release);
        self.medal.work.lock().unwrap().progress.total = ids.len().min(2000);
        for (index, &id) in ids.iter().take(2000).enumerate() {
            if self.medal.cancel.load(Ordering::Acquire) {
                pending += ids.len().min(2000) - index;
                break;
            }
            if self.cached_thumbnail(id).await.ok().flatten().is_some() {
                self.medal.done.fetch_add(1, Ordering::Release);
                continue;
            }
            let token = format!("medal-preview-{id}");
            // Importing copies provider images; video decoding stays lazy in the gallery.
            let work = self.thumbnail_with_fallback(
                id,
                token.clone(),
                executable.clone(),
                analysis.clone(),
                false,
            );
            tokio::pin!(work);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            let outcome = loop {
                tokio::select! {
                    result=&mut work=>break result,
                    _=tokio::time::sleep(std::time::Duration::from_millis(100))=>{
                        if self.medal.cancel.load(Ordering::Acquire) || std::time::Instant::now()>=deadline {
                            self.thumbnails.cancel(&token,&analysis);
                            break work.await;
                        }
                    }
                }
            };
            if !matches!(outcome, Ok(Some(_))) {
                pending += 1;
            }
            self.medal.done.fetch_add(1, Ordering::Release);
        }
        (pending>0).then(||format!("Metadata saved. {pending} previews were not cached (cancelled, busy, unavailable, or beyond the 2,000-preview retention limit); they can load on demand with FFmpeg fallback."))
    }
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
