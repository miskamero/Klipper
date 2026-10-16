mod commands;
pub(crate) mod db;
pub(crate) mod files;
mod query;
mod scan;
#[cfg(test)]
#[path = "../../tests/library/mod.rs"]
mod tests;
pub(crate) mod thumbnails;
pub use commands::*;

use crate::analysis::Analysis;
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
    pub fn open(path: &Path, cache: &Path) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            db: Arc::new(Mutex::new(db::Db::open(path)?)),
            scan: Mutex::new(ScanState {
                progress: Progress::default(),
                cancel: None,
                stopped: false,
            }),
            idle: watch::channel(true).0,
            thumbnails: thumbnails::Thumbnails::new(cache),
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
        *self.idle.borrow()
    }
    pub fn cancel(&self, analysis: &Analysis) {
        if let Some(cancel) = &self.scan.lock().unwrap().cancel {
            cancel.store(true, Ordering::Release);
        }
        analysis.cancel_background();
    }
    pub fn stop(&self, analysis: &Analysis) {
        self.scan.lock().unwrap().stopped = true;
        self.cancel(analysis);
        self.thumbnails.stop(analysis);
    }
    pub async fn shutdown(&self, analysis: &Analysis) {
        let mut idle = self.idle.subscribe();
        self.stop(analysis);
        let _ = idle.wait_for(|value| *value).await;
        self.thumbnails.shutdown().await;
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
