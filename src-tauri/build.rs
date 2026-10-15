fn main() {
    let commands: &'static [&'static str] = if std::env::var_os("CARGO_FEATURE_MPV_SPIKE").is_some()
    {
        &[
            "select_recording",
            "analyze_recording",
            "playback_backend",
            "mpv_control",
            "mpv_surface",
            "mpv_overlay",
        ]
    } else {
        &["select_recording", "analyze_recording", "playback_backend"]
    };
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("Unable to build application permissions");
}
