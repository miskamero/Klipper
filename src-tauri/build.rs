fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["select_recording", "analyze_recording"]),
    ))
    .expect("Unable to build application permissions");
}
