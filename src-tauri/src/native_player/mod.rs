//! Embedded Windows playback: one native worker, an owned video surface and fullscreen controls.
mod api;
mod cursor;
mod fullscreen;
pub(crate) mod overlay;
use api::Player;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{sync_channel, SyncSender},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{Manager, State};
use windows_sys::Win32::{
    Foundation::*, System::SystemServices::SS_BLACKRECT, UI::WindowsAndMessaging::*,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Control {
    Load,
    LoadPaused,
    Reload,
    Pause(bool),
    Seek(f64),
    Speed(f64),
    Volume(f64),
    Mute(bool),
    Snapshot,
    Stop,
    Fullscreen(bool),
}
impl Control {
    fn allowed_from_overlay(&self) -> bool {
        matches!(
            self,
            Self::Pause(_)
                | Self::Seek(_)
                | Self::Speed(_)
                | Self::Volume(_)
                | Self::Mute(_)
                | Self::Snapshot
                | Self::Fullscreen(false)
        )
    }
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Seek(v) if !v.is_finite() || *v < 0.0 || *v > 604800.0 => {
                Err("Invalid seek".into())
            }
            Self::Speed(v) if ![0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0].contains(v) => {
                Err("Invalid speed".into())
            }
            Self::Volume(v) if !v.is_finite() || !(0.0..=100.0).contains(v) => {
                Err("Invalid volume".into())
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    id: u64,
    position: f64,
    duration: f64,
    paused: bool,
    speed: f64,
    volume: f64,
    muted: bool,
    hwdec: String,
    dropped: f64,
    error: String,
    fullscreen: bool,
    pointer: Option<fullscreen::Pointer>,
}
struct Request {
    id: u64,
    control: Control,
    path: Option<String>,
    reply: tokio::sync::oneshot::Sender<Result<Snapshot, String>>,
}
pub struct NativePlayer {
    sender: SyncSender<Request>,
    stopping: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    surface: usize,
    surface_id: AtomicU64,
    finished: AtomicBool,
    fullscreen: fullscreen::Fullscreen,
    overlay: Result<overlay::Overlay, String>,
    cursor: Mutex<cursor::Cursor>,
    cursor_hidden: AtomicBool,
}

// A native surface failure is visible in the UI instead of aborting application startup.
pub struct NativeState(pub Result<NativePlayer, String>);
impl NativeState {
    pub fn player(&self) -> Result<&NativePlayer, String> {
        self.0.as_ref().map_err(Clone::clone)
    }
}

pub fn check_library() -> Result<(), String> {
    if api::library_path()?.is_file() {
        Ok(())
    } else {
        Err(api::missing_dll_message())
    }
}
impl NativePlayer {
    pub fn start(window: &tauri::WebviewWindow) -> Result<Self, String> {
        let parent = window.hwnd().map_err(|e| e.to_string())?.0;
        // This function runs in Tauri setup, on the window thread. The child is initially hidden.
        let surface = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                windows_sys::w!("STATIC"),
                windows_sys::w!("Klipper native video"),
                WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN | WS_DISABLED | SS_BLACKRECT,
                0,
                0,
                1,
                1,
                parent,
                null_mut(),
                null_mut(),
                null(),
            )
        };
        if surface.is_null() {
            return Err("Could not create native video surface".into());
        }
        let surface = surface as usize;
        let (sender, receiver) = sync_channel::<Request>(8);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let worker = std::thread::spawn(move || {
            // No HWND ownership crosses threads: the worker only passes its numeric ID to mpv.
            let mut player: Option<Player> = None;
            let mut current = 0;
            let mut error = String::new();
            let mut loaded = false;
            let mut started = false;
            let mut load_requested = false;
            while !stop.load(Ordering::Acquire) {
                let Ok(request) = receiver.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                let result = (|| -> Result<Snapshot, String> {
                    unsafe {
                        if matches!(
                            request.control,
                            Control::Load | Control::LoadPaused | Control::Reload
                        ) {
                            if request.id < current {
                                return Err("Superseded recording".into());
                            }
                            if player.is_none() {
                                player = Some(Player::new(surface as HWND)?);
                            }
                            let p = player.as_ref().unwrap();
                            if current != request.id
                                || !load_requested
                                || matches!(request.control, Control::Reload)
                            {
                                current = request.id;
                                error.clear();
                                loaded = false;
                                started = false;
                                while (*(p.wait_event)(p.context, 0.0)).id != 0 {}
                                if matches!(request.control, Control::LoadPaused) {
                                    p.send(&["set", "pause", "yes"])?;
                                }
                                p.send(&[
                                    "loadfile",
                                    request.path.as_deref().ok_or("Missing authorized file")?,
                                    "replace",
                                ])?;
                                p.send(&[
                                    "set",
                                    "pause",
                                    if matches!(request.control, Control::LoadPaused) {
                                        "yes"
                                    } else {
                                        "no"
                                    },
                                ])?;
                                p.send(&["set", "speed", "1"])?;
                                load_requested = true;
                            }
                        }
                        if request.id != current {
                            return Err("Superseded recording".into());
                        }
                        let p = player.as_ref().ok_or("Native player is not loaded")?;
                        match request.control {
                            Control::Pause(value) => {
                                p.send(&["set", "pause", if value { "yes" } else { "no" }])?
                            }
                            Control::Seek(value) => {
                                p.send(&["seek", &value.to_string(), "absolute+exact"])?
                            }
                            Control::Speed(value) => {
                                p.send(&["set", "speed", &value.to_string()])?
                            }
                            Control::Volume(value) => {
                                p.send(&["set", "volume", &value.to_string()])?
                            }
                            Control::Mute(value) => {
                                p.send(&["set", "mute", if value { "yes" } else { "no" }])?
                            }
                            Control::Stop => {
                                p.send(&["stop"])?;
                                loaded = false;
                                load_requested = false;
                            }
                            _ => {}
                        }
                        for _ in 0..128 {
                            let event = &*(p.wait_event)(p.context, 0.0);
                            if event.id == 0 {
                                break;
                            }
                            if event.id == 6 {
                                started = true;
                            }
                            if event.id == 8 {
                                loaded = true;
                            }
                            if event.id == 7 && started && !event.data.is_null() {
                                // mpv_event_end_file begins with reason and error (both C int).
                                let data = event.data.cast::<i32>();
                                if *data == 4 {
                                    error = format!("Native decoding failed ({})", *data.add(1));
                                    loaded = false;
                                }
                            }
                            if event.error < 0 {
                                error = format!("Native playback command failed ({})", event.error);
                            }
                        }
                        let number = |name| {
                            p.get(name)
                                .and_then(|v| v.parse::<f64>().ok())
                                .filter(|v| v.is_finite())
                                .unwrap_or(0.0)
                        };
                        if !loaded || !load_requested {
                            return Ok(Snapshot {
                                id: current,
                                error: error.clone(),
                                ..Default::default()
                            });
                        }
                        Ok(Snapshot {
                            id: current,
                            position: number("time-pos"),
                            duration: number("duration"),
                            paused: p.get("pause").as_deref() != Some("no"),
                            speed: number("speed"),
                            volume: number("volume"),
                            muted: p.get("mute").as_deref() == Some("yes"),
                            hwdec: p.get("hwdec-current").unwrap_or_default(),
                            dropped: number("frame-drop-count"),
                            error: error.clone(),
                            fullscreen: false,
                            pointer: None,
                        })
                    }
                })();
                let _ = request.reply.send(result);
            }
            // Drop waits for libmpv's decoding/render/audio threads before Tauri destroys the HWND.
            drop(player);
        });
        Ok(Self {
            sender,
            stopping,
            worker: Mutex::new(Some(worker)),
            surface,
            surface_id: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            fullscreen: fullscreen::Fullscreen::default(),
            overlay: overlay::Overlay::new(window),
            cursor: Mutex::new(cursor::Cursor::default()),
            cursor_hidden: AtomicBool::new(false),
        })
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
    fn refresh_cursor(&self, window: &tauri::WebviewWindow) {
        let requested = self.cursor_hidden.load(Ordering::Acquire)
            && self
                .fullscreen
                .active(self.surface_id.load(Ordering::Acquire));
        self.cursor.lock().unwrap().update(window, requested);
    }
    pub fn shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
        self.finished.store(true, Ordering::Release);
    }
}
use std::ptr::{null, null_mut};

