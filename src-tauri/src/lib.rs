mod commands;
mod config;
pub mod media;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::RecordingState::default())
        .invoke_handler(tauri::generate_handler![
            commands::select_recording,
            commands::analyze_recording
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start Klipper");
}
