mod commands;
pub(crate) mod db;
pub(crate) mod files;
pub(crate) mod medal;
mod navigation;
pub(crate) mod providers;
mod query;
mod reset;
mod scan;
#[cfg(test)]
#[path = "../../tests/library/mod.rs"]
mod tests;
pub(crate) mod thumbnails;
pub use commands::*;

use crate::analysis::Analysis;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub id: i64,
    pub path: String,
    #[serde(skip_serializing)]
    pub identity: Option<String>,
    pub scan_state: String,
    pub error: Option<String>,
    pub last_scan: Option<i64>,
    pub recordings: i64,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub root_id: Option<i64>,
    pub phase: String,
    pub discovered: u64,
    pub analyzed: u64,
    pub reused: u64,
    pub skipped_links: u64,
    pub issue_count: u64,
    pub issues: Vec<String>,
}

struct ScanState {
    progress: Progress,
    cancel: Option<Arc<AtomicBool>>,
    stopped: bool,
}
pub struct Library {
    db: Arc<Mutex<db::Db>>,
    scan: Mutex<ScanState>,
    idle: watch::Sender<bool>,
    pub(crate) thumbnails: Arc<thumbnails::Thumbnails>,
    pub(crate) medal: Arc<medal::ImportState>,
    operations: Arc<tokio::sync::RwLock<()>>,
    resetting: AtomicBool,
    generation: std::sync::atomic::AtomicU64,
    stopped: AtomicBool,
}
pub struct LibraryState(pub Result<Arc<Library>, String>);

