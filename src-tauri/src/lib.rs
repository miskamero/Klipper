mod analysis;
mod commands;
mod config;
pub mod media;
#[cfg(all(windows, feature = "mpv-spike"))]
mod mpv_spike;
mod playback;

pub fn run() {
    use tauri::Manager;
    let mut exiting = false;
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::RecordingState::default());
    #[cfg(not(all(windows, feature = "mpv-spike")))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::select_recording,
        commands::analyze_recording,
        playback::playback_backend
    ]);
    #[cfg(all(windows, feature = "mpv-spike"))]
    let builder = builder
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or("Missing main window")?;
            // Playback availability must not prevent the shell or file inspection from opening.
            app.manage(mpv_spike::NativeState(mpv_spike::NativePlayer::start(
                &window,
            )));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::select_recording,
            commands::analyze_recording,
            playback::playback_backend,
            mpv_spike::mpv_control,
            mpv_spike::mpv_surface,
            mpv_spike::overlay::mpv_overlay
        ]);
    builder
        .build(tauri::generate_context!())
        .expect("Unable to start Klipper")
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                let analysis = app.state::<commands::RecordingState>().analysis.clone();
                analysis.stop();
                #[cfg(all(windows, feature = "mpv-spike"))]
                if app
                    .state::<mpv_spike::NativeState>()
                    .player()
                    .is_ok_and(|player| !player.is_finished())
                {
                    api.prevent_exit();
                    if exiting {
                        return;
                    }
                    exiting = true;
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let cleanup_app = app.clone();
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            if let Ok(player) =
                                cleanup_app.state::<mpv_spike::NativeState>().player()
                            {
                                player.shutdown();
                            }
                        })
                        .await;
                        analysis.shutdown().await;
                        app.exit(code.unwrap_or(0));
                    });
                    return;
                }
                if !analysis.is_idle() {
                    api.prevent_exit();
                    if !exiting {
                        exiting = true;
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            analysis.shutdown().await;
                            app.exit(code.unwrap_or(0));
                        });
                    }
                }
            }
        });
}
