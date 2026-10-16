//! Window transitions stay on the owning UI thread; decoding never moves or restarts.
use serde::Serialize;
use std::sync::Mutex;
use tauri::Manager;
use windows_sys::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

#[derive(Default)]
pub(super) struct Fullscreen {
    // Also identifies which recording may dismiss fullscreen during unmount cleanup.
    saved: Mutex<Option<(u64, bool)>>,
}

#[derive(Clone, Serialize)]
pub(super) struct Pointer {
    x: f64,
    y: f64,
}

impl Fullscreen {
    pub fn active(&self, id: u64) -> bool {
        self.saved
            .lock()
            .unwrap()
            .is_some_and(|(owner, _)| owner == id)
    }

    fn transition(
        &self,
        window: &tauri::WebviewWindow,
        id: u64,
        enter: bool,
    ) -> Result<(), String> {
        let mut saved = self
            .saved
            .lock()
            .map_err(|_| "Fullscreen state unavailable")?;
        if enter {
            if saved.is_some() {
                return Ok(());
            }
            let maximized = window.is_maximized().map_err(|e| e.to_string())?;
            // Tao otherwise retains maximized work-area bounds on this Windows configuration.
            // Executing both operations on the window thread makes their ordering explicit.
            if maximized {
                window.unmaximize().map_err(|e| e.to_string())?;
            }
            if let Err(error) = window.set_fullscreen(true) {
                if maximized {
                    let _ = window.maximize();
                }
                return Err(error.to_string());
            }
            *saved = Some((id, maximized));
        } else if let Some((owner, maximized)) = *saved {
            if owner != id {
                return Ok(());
            }
            if let Ok(player) = window.state::<super::NativeState>().player() {
                if let Ok(overlay) = &player.overlay {
                    overlay.dismiss(window)?;
                }
                player.cursor.lock().unwrap().update(window, false);
            }
            window.set_fullscreen(false).map_err(|e| e.to_string())?;
            if maximized {
                window.maximize().map_err(|e| e.to_string())?;
            }
            *saved = None;
        }
        Ok(())
    }
}

pub(super) async fn set(window: &tauri::WebviewWindow, id: u64, enter: bool) -> Result<(), String> {
    let (reply, response) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let result = (|| {
                if enter {
                    target
                        .state::<crate::commands::RecordingState>()
                        .with_selected(id, |_| Ok(()))?;
                }
                target
                    .state::<super::NativeState>()
                    .player()?
                    .fullscreen
                    .transition(&target, id, enter)
            })();
            let _ = reply.send(result);
        })
        .map_err(|e| e.to_string())?;
    response
        .await
        .map_err(|_| "Fullscreen transition stopped".to_string())?
}

// Disabled native video children do not consistently deliver pointer events to WebView2.
// Only return cursor coordinates relative to this fullscreen window, never screen coordinates.
pub(super) fn pointer(window: &tauri::WebviewWindow) -> Option<Pointer> {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    let origin = window.inner_position().ok()?;
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    let x = point.x - origin.x;
    let y = point.y - origin.y;
    if x < 0 || y < 0 || x >= size.width as i32 || y >= size.height as i32 {
        return None;
    }
    Some(Pointer {
        x: x as f64 / scale,
        y: y as f64 / scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fullscreen_cleanup_is_owned_by_the_current_recording() {
        let state = Fullscreen::default();
        assert!(!state.active(1));
        *state.saved.lock().unwrap() = Some((2, true));
        assert!(!state.active(1));
        assert!(state.active(2));
    }
}