impl LibraryState {
    pub fn get(&self) -> Result<Arc<Library>, String> {
        self.0.as_ref().cloned().map_err(Clone::clone)
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

impl Library {
    // Imported exact-file grants use the same bounded FFprobe worker as folder discovery.
    // Pending items remain in SQLite if the user cancels or closes the application.
    pub async fn start_provider(
        self: &Arc<Self>,
        executable: std::path::PathBuf,
        analysis: Arc<Analysis>,
    ) -> Result<(), String> {
        let total=self.database(|db|db.connection.query_row("SELECT count(DISTINCT f.id) FROM recordings f JOIN locations l ON l.file_id=f.id JOIN provider_access a ON a.location_id=l.id WHERE f.status='pending' AND a.availability='available'",[],|r|r.get::<_,i64>(0)).map_err(|e|e.to_string())).await?;
        if total == 0 {
            return Ok(());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut state = self.scan.lock().unwrap();
            if state.stopped || !self.is_idle() {
                return Ok(());
            }
            state.progress = Progress {
                phase: "analyzing".into(),
                discovered: total as u64,
                ..Default::default()
            };
            state.cancel = Some(cancel.clone());
            self.idle.send_replace(false);
        }
        let library = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = async {
                loop {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let next = library
                        .database(|db| {
                            db.connection.query_row(
                            "SELECT f.id FROM recordings f JOIN locations l ON l.file_id=f.id
                            JOIN provider_access a ON a.location_id=l.id
                            WHERE f.status='pending' AND a.availability='available'
                            ORDER BY f.id LIMIT 1", [], |r| r.get::<_, i64>(0)
                        ).optional().map_err(|e| e.to_string())
                        })
                        .await?;
                    let Some(id) = next else {
                        break;
                    };
                    let file = match library
                        .database(move |db| db.resolve_file(id).map(|r| r.0))
                        .await
                    {
                        Ok(file) => file,
                        Err(error) => {
                            let message = error.clone();
                            library
                                .database(move |db| db.fail_media(id, &message))
                                .await?;
                            library.issue(error);
                            continue;
                        }
                    };
                    if let Some(reply) = analysis
                        .try_background(executable.clone(), std::path::PathBuf::from(&file.path))
                    {
                        let info = reply.await.map_err(|_| "Metadata worker stopped")?;
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        match info {
                            Ok(Some(info)) => {
                                library
                                    .database(move |db| db.save_media(id, &file, &info))
                                    .await?;
                                library.scan.lock().unwrap().progress.analyzed += 1;
                            }
                            Ok(None) => {}
                            Err(error) => {
                                let message = error.clone();
                                library
                                    .database(move |db| db.fail_media(id, &message))
                                    .await?;
                                library.issue(error);
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Ok::<_, String>(())
            }
            .await;
            if let Err(error) = &result {
                library.issue(error.clone());
            }
            let mut state = library.scan.lock().unwrap();
            state.progress.phase = if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else if result.is_err() {
                "failed"
            } else {
                "complete"
            }
            .into();
            state.cancel = None;
            library.idle.send_replace(true);
        });
        Ok(())
    }
    pub fn open(path: &Path, cache: &Path) -> Result<Arc<Self>, String> {
        let mut db = db::Db::open(path)?;
        let thumbnails = thumbnails::Thumbnails::new(cache);
        if db.cache_cleanup_pending()? && thumbnails.clear_cache_sync().is_ok() {
            db.finish_cache_cleanup()?;
        }
        Ok(Arc::new(Self {
            db: Arc::new(Mutex::new(db)),
            scan: Mutex::new(ScanState {
                progress: Progress::default(),
                cancel: None,
                stopped: false,
            }),
            idle: watch::channel(true).0,
            thumbnails,
            medal: Arc::new(medal::ImportState::default()),
            operations: Arc::new(tokio::sync::RwLock::new(())),
            resetting: AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
            stopped: AtomicBool::new(false),
        }))
    }

    pub async fn database<T: Send + 'static>(
        &self,
        action: impl FnOnce(&mut db::Db) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let db = self.db.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut database = db.lock().map_err(|_| "Library database unavailable")?;
            action(&mut database)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub fn progress(&self) -> Progress {
        self.scan.lock().unwrap().progress.clone()
    }
    pub fn is_idle(&self) -> bool {
        !self.resetting.load(Ordering::Acquire) && *self.idle.borrow() && self.medal.is_idle()
    }
    pub fn cancel(&self, analysis: &Analysis) {
        if let Some(cancel) = &self.scan.lock().unwrap().cancel {
            cancel.store(true, Ordering::Release);
        }
        analysis.cancel_background();
    }
    pub fn stop(&self, analysis: &Analysis) {
        self.stopped.store(true, Ordering::Release);
        self.medal.stop();
        self.scan.lock().unwrap().stopped = true;
        self.cancel(analysis);
        self.thumbnails.stop(analysis);
    }
    pub async fn shutdown(&self, analysis: &Analysis) {
        let mut idle = self.idle.subscribe();
        self.stop(analysis);
        let _ = idle.wait_for(|value| *value).await;
        self.thumbnails.shutdown().await;
        self.medal.shutdown().await;
        let _operations = self.operations.write().await;
    }

    pub async fn start(
        self: &Arc<Self>,
        root: i64,
        executable: std::path::PathBuf,
        analysis: Arc<Analysis>,
    ) -> Result<(), String> {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut state = self.scan.lock().unwrap();
            if state.stopped {
                return Err("Library is shutting down".into());
            }
            if !self.is_idle() {
                return Err("Cancel or finish the current folder scan first.".into());
            }
            state.progress = Progress {
                root_id: Some(root),
                phase: "discovering".into(),
                ..Default::default()
            };
            state.cancel = Some(cancel.clone());
            self.idle.send_replace(false);
        }
        let library = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = library
                .scan_root(root, executable, analysis, cancel.clone())
                .await;
            let mut final_state = if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else if result.is_err() {
                "failed"
            } else if library.progress().issue_count > 0 {
                "partial"
            } else {
                "complete"
            };
            if let Err(error) = result {
                library.issue(error);
            }
            let message = library.progress().issues.first().cloned();
            if let Err(error) = library
                .database(move |db| db.finish_scan(root, final_state, message.as_deref()))
                .await
            {
                library.issue(error);
                final_state = "failed";
            }
            let mut state = library.scan.lock().unwrap();
            state.progress.phase = final_state.into();
            state.cancel = None;
            library.idle.send_replace(true);
        });
        Ok(())
    }

    fn issue(&self, message: String) {
        let mut state = self.scan.lock().unwrap();
        state.progress.issue_count += 1;
        if state.progress.issues.len() < 20 {
            state.progress.issues.push(message);
        }
    }
}
