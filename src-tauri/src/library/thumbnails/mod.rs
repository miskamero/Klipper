mod cache;
use super::{files, Library};
use crate::{analysis::Analysis, config, media_process};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::State;
use tokio::sync::watch;

pub struct Thumbnails {
    directory: PathBuf,
    cache: Mutex<Option<cache::Cache>>,
    state: Mutex<Work>,
    idle: watch::Sender<bool>,
    cache_disabled: std::sync::atomic::AtomicBool,
}
#[derive(Default)]
struct Work {
    active: Option<(String, watch::Sender<bool>)>,
    stopped: bool,
}

struct Ticket {
    owner: Arc<Thumbnails>,
    analysis: Arc<Analysis>,
    token: String,
    cancel: watch::Receiver<bool>,
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.owner.cancel(&self.token, &self.analysis);
        let mut state = self.owner.state.lock().unwrap();
        if state
            .active
            .as_ref()
            .is_some_and(|(token, _)| *token == self.token)
        {
            state.active = None;
            self.owner.idle.send_replace(true);
        }
    }
}

impl Thumbnails {
    pub(super) fn resume(&self) {
        self.state.lock().unwrap().stopped = false;
    }
    pub(super) fn clear_cache_sync(&self) -> Result<(), String> {
        use std::sync::atomic::Ordering;
        self.cache_disabled.store(true, Ordering::Release);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "Thumbnail cache unavailable")?;
        *cache = None;
        cache::reset_owned(&self.directory)?;
        self.cache_disabled.store(false, Ordering::Release);
        Ok(())
    }
    pub fn new(directory: &Path) -> Arc<Self> {
        Arc::new(Self {
            directory: directory.into(),
            cache: Mutex::new(None),
            state: Mutex::default(),
            idle: watch::channel(true).0,
            cache_disabled: std::sync::atomic::AtomicBool::new(false),
        })
    }
    pub fn is_idle(&self) -> bool {
        *self.idle.borrow()
    }
    fn begin(
        self: &Arc<Self>,
        token: String,
        analysis: Arc<Analysis>,
    ) -> Result<Option<Ticket>, String> {
        if token.len() > 64
            || token.is_empty()
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Invalid thumbnail request.".into());
        }
        let mut state = self.state.lock().unwrap();
        if state.stopped || state.active.is_some() {
            return Ok(None);
        }
        let (cancel, receiver) = watch::channel(false);
        state.active = Some((token.clone(), cancel));
        self.idle.send_replace(false);
        Ok(Some(Ticket {
            owner: self.clone(),
            token,
            analysis,
            cancel: receiver,
        }))
    }
    pub fn cancel(&self, token: &str, analysis: &Analysis) {
        let state = self.state.lock().unwrap();
        if let Some((current, cancel)) = &state.active {
            if current == token {
                cancel.send_replace(true);
                analysis.cancel_thumbnail(token);
            }
        }
    }
    pub fn stop(&self, analysis: &Analysis) {
        let mut state = self.state.lock().unwrap();
        state.stopped = true;
        if let Some((token, cancel)) = &state.active {
            cancel.send_replace(true);
            analysis.cancel_thumbnail(token);
        }
    }
    pub async fn shutdown(&self) {
        let mut idle = self.idle.subscribe();
        let _ = idle.wait_for(|v| *v).await;
    }
    async fn cache<T: Send + 'static>(
        self: &Arc<Self>,
        run: impl FnOnce(&mut cache::Cache) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if owner
                .cache_disabled
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Err("Thumbnail cleanup is pending. Retry Reset Library.".into());
            }
            let mut cache = owner
                .cache
                .lock()
                .map_err(|_| "Thumbnail cache unavailable")?;
            if cache.is_none() {
                *cache = Some(cache::Cache::open(&owner.directory)?);
            }
            run(cache.as_mut().unwrap())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

