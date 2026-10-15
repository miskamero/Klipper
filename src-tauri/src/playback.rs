use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backend {
    engine: &'static str,
    message: Option<String>,
}

#[tauri::command]
pub fn playback_backend(_app: tauri::AppHandle) -> Backend {
    #[cfg(all(windows, feature = "mpv-spike"))]
    {
        use tauri::Manager;
        let result = _app
            .state::<crate::mpv_spike::NativeState>()
            .player()
            .and_then(|_| crate::mpv_spike::check_development_dll());
        return match result {
            Ok(()) => Backend {
                engine: "native",
                message: None,
            },
            Err(message) => Backend {
                engine: "unavailable",
                message: Some(message),
            },
        };
    }
    #[cfg(all(windows, not(feature = "mpv-spike")))]
    {
        Backend {
            engine: "unavailable",
            message: Some("Native playback is not enabled in this Windows build. Run npm run desktop from the repository to enable libmpv. HEVC cannot be played reliably by this machine's system player.".into()),
        }
    }
    #[cfg(not(windows))]
    {
        Backend {
            engine: "html",
            message: None,
        }
    }
}
