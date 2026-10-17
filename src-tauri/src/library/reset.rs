use super::{db::Db, Library};
use crate::analysis::Analysis;
use serde::Serialize;
use std::sync::{atomic::Ordering, Arc};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetResult {
    pub cache_warning: Option<String>,
}

impl Db {
    pub fn cache_cleanup_pending(&self) -> Result<bool, String> {
        self.connection
            .query_row(
                "SELECT cache_cleanup_pending FROM library_state WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn finish_cache_cleanup(&mut self) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE library_state SET cache_cleanup_pending=0 WHERE id=1",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn reset_library(&mut self) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        // Explicit library-owned tables only. Do not delete the database or external paths.
        tx.execute_batch(
            "DELETE FROM provider_previews; DELETE FROM provider_links;
            DELETE FROM provider_access; DELETE FROM provider_imports; DELETE FROM provider_entries;
            DELETE FROM source_recordings; DELETE FROM recording_overrides; DELETE FROM media;
            DELETE FROM root_entries; DELETE FROM locations; DELETE FROM roots;
            DELETE FROM recordings; DELETE FROM sources;
            UPDATE library_state SET cache_cleanup_pending=1 WHERE id=1;",
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
}

impl Library {
    pub fn epoch(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub async fn operation(&self) -> Result<tokio::sync::OwnedRwLockReadGuard<()>, String> {
        let epoch = self.epoch();
        self.check_epoch(epoch)?;
        let guard = self.operations.clone().read_owned().await;
        self.check_epoch(epoch)?;
        Ok(guard)
    }
    pub fn check_epoch(&self, epoch: u64) -> Result<(), String> {
        if self.resetting.load(Ordering::Acquire)
            || self.stopped.load(Ordering::Acquire)
            || self.epoch() != epoch
        {
            Err("Library changed or is resetting. Try again when reset finishes.".into())
        } else {
            Ok(())
        }
    }
    pub async fn reset<F: std::future::Future<Output = Result<(), String>>>(
        self: &Arc<Self>,
        analysis: Arc<Analysis>,
        before_clear: impl FnOnce() -> F,
    ) -> Result<ResetResult, String> {
        if self.resetting.swap(true, Ordering::AcqRel) {
            return Err("Library reset is already running.".into());
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        // Pause admission first, then signal owned workers and wait for all writes and children.
        self.scan.lock().unwrap().stopped = true;
        self.medal.stop();
        self.cancel(&analysis);
        self.thumbnails.stop(&analysis);
        let mut idle = self.idle.subscribe();
        let _ = idle.wait_for(|v| *v).await;
        self.medal.shutdown().await;
        self.thumbnails.shutdown().await;
        let guard = self.operations.write().await;
        let result=async {
            before_clear().await?;
            self.database(|db|db.reset_library()).await?;
            let previews=self.thumbnails.clone();
            let cleanup=tauri::async_runtime::spawn_blocking(move ||previews.clear_cache_sync()).await.map_err(|e|e.to_string()).and_then(|v|v);
            let cleanup=match cleanup {Ok(())=>self.database(|db|db.finish_cache_cleanup()).await,Err(e)=>Err(e)};
            Ok(ResetResult { cache_warning:cleanup.err().map(|e|format!("Library was cleared, but thumbnail cleanup is pending: {e}. Old previews are disabled; retry Reset Library to finish cleanup.")) })
        }.await;
        if !self.stopped.load(Ordering::Acquire) {
            let mut scan = self.scan.lock().unwrap();
            scan.stopped = false;
            scan.progress = super::Progress::default();
            drop(scan);
            self.medal.resume();
            self.thumbnails.resume();
        }
        drop(guard);
        self.resetting.store(false, Ordering::Release);
        result
    }
}