#[tauri::command]
pub async fn mpv_control(
    window: tauri::WebviewWindow,
    state: State<'_, NativeState>,
    recordings: State<'_, crate::commands::RecordingState>,
    id: u64,
    control: Control,
) -> Result<Snapshot, String> {
    if window.label() != "main"
        && (window.label() != overlay::LABEL || !control.allowed_from_overlay())
    {
        return Err("Control not allowed from this window".into());
    }
    let window = window
        .get_webview_window("main")
        .ok_or("Main window unavailable")?;
    control.validate()?;
    let state = state.player()?;
    if state.stopping.load(Ordering::Acquire) {
        return Err("Player is shutting down".into());
    }
    let (reply, response) = tokio::sync::oneshot::channel();
    let control = if let Control::Fullscreen(value) = control {
        if value {
            state.overlay.as_ref().map_err(Clone::clone)?;
        }
        recordings.with_selected(id, |_| Ok(()))?;
        fullscreen::set(&window, id, value).await?;
        Control::Snapshot
    } else {
        control
    };
    let is_stop = matches!(control, Control::Stop);
    if is_stop && state.fullscreen.active(id) {
        fullscreen::set(&window, id, false).await?;
    }
    let send = |path: Option<String>| {
        state
            .sender
            .try_send(Request {
                id,
                control,
                path,
                reply,
            })
            .map_err(|_| "Native player is busy or unavailable".to_string())
    };
    if is_stop {
        send(None)?;
    } else {
        recordings.with_selected(id, |path| {
            send(Some(
                path.to_str().ok_or("Unsupported filename")?.to_owned(),
            ))
        })?;
    }
    let mut snapshot = response
        .await
        .map_err(|_| "Native player stopped".to_string())??;
    snapshot.fullscreen = state.fullscreen.active(id);
    if snapshot.fullscreen {
        snapshot.pointer = fullscreen::pointer(&window);
    }
    if let Ok(overlay) = &state.overlay {
        overlay.publish(&snapshot)?;
    }
    // Refresh after focus/overlay changes even if the frontend's bounds did not change.
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            if let Ok(player) = target.state::<NativeState>().player() {
                player.refresh_cursor(&target);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(snapshot)
}

#[derive(Deserialize)]
pub struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    visible: bool,
    #[serde(default, rename = "cursorHidden")]
    cursor_hidden: bool,
    #[serde(default, rename = "controlsVisible")]
    controls_visible: bool,
}

