//! One process owner and one replaceable pending request. IPC callers never own the child.
use crate::media;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{oneshot, watch};

type Reply = Result<Option<media::MediaInfo>, String>;

struct Request {
    executable: PathBuf,
    path: PathBuf,
    work: Work,
    background: bool,
}

enum Work {
    Probe(oneshot::Sender<Reply>),
    Thumbnail {
        token: String,
        seconds: f64,
        reply: oneshot::Sender<Result<Option<Vec<u8>>, String>>,
    },
}
impl Work {
    fn supersede(self) {
        match self {
            Self::Probe(reply) => {
                let _ = reply.send(Ok(None));
            }
            Self::Thumbnail { reply, .. } => {
                let _ = reply.send(Ok(None));
            }
        }
    }
    fn thumbnail_token(&self) -> Option<&str> {
        match self {
            Self::Thumbnail { token, .. } => Some(token),
            _ => None,
        }
    }
}

#[derive(Default)]
struct State {
    pending: Option<Request>,
    cancel: Option<watch::Sender<bool>>,
    running: bool,
    stopped: bool,
    background_active: bool,
    thumbnail_active: Option<String>,
}

pub(crate) struct Analysis {
    state: Mutex<State>,
    idle: watch::Sender<bool>,
    timeout: Duration,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
            idle: watch::channel(true).0,
            timeout: Duration::from_secs(30),
        }
    }
}

impl State {
    fn supersede(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.send_replace(true);
        }
        if let Some(pending) = self.pending.take() {
            pending.work.supersede();
        }
    }
}

impl Analysis {
    pub fn supersede(&self) {
        self.state.lock().unwrap().supersede();
    }

    pub fn request(
        self: &Arc<Self>,
        executable: PathBuf,
        path: PathBuf,
    ) -> oneshot::Receiver<Reply> {
        let (reply, receiver) = oneshot::channel();
        let mut state = self.state.lock().unwrap();
        state.supersede();
        if state.stopped {
            let _ = reply.send(Ok(None));
            return receiver;
        }
        state.pending = Some(Request {
            executable,
            path,
            work: Work::Probe(reply),
            background: false,
        });
        if !state.running {
            state.running = true;
            self.idle.send_replace(false);
            let worker = Arc::clone(self);
            // This task outlives a dropped IPC response and always finishes child cleanup.
            tauri::async_runtime::spawn(async move { worker.run().await });
        }
        receiver
    }

    async fn run(&self) {
        loop {
            let (request, mut cancelled) = {
                let mut state = self.state.lock().unwrap();
                state.cancel = None;
                let Some(request) = state.pending.take() else {
                    state.running = false;
                    self.idle.send_replace(true);
                    return;
                };
                let (cancel, cancelled) = watch::channel(false);
                state.cancel = Some(cancel);
                state.background_active = request.background;
                state.thumbnail_active = request.work.thumbnail_token().map(str::to_owned);
                (request, cancelled)
            };
            match request.work {
                Work::Probe(reply) => {
                    let result = media::probe_cancellable(
                        &request.executable,
                        &request.path,
                        &mut cancelled,
                        self.timeout,
                    )
                    .await;
                    let _state = self.state.lock().unwrap();
                    let _ = reply.send(if *cancelled.borrow() {
                        Ok(None)
                    } else {
                        result
                    });
                }
                Work::Thumbnail { seconds, reply, .. } => {
                    let result = crate::library::thumbnails::extract(
                        &request.executable,
                        &request.path,
                        seconds,
                        &mut cancelled,
                    )
                    .await;
                    let _state = self.state.lock().unwrap();
                    let _ = reply.send(if *cancelled.borrow() {
                        Ok(None)
                    } else {
                        result
                    });
                }
            }
        }
    }

    pub fn is_idle(&self) -> bool {
        *self.idle.borrow()
    }

    // No background queue: the scanner keeps its pending work in SQLite and retries when idle.
    // Foreground request()/supersede() still preempts and reaps this process through the same owner.
    pub fn try_background(
        self: &Arc<Self>,
        executable: PathBuf,
        path: PathBuf,
    ) -> Option<oneshot::Receiver<Reply>> {
        let mut state = self.state.lock().unwrap();
        if state.running || state.pending.is_some() || state.stopped {
            return None;
        }
        let (reply, response) = oneshot::channel();
        state.pending = Some(Request {
            executable,
            path,
            work: Work::Probe(reply),
            background: true,
        });
        state.running = true;
        self.idle.send_replace(false);
        let worker = Arc::clone(self);
        tauri::async_runtime::spawn(async move { worker.run().await });
        Some(response)
    }

    pub fn cancel_background(&self) {
        let mut state = self.state.lock().unwrap();
        if state.background_active && state.thumbnail_active.is_none() {
            if let Some(cancel) = &state.cancel {
                cancel.send_replace(true);
            }
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|request| request.background && request.work.thumbnail_token().is_none())
        {
            if let Some(request) = state.pending.take() {
                request.work.supersede();
            }
        }
    }

    pub fn try_thumbnail(
        self: &Arc<Self>,
        executable: PathBuf,
        path: PathBuf,
        seconds: f64,
        token: String,
    ) -> Option<oneshot::Receiver<Result<Option<Vec<u8>>, String>>> {
        let mut state = self.state.lock().unwrap();
        if state.running || state.stopped {
            return None;
        }
        let (reply, response) = oneshot::channel();
        state.pending = Some(Request {
            executable,
            path,
            background: true,
            work: Work::Thumbnail {
                token,
                seconds,
                reply,
            },
        });
        state.running = true;
        self.idle.send_replace(false);
        let worker = self.clone();
        tauri::async_runtime::spawn(async move { worker.run().await });
        Some(response)
    }

    pub fn cancel_thumbnail(&self, token: &str) {
        let mut state = self.state.lock().unwrap();
        if state.thumbnail_active.as_deref() == Some(token) {
            if let Some(cancel) = &state.cancel {
                cancel.send_replace(true);
            }
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|r| r.work.thumbnail_token() == Some(token))
        {
            if let Some(request) = state.pending.take() {
                request.work.supersede();
            }
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.state.lock().unwrap().stopped
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.stopped = true;
        state.supersede();
    }

    pub async fn shutdown(&self) {
        let mut idle = self.idle.subscribe();
        self.stop();
        let _ = idle.wait_for(|value| *value).await;
    }
}

#[cfg(test)]
#[path = "../tests/analysis/mod.rs"]
mod tests;
