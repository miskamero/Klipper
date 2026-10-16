use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backend {
    engine: &'static str,
    message: Option<String>,
}

#[tauri::command]
pub fn playback_backend(_app: tauri::AppHandle) -> Backend {
    #[cfg(windows)]
    {
        use tauri::Manager;
        let result = _app
            .state::<crate::native_player::NativeState>()
            .player()
            .and_then(|_| crate::native_player::check_library());
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
    #[cfg(not(windows))]
    {
        Backend {
            engine: "html",
            message: None,
        }
    }
}
