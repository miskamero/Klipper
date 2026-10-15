mod analysis;
mod commands;
mod config;
pub mod media;

pub fn run() {
    use tauri::Manager;
    let mut exiting = false;
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::RecordingState::default())
        .invoke_handler(tauri::generate_handler![
            commands::select_recording,
            commands::analyze_recording
        ])
        .build(tauri::generate_context!())
        .expect("Unable to start Klipper")
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                let analysis = app.state::<commands::RecordingState>().analysis.clone();
                analysis.stop();
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
