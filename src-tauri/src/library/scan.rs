use super::{files, Library};
use crate::analysis::Analysis;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

impl Library {
    pub(super) async fn scan_root(
        &self,
        id: i64,
        executable: PathBuf,
        analysis: Arc<Analysis>,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let root = self.database(move |db| db.root(id)).await?;
        let scan_root = root.clone();
        let generation = self
            .database(move |db| {
                files::within(
                    Path::new(&scan_root.path),
                    Path::new(&scan_root.path),
                    scan_root.identity.as_deref(),
                )?;
                db.begin_scan(id)
            })
            .await?;
        let (send, mut receive) = tokio::sync::mpsc::channel(32);
        let path = PathBuf::from(&root.path);
        let stop = cancel.clone();
        let walker = tauri::async_runtime::spawn_blocking(move || files::walk(&path, stop, send));
        let discovery = async {
            while let Some(entry) = receive.recv().await {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                match entry {
                    files::Entry::Issue(error) => self.issue(error),
                    files::Entry::Link => self.scan.lock().unwrap().progress.skipped_links += 1,
                    files::Entry::File(file) => {
                        let location = root.clone();
                        let discovered = self
                            .database(move |db| {
                                files::within(
                                    Path::new(&file.path),
                                    Path::new(&location.path),
                                    location.identity.as_deref(),
                                )?;
                                db.discover(id, generation, &file)
                            })
                            .await;
                        let reused = match discovered {
                            Ok(reused) => reused,
                            Err(error) => {
                                self.issue(error);
                                continue;
                            }
                        };
                        let mut state = self.scan.lock().unwrap();
                        state.progress.discovered += 1;
                        if reused {
                            state.progress.reused += 1;
                        }
                    }
                }
            }
            Ok::<_, String>(())
        }
        .await;
        // Closing the receiver releases a walker blocked by channel backpressure.
        drop(receive);
        walker.await.map_err(|e| e.to_string())?;
        discovery?;
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        let complete = self.progress().issue_count == 0;
        self.database(move |db| db.finish_discovery(id, generation, complete))
            .await?;
        self.scan.lock().unwrap().progress.phase = "analyzing".into();
        loop {
            if cancel.load(Ordering::Acquire) || analysis.is_stopped() {
                break;
            }
            let Some((file_id, path)) = self.database(move |db| db.pending(id)).await? else {
                break;
            };
            let location = root.clone();
            let unavailable_path = path.clone();
            let before = tauri::async_runtime::spawn_blocking(move || {
                files::within(
                    &path,
                    Path::new(&location.path),
                    location.identity.as_deref(),
                )?;
                files::describe(&path)
            })
            .await
            .map_err(|e| e.to_string())?;
            let file = match before {
                Ok(file) => file,
                Err(error) => {
                    let message = error.clone();
                    self.database(move |db| {
                        db.unavailable(
                            &unavailable_path.to_string_lossy(),
                            matches!(unavailable_path.try_exists(), Ok(false)),
                        )?;
                        db.fail_media(file_id, &message)
                    })
                    .await?;
                    self.issue(error);
                    continue;
                }
            };
            // Refresh the fingerprint if a file changed after discovery, before probing.
            let refresh = file.clone();
            let ready = self
                .database(move |db| {
                    let ready = db.discover(id, generation, &refresh)?;
                    let pending = db.pending(id)?.map(|(id, _)| id);
                    Ok(ready || pending != Some(file_id))
                })
                .await?;
            if ready {
                continue;
            }
            let result = loop {
                if cancel.load(Ordering::Acquire) || analysis.is_stopped() {
                    break None;
                }
                if let Some(reply) =
                    analysis.try_background(executable.clone(), PathBuf::from(&file.path))
                {
                    let result = reply.await.map_err(|_| "Background analysis stopped")?;
                    if let Some(media) = result.as_ref().ok().and_then(|m| m.as_ref()) {
                        break Some(Ok(media.clone()));
                    }
                    if let Err(error) = result {
                        break Some(Err(error));
                    }
                    // Superseded by foreground inspection. Retry later without displaying an error.
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            let Some(result) = result else { break };
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let probe_file = file.clone();
            let after = tauri::async_runtime::spawn_blocking(move || {
                files::describe(Path::new(&probe_file.path))
            })
            .await
            .map_err(|e| e.to_string())?;
            let unchanged = after.is_ok_and(|after| {
                after.identity == file.identity
                    && after.size == file.size
                    && after.modified_ns == file.modified_ns
            });
            match result {
                Ok(media) if unchanged => {
                    self.database(move |db| db.save_media(file_id, &file, &media))
                        .await?;
                    self.scan.lock().unwrap().progress.analyzed += 1;
                }
                other => {
                    let error = other.err().unwrap_or(
                        "Recording changed during analysis; rescan after recording finishes."
                            .into(),
                    );
                    let message = error.clone();
                    self.database(move |db| db.fail_media(file_id, &message))
                        .await?;
                    self.issue(error);
                }
            }
        }
        Ok(())
    }
}
