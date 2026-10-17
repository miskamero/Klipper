fn main() {
    let commands: &'static [&'static str] =
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            &[
                "select_recording",
                "analyze_recording",
                "activate_recording",
                "deactivate_recording",
                "playback_backend",
                "mpv_control",
                "mpv_surface",
                "mpv_overlay",
                "library_status",
                "library_query",
                "library_reset",
                "library_add_folder",
                "library_rescan",
                "library_cancel",
                "library_remove_folder",
                "library_open",
                "library_thumbnail",
                "library_thumbnail_cancel",
                "medal_select",
                "medal_preview",
                "medal_commit",
                "medal_cancel",
                "medal_status",
            ]
        } else {
            &[
                "select_recording",
                "analyze_recording",
                "activate_recording",
                "deactivate_recording",
                "playback_backend",
                "library_status",
                "library_query",
                "library_reset",
                "library_add_folder",
                "library_rescan",
                "library_cancel",
                "library_remove_folder",
                "library_open",
                "library_thumbnail",
                "library_thumbnail_cancel",
                "medal_select",
                "medal_preview",
                "medal_commit",
                "medal_cancel",
                "medal_status",
            ]
        };
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("Unable to build application permissions");
}