// Reuse the existing stop/fullscreen/surface ownership checks. Navigation awaits this barrier
// before activating another selection; stale IDs cannot stop or hide a newer worker/surface.
pub(crate) async fn deactivate(window: &tauri::WebviewWindow, id: u64) -> Result<(), String> {
    if window.state::<NativeState>().player().is_err() {
        return Ok(());
    }
    let result = mpv_control(
        window.clone(),
        window.state::<NativeState>(),
        window.state::<crate::commands::RecordingState>(),
        id,
        Control::Stop,
    )
    .await;
    mpv_surface(
        window.clone(),
        window.state::<NativeState>(),
        window.state::<crate::commands::RecordingState>(),
        id,
        Bounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            visible: false,
            cursor_hidden: false,
            controls_visible: false,
        },
    )
    .await?;
    match result {
        Ok(_) => Ok(()),
        Err(error) if error == "Superseded recording" || error == "Native player is not loaded" => {
            Ok(())
        }
        Err(error) => Err(error),
    }
}
#[tauri::command]
pub async fn mpv_surface(
    window: tauri::WebviewWindow,
    state: State<'_, NativeState>,
    recordings: State<'_, crate::commands::RecordingState>,
    id: u64,
    bounds: Bounds,
) -> Result<(), String> {
    let state = state.player()?;
    if ![bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|v| v.is_finite() && v.abs() < 100000.0)
    {
        return Err("Invalid video bounds".into());
    }
    if bounds.visible {
        recordings.with_selected(id, |_| Ok(()))?;
        state.surface_id.store(id, Ordering::Release);
    } else if state.surface_id.load(Ordering::Acquire) != id {
        return Ok(());
    }
    let surface = state.surface;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let parent = window.hwnd().map_err(|e| e.to_string())?.0 as usize;
    let app = window.app_handle().clone();
    let target = window.clone();
    window
        .run_on_main_thread(move || unsafe {
            if !app
                .state::<NativeState>()
                .player()
                .is_ok_and(|state| state.surface_id.load(Ordering::Acquire) == id)
            {
                return;
            }
            let mut client = RECT::default();
            GetClientRect(parent as HWND, &mut client);
            let left = ((bounds.x * scale).round() as i32).clamp(0, client.right.max(0));
            let top = ((bounds.y * scale).round() as i32).clamp(0, client.bottom.max(0));
            let right = (((bounds.x + bounds.width) * scale).round() as i32)
                .clamp(left, client.right.max(left));
            let bottom = (((bounds.y + bounds.height) * scale).round() as i32)
                .clamp(top, client.bottom.max(top));
            SetWindowPos(
                surface as HWND,
                HWND_TOP,
                left,
                top,
                right - left,
                bottom - top,
                SWP_NOACTIVATE
                    | if bounds.visible && right > left && bottom > top {
                        SWP_SHOWWINDOW
                    } else {
                        SWP_HIDEWINDOW
                    },
            );
            if let Ok(player) = app.state::<NativeState>().player() {
                player
                    .cursor_hidden
                    .store(bounds.visible && bounds.cursor_hidden, Ordering::Release);
                if let Ok(overlay) = &player.overlay {
                    if player.fullscreen.active(id) && bounds.visible {
                        let _ = overlay.update(&target, bounds.controls_visible);
                    } else {
                        let _ = overlay.dismiss(&target);
                    }
                }
                player.refresh_cursor(&target);
            }
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_are_bounded() {
        assert!(Control::Volume(101.0).validate().is_err());
        assert!(Control::Seek(f64::NAN).validate().is_err());
        assert!(Control::Speed(99.0).validate().is_err());
        for value in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            assert!(Control::Speed(value).validate().is_ok());
        }
    }
    #[test]
    fn arbitrary_native_commands_are_rejected() {
        assert!(serde_json::from_str::<Control>(r#"{"kind":"command","value":"run"}"#).is_err());
    }
    #[test]
    fn controls_window_cannot_load_files_or_enter_fullscreen() {
        assert!(!Control::Load.allowed_from_overlay());
        assert!(!Control::LoadPaused.allowed_from_overlay());
        assert!(!Control::Reload.allowed_from_overlay());
        assert!(!Control::Stop.allowed_from_overlay());
        assert!(!Control::Fullscreen(true).allowed_from_overlay());
        assert!(Control::Fullscreen(false).allowed_from_overlay());
        assert!(Control::Seek(2.0).allowed_from_overlay());
    }
}