fn fingerprint(id: i64, file: &files::File) -> Result<String, String> {
    // Full metadata tuple avoids hash collisions and reuses images across renamed/hardlinked locations.
    serde_json::to_string(&(
        "jpeg-320x180-q5-v1",
        id,
        &file.identity,
        if file.identity.is_none() {
            Some(&file.path)
        } else {
            None
        },
        file.size,
        &file.modified_ns,
    ))
    .map_err(|e| e.to_string())
}
pub(crate) fn valid_image(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && bytes.len() <= cache::MAX_IMAGE
        && bytes.starts_with(&[255, 216])
        && bytes.ends_with(&[255, 217])
}
pub(crate) async fn extract(
    executable: &Path,
    path: &Path,
    seconds: f64,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Option<Vec<u8>>, String> {
    let mut command = tokio::process::Command::new(executable);
    command.args(["-v","error","-nostdin","-hwaccel","none","-threads","1",
        "-filter_threads","1","-protocol_whitelist","file","-format_whitelist","mov,matroska,webm",
        "-probesize","2000000","-analyzeduration","2000000","-ss"])
        .arg(format!("{seconds:.3}")).arg("-i").arg(path)
        .args(["-map","0:V:0","-frames:v","1","-an","-sn","-dn",
            // Scale from display aspect ratio (including anamorphic pixels) to square pixels.
            "-vf","scale=w='max(2,trunc(min(320,180*dar)/2)*2)':h='max(2,trunc(min(180,320/dar)/2)*2)',setsar=1",
            "-c:v","mjpeg","-threads","1","-q:v","5","-f","image2pipe","pipe:1"]);
    match media_process::capture(&mut command,cancel,Duration::from_secs(10),cache::MAX_IMAGE as u64,"ffmpeg",true).await? {
        None=>Ok(None),
        Some((bytes,status)) if status.success() && valid_image(&bytes)=>Ok(Some(bytes)),
        Some(_)=>Err("FFmpeg could not extract a preview. The recording may be unsupported, damaged or unavailable.".into()),
    }
}

pub(crate) async fn extract_image(
    executable: &Path,
    path: &Path,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Option<Vec<u8>>, String> {
    let mut command = tokio::process::Command::new(executable);
    command.args(["-v","error","-nostdin","-max_alloc","67108864","-threads","1","-filter_threads","1","-protocol_whitelist","file","-format_whitelist","image2","-f","image2","-pattern_type","none","-i"]).arg(path)
        .args(["-frames:v","1","-an","-sn","-dn","-vf","scale=w='max(2,trunc(min(320,180*dar)/2)*2)':h='max(2,trunc(min(180,320/dar)/2)*2)',setsar=1","-c:v","mjpeg","-threads","1","-q:v","5","-f","image2pipe","pipe:1"]);
    match media_process::capture(
        &mut command,
        cancel,
        Duration::from_secs(5),
        cache::MAX_IMAGE as u64,
        "ffmpeg",
        true,
    )
    .await?
    {
        None => Ok(None),
        Some((bytes, status)) if status.success() && valid_image(&bytes) => Ok(Some(bytes)),
        _ => Err("The provider thumbnail could not be decoded.".into()),
    }
}

async fn generate(
    ticket: &mut Ticket,
    executable: PathBuf,
    path: PathBuf,
    seconds: Option<f64>,
) -> Result<Option<Vec<u8>>, String> {
    loop {
        if *ticket.cancel.borrow() || ticket.analysis.is_stopped() {
            return Ok(None);
        }
        let reply = if let Some(seconds) = seconds {
            ticket.analysis.try_thumbnail(
                executable.clone(),
                path.clone(),
                seconds,
                ticket.token.clone(),
            )
        } else {
            ticket
                .analysis
                .try_image(executable.clone(), path.clone(), ticket.token.clone())
        };
        if let Some(mut reply) = reply {
            let result=tokio::select!{
                value=&mut reply=>value,
                _=async{let _=ticket.cancel.wait_for(|v|*v).await;}=>{ticket.analysis.cancel_thumbnail(&ticket.token);reply.await}
            }.map_err(|_|"Thumbnail worker stopped")??;
            if *ticket.cancel.borrow() {
                return Ok(None);
            }
            if result.is_some() {
                return Ok(result);
            }
        }
        tokio::select! {_=ticket.cancel.wait_for(|v|*v)=>return Ok(None),_=tokio::time::sleep(Duration::from_millis(100))=>{}}
    }
}

impl Library {
    async fn provider_image(
        &self,
        id: i64,
        file: &files::File,
    ) -> Result<Option<(String, String, String)>, String> {
        let file_copy = file.clone();
        let provider = self
            .database(move |db| db.provider_preview(id, &file_copy))
            .await?;
        provider
            .map(|(path, revision)| {
                let key = serde_json::to_string(&(
                    "medal-preview-320x180-v1",
                    fingerprint(id, file)?,
                    &revision,
                ))
                .map_err(|e| e.to_string())?;
                Ok((key, path, revision))
            })
            .transpose()
    }
    // Disk hits do not acquire the extraction ticket or wait for the media worker.
    // Every lookup still resolves the library ID against a selected root and live file identity.
    pub(crate) async fn cached_thumbnail(
        self: &Arc<Self>,
        id: i64,
    ) -> Result<Option<Vec<u8>>, String> {
        let before = self.database(move |db| db.resolve_file(id)).await?.0;
        if before.modified_ns.is_none() {
            return Ok(None);
        }
        let key = match self.provider_image(id, &before).await? {
            Some((key, _, _)) => {
                let lookup = key.clone();
                if self
                    .thumbnails
                    .cache(move |cache| cache.failed(&lookup))
                    .await?
                {
                    fingerprint(id, &before)?
                } else {
                    key
                }
            }
            None => fingerprint(id, &before)?,
        };
        let bytes = self.thumbnails.cache(move |cache| cache.get(&key)).await?;
        let Some(bytes) = bytes.filter(|bytes| valid_image(bytes)) else {
            return Ok(None);
        };
        let after = self.database(move |db| db.resolve_file(id)).await?.0;
        Ok((before == after).then_some(bytes))
    }

    pub(crate) async fn thumbnail(
        self: &Arc<Self>,
        id: i64,
        token: String,
        executable: PathBuf,
        analysis: Arc<Analysis>,
    ) -> Result<Option<Vec<u8>>, String> {
        self.thumbnail_with_fallback(id, token, executable, analysis, true)
            .await
    }

    pub(crate) async fn thumbnail_with_fallback(
        self: &Arc<Self>,
        id: i64,
        token: String,
        executable: PathBuf,
        analysis: Arc<Analysis>,
        video_fallback: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        let Some(mut ticket) = self.thumbnails.begin(token.clone(), analysis.clone())? else {
            return Ok(None);
        };
        let (before, duration) = self
            .database(move |db| {
                let (file, _) = db.resolve_file(id)?;
                let info = db.cached(id, &file)?;
                let duration = info.and_then(|m| m.duration());
                Ok((file, duration))
            })
            .await?;
        if *ticket.cancel.borrow() {
            return Ok(None);
        }
        if let Some((provider_key, path, revision)) = self.provider_image(id, &before).await? {
            let lookup = provider_key.clone();
            if let Some(bytes) = self
                .thumbnails
                .cache(move |cache| cache.get(&lookup))
                .await?
                .filter(|b| valid_image(b))
            {
                if self.database(move |db| db.resolve_file(id)).await?.0 == before
                    && !*ticket.cancel.borrow()
                {
                    return Ok(Some(bytes));
                }
                return Ok(None);
            }
            let lookup = provider_key.clone();
            let failed = self
                .thumbnails
                .cache(move |cache| cache.failed(&lookup))
                .await?;
            let path_copy = path.clone();
            let current = tauri::async_runtime::spawn_blocking(move || {
                super::medal::paths::image(&path_copy)
            })
            .await
            .map_err(|_| "Thumbnail validation stopped")?;
            if !failed && current == Some((path.clone(), revision.clone())) {
                match generate(&mut ticket, executable.clone(), PathBuf::from(&path), None).await {
                    Ok(Some(bytes)) => {
                        let source = path.clone();
                        let unchanged = tauri::async_runtime::spawn_blocking(move || {
                            super::medal::paths::image(&source)
                        })
                        .await
                        .map_err(|_| "Thumbnail validation stopped")?;
                        if unchanged != Some((path, revision)) {
                            return Ok(None);
                        }
                        if self.database(move |db| db.resolve_file(id)).await?.0 != before {
                            return Ok(None);
                        }
                        let image = bytes.clone();
                        self.thumbnails
                            .cache(move |cache| cache.put(&provider_key, &image))
                            .await?;
                        return Ok(Some(bytes));
                    }
                    Ok(None) => return Ok(None),
                    Err(_) => {
                        self.thumbnails
                            .cache(move |cache| cache.fail(&provider_key))
                            .await?;
                    }
                }
            }
        }
        if !video_fallback {
            return Ok(None);
        }
        let key = fingerprint(id, &before)?;
        let cache_key = key.clone();
        let cached = if before.modified_ns.is_some() {
            self.thumbnails
                .cache(move |cache| cache.get(&cache_key))
                .await?
        } else {
            None
        };
        if let Some(bytes) = cached.filter(|b| valid_image(b)) {
            let after = self.database(move |db| db.resolve_file(id)).await?.0;
            if before != after || *ticket.cancel.borrow() {
                return Ok(None);
            }
            return Ok(Some(bytes));
        }
        let seconds = duration
            .filter(|v| v.is_finite() && *v > 0.0)
            .map_or(0.0, |v| (v * 0.1).min(5.0));
        let Some(bytes) = generate(
            &mut ticket,
            executable,
            PathBuf::from(&before.path),
            Some(seconds),
        )
        .await?
        else {
            return Ok(None);
        };
        let after = self.database(move |db| db.resolve_file(id)).await?.0;
        if before != after {
            return Err("Recording changed during thumbnail extraction. Rescan its folder.".into());
        }
        if *ticket.cancel.borrow() {
            return Ok(None);
        }
        let image = bytes.clone();
        if before.modified_ns.is_some() {
            self.thumbnails
                .cache(move |cache| cache.put(&key, &image))
                .await?;
        }
        Ok(Some(bytes))
    }
}

#[tauri::command]
pub async fn library_thumbnail(
    app: tauri::AppHandle,
    library: State<'_, super::LibraryState>,
    recordings: State<'_, crate::commands::RecordingState>,
    file_id: i64,
    request_id: String,
    cached_only: Option<bool>,
) -> Result<tauri::ipc::Response, String> {
    let library = library.get()?;
    let _operation = library.operation().await?;
    if cached_only.unwrap_or(false) {
        return Ok(tauri::ipc::Response::new(
            library.cached_thumbnail(file_id).await?.unwrap_or_default(),
        ));
    }
    let bytes = library
        .thumbnail(
            file_id,
            request_id,
            config::ffmpeg(&app)?,
            recordings.analysis.clone(),
        )
        .await?;
    Ok(tauri::ipc::Response::new(bytes.unwrap_or_default()))
}
#[tauri::command]
pub fn library_thumbnail_cancel(
    library: State<'_, super::LibraryState>,
    recordings: State<'_, crate::commands::RecordingState>,
    request_id: String,
) -> Result<(), String> {
    library
        .get()?
        .thumbnails
        .cancel(&request_id, &recordings.analysis);
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/thumbnails/mod.rs"]
mod tests;
