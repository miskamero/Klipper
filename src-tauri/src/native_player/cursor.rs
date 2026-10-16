//! Cursor shape belongs to the window under the pointer, not a global ShowCursor counter.
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::POINT,
    UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, GetForegroundWindow, LoadCursorW, SetCursor, WindowFromPoint,
        GA_ROOT, IDC_ARROW,
    },
};

#[derive(Default)]
pub(super) struct Cursor {
    hidden: bool,
}

impl Cursor {
    // Run on the main window's thread. A child HWND and the controls WebView can invalidate
    // Tao's cached IN_WINDOW flag; SetCursor does not depend on that flag or alter ShowCursor.
    pub fn update(&mut self, window: &tauri::WebviewWindow, requested: bool) {
        let Ok(main) = window.hwnd() else { return };
        let mut point = POINT::default();
        unsafe {
            if GetCursorPos(&mut point) == 0 {
                return;
            }
            let over_main = GetAncestor(WindowFromPoint(point), GA_ROOT) == main.0;
            let in_video = over_main && super::fullscreen::pointer(window).is_some();
            let focused = GetForegroundWindow() == main.0;
            if should_hide(requested, in_video, focused) {
                SetCursor(null_mut());
                self.hidden = true;
            } else {
                // Never change another application's cursor, including on another monitor.
                if self.hidden && over_main {
                    SetCursor(LoadCursorW(null_mut(), IDC_ARROW));
                }
                self.hidden = false;
            }
        }
    }
}

fn should_hide(requested: bool, in_video: bool, focused: bool) -> bool {
    requested && in_video && focused
}

#[cfg(test)]
mod tests {
    use super::should_hide;

    #[test]
    fn idle_cursor_hiding_requires_the_focused_video_under_the_pointer() {
        assert!(should_hide(true, true, true));
        assert!(!should_hide(false, true, true)); // Controls visible or fullscreen exited.
        assert!(!should_hide(true, false, true)); // Other monitor, window or controls overlay.
        assert!(!should_hide(true, true, false)); // Player lost foreground focus.
    }
}
