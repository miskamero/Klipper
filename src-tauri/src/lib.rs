mod analysis;
mod commands;
mod config;
mod library;
pub mod media;
mod media_process;
#[cfg(windows)]
mod native_player;
mod playback;

pub fn run() {
    use tauri::Manager;
    let mut exiting = false;
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::RecordingState::default())
        .setup(|app| {
            app.manage(library::LibraryState(library::initialize(app.handle())));
            #[cfg(windows)]
            {
                let window = app
                    .get_webview_window("main")
                    .ok_or("Missing main window")?;
                app.manage(native_player::NativeState(
                    native_player::NativePlayer::start(&window),
                ));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window.app_handle().exit(0);
            }
        });
    #[cfg(not(windows))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::select_recording,
        commands::analyze_recording,
        playback::playback_backend,
        library::library_status,
        library::library_query,
        library::library_add_folder,
        library::library_rescan,
        library::library_cancel,
        library::library_remove_folder,
        library::library_open,
        library::thumbnails::library_thumbnail,
        library::thumbnails::library_thumbnail_cancel
    ]);
    #[cfg(windows)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::select_recording,
        commands::analyze_recording,
        playback::playback_backend,
        native_player::mpv_control,
        native_player::mpv_surface,
        native_player::overlay::mpv_overlay,
        library::library_status,
        library::library_query,
        library::library_add_folder,
        library::library_rescan,
        library::library_cancel,
        library::library_remove_folder,
        library::library_open,
        library::thumbnails::library_thumbnail,
        library::thumbnails::library_thumbnail_cancel
    ]);
    builder
        .build(tauri::generate_context!())
        .expect("Unable to start Klipper")
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                let analysis = app.state::<commands::RecordingState>().analysis.clone();
                analysis.stop();
                let library = app.state::<library::LibraryState>().get().ok();
                if let Some(library) = &library {
                    library.stop(&analysis);
                }
                let mut needs_cleanup = !analysis.is_idle()
                    || library
                        .as_ref()
                        .is_some_and(|l| !l.is_idle() || !l.thumbnails.is_idle());
                #[cfg(windows)]
                {
                    needs_cleanup |= app
                        .state::<native_player::NativeState>()
                        .player()
                        .is_ok_and(|player| !player.is_finished());
                }
                if needs_cleanup {
                    api.prevent_exit();
                    if exiting {
                        return;
                    }
                    exiting = true;
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        #[cfg(windows)]
                        {
                            let cleanup_app = app.clone();
                            let _ = tauri::async_runtime::spawn_blocking(move || {
                                if let Ok(player) =
                                    cleanup_app.state::<native_player::NativeState>().player()
                                {
                                    player.shutdown();
                                }
                            })
                            .await;
                        }
                        if let Some(library) = library {
                            library.shutdown(&analysis).await;
                        }
                        analysis.shutdown().await;
                        app.exit(code.unwrap_or(0));
                    });
                    return;
                }
            }
        });
}
