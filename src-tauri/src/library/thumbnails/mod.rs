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
    pub fn new(directory: &Path) -> Arc<Self> {
        Arc::new(Self {
            directory: directory.into(),
            cache: Mutex::new(None),
            state: Mutex::default(),
            idle: watch::channel(true).0,
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

impl Library {
    pub(crate) async fn thumbnail(
        self: &Arc<Self>,
        id: i64,
        token: String,
        executable: PathBuf,
        analysis: Arc<Analysis>,
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
        let bytes = loop {
            if *ticket.cancel.borrow() || analysis.is_stopped() {
                return Ok(None);
            }
            if let Some(mut reply) = analysis.try_thumbnail(
                executable.clone(),
                PathBuf::from(&before.path),
                seconds,
                token.clone(),
            ) {
                let result = tokio::select! {
                    value=&mut reply=>value,
                    _=async {let _=ticket.cancel.wait_for(|v| *v).await;}=>{
                        analysis.cancel_thumbnail(&token);
                        reply.await
                    }
                }
                .map_err(|_| "Thumbnail worker stopped")??;
                if *ticket.cancel.borrow() {
                    return Ok(None);
                }
                if let Some(bytes) = result {
                    break bytes;
                }
            }
            tokio::select! {
                _=ticket.cancel.wait_for(|v| *v)=>return Ok(None),
                _=tokio::time::sleep(Duration::from_millis(100))=>{}
            }
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
) -> Result<tauri::ipc::Response, String> {
    let bytes = library
        .get()?
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
