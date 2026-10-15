//! An owned transparent window composes over the native video without making the player transparent.
use super::{NativeState, Snapshot};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOACTIVATE};

pub(super) const LABEL: &str = "player-controls";

pub(super) struct Overlay {
    window: WebviewWindow,
    ready: AtomicBool,
    visible: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Presentation<'a> {
    snapshot: &'a Snapshot,
    visible: bool,
}

impl Overlay {
    pub fn new(parent: &WebviewWindow) -> Result<Self, String> {
        let window = WebviewWindowBuilder::new(
            parent.app_handle(),
            LABEL,
            WebviewUrl::App("index.html?native-controls=1".into()),
        )
        .title("Klipper playback controls")
        .owner(parent)
        .map_err(|e| e.to_string())?
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(false)
        .inner_size(800.0, 64.0)
        .build()
        .map_err(|e| e.to_string())?;
        Ok(Self {
            window,
            ready: AtomicBool::new(false),
            visible: AtomicBool::new(false),
        })
    }

    // Called on the UI thread with physical coordinates, including the current monitor's DPI.
    pub fn update(&self, parent: &WebviewWindow, visible: bool) -> Result<(), String> {
        self.visible.store(visible, Ordering::Release);
        if visible && self.ready.load(Ordering::Acquire) {
            let origin = parent.inner_position().map_err(|e| e.to_string())?;
            let size = parent.inner_size().map_err(|e| e.to_string())?;
            let scale = parent.scale_factor().map_err(|e| e.to_string())?;
            let margin = (16.0 * scale).round() as i32;
            let height = (64.0 * scale).round() as i32;
            unsafe {
                SetWindowPos(
                    self.window.hwnd().map_err(|e| e.to_string())?.0,
                    HWND_TOP,
                    origin.x + margin,
                    origin.y + size.height as i32 - margin - height,
                    (size.width as i32 - margin * 2).max(1),
                    height,
                    SWP_NOACTIVATE,
                );
            }
            // Visibility must go through Tauri in both directions: raw SWP_SHOWWINDOW bypasses
            // Tao's visibility flags and makes a later hide() a no-op. focused(false) prevents
            // automatic activation when this owned window is shown.
            self.window.show().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn publish(&self, snapshot: &Snapshot) -> Result<(), String> {
        if self.ready.load(Ordering::Acquire) {
            self.window
                .emit(
                    "native-controls-state",
                    Presentation {
                        snapshot,
                        visible: snapshot.fullscreen && self.visible.load(Ordering::Acquire),
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn dismiss(&self, parent: &WebviewWindow) -> Result<(), String> {
        self.visible.store(false, Ordering::Release);
        let focused = self.window.is_focused().unwrap_or(false);
        self.window.hide().map_err(|e| e.to_string())?;
        if focused {
            parent.set_focus().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Ready,
    Hidden { id: u64 },
    Activity { id: u64, keyboard: bool },
    Focus { id: u64 },
}

#[tauri::command]
pub async fn mpv_overlay(window: WebviewWindow, action: Action) -> Result<(), String> {
    let main = window
        .get_webview_window("main")
        .ok_or("Main window unavailable")?;
    let state = main.state::<NativeState>();
    let player = state.player()?;
    let overlay = player.overlay.as_ref().map_err(Clone::clone)?;
    match action {
        Action::Ready if window.label() == LABEL => {
            overlay.ready.store(true, Ordering::Release);
            let target = main.clone();
            main.run_on_main_thread(move || {
                if let Ok(player) = target.state::<NativeState>().player() {
                    if let Ok(overlay) = &player.overlay {
                        let _ = overlay.update(&target, overlay.visible.load(Ordering::Acquire));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        }
        Action::Hidden { id } if window.label() == LABEL => {
            let target = main.clone();
            main.run_on_main_thread(move || {
                if let Ok(player) = target.state::<NativeState>().player() {
                    if let Ok(overlay) = &player.overlay {
                        // Check after dispatch: movement may have revealed controls in the meantime.
                        if player.fullscreen.active(id) && !overlay.visible.load(Ordering::Acquire)
                        {
                            let _ = overlay.dismiss(&target);
                            player.refresh_cursor(&target);
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        }
        Action::Activity { id, keyboard } if window.label() == LABEL => {
            if player.fullscreen.active(id) {
                main.emit("native-controls-activity", keyboard)
                    .map_err(|e| e.to_string())?;
            }
        }
        Action::Focus { id } if window.label() == "main" => {
            let target = main.clone();
            main.run_on_main_thread(move || {
                if let Ok(player) = target.state::<NativeState>().player() {
                    if let Ok(overlay) = &player.overlay {
                        if player.fullscreen.active(id) {
                            let _ = overlay.update(&target, true);
                            let _ = overlay.window.set_focus();
                            let _ = overlay.window.emit("native-controls-focus", ());
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        }
        _ => return Err("Invalid controls-window request".into()),
    }
    Ok(())
}
