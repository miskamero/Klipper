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
    reply: oneshot::Sender<Reply>,
    background: bool,
}

#[derive(Default)]
struct State {
    pending: Option<Request>,
    cancel: Option<watch::Sender<bool>>,
    running: bool,
    stopped: bool,
    background_active: bool,
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
            let _ = pending.reply.send(Ok(None));
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
            reply,
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
                (request, cancelled)
            };
            let result = media::probe_cancellable(
                &request.executable,
                &request.path,
                &mut cancelled,
                self.timeout,
            )
            .await;
            // Serialize completion with selection/supersession, including genuine errors.
            let _state = self.state.lock().unwrap();
            let result = if *cancelled.borrow() {
                Ok(None)
            } else {
                result
            };
            let _ = request.reply.send(result);
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
            reply,
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
        if state.background_active {
            if let Some(cancel) = &state.cancel {
                cancel.send_replace(true);
            }
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|request| request.background)
        {
            if let Some(request) = state.pending.take() {
                let _ = request.reply.send(Ok(None));
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
