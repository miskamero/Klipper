//! Opt-in Windows check of the actual embedded player and packaged DLL closure.
use super::*;
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct Surface(HWND);

#[test]
#[ignore = "requires registered libmpv; uses only synthetic local files and null audio/video outputs"]
fn disguised_playlists_do_not_open_unselected_files() {
    crate::config::secure_runtime();
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("unselected.wav");
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&8036u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&[128; 8000]);
    std::fs::write(&target, &wav).unwrap();
    for contents in [
        format!("#EXTM3U\n{}\n", target.display()),
        format!("# mpv EDL v0\n{}\n", target.display()),
    ] {
        let selected = directory.path().join("disguised.mp4");
        std::fs::write(&selected, contents).unwrap();
        unsafe {
            let player = Player::new(null_mut()).unwrap();
            player.send(&["set", "vo", "null"]).unwrap();
            player.send(&["set", "ao", "null"]).unwrap();
            player.send(&["set", "pause", "yes"]).unwrap();
            player
                .send(&["loadfile", selected.to_str().unwrap(), "replace"])
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut ended = false;
            while Instant::now() < deadline {
                let event = &*(player.wait_event)(player.context, 0.1);
                assert_ne!(event.id, 8, "disguised playlist loaded referenced media");
                if event.id == 7 {
                    ended = true;
                    break;
                }
            }
            assert!(ended, "disguised input did not finish within deadline");
        }
    }
    assert_eq!(std::fs::read(target).unwrap(), wav);
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

unsafe fn wait_for(player: &Player, description: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        // mpv's video reconfiguration can send messages to the owning window thread.
        let mut message = std::mem::zeroed();
        while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        for _ in 0..128 {
            if (*(player.wait_event)(player.context, 0.0)).id == 0 {
                break;
            }
        }
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "Timed out waiting for {description}; codec={:?}, vo={:?}, hwdec={:?}",
        player.get("video-codec"),
        player.get("current-vo"),
        player.get("hwdec-current")
    );
}

#[tokio::test]
#[ignore = "requires registered KlinoffBuilds packages and a Windows graphics session; generates only temporary media"]
async fn klinoff_player_loads_local_dlls_decodes_and_seeks() {
    crate::config::secure_runtime();
    let directory = tempfile::tempdir().unwrap();
    let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../native-deps/windows-x64-klinoff-rc1/ffmpeg/ffmpeg.exe");
    // Keep the sender alive; capture treats a closed cancellation channel as cancellation.
    let (sender, mut cancel) = tokio::sync::watch::channel(false);
    for codec in ["libx264", "libx265"] {
        let path = directory.path().join(format!("synthetic-{codec}.mp4"));
        let mut command = tokio::process::Command::new(&ffmpeg);
        command.args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=30",
            "-t",
            "3",
            "-c:v",
            codec,
            "-preset",
            "ultrafast",
            "-threads",
            "2",
            "-pix_fmt",
            "yuv420p",
        ]);
        if codec == "libx265" {
            command.args(["-x265-params", "pools=2:frame-threads=1:log-level=error"]);
        }
        command.arg(&path);
        let (_, status) = crate::media_process::capture(
            &mut command,
            &mut cancel,
            Duration::from_secs(30),
            8192,
            "FFmpeg",
            true,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(status.success());
        let before = std::fs::read(&path).unwrap();
        unsafe {
            let surface = Surface(CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("Klipper synthetic playback"),
                WS_OVERLAPPED,
                0,
                0,
                320,
                180,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            ));
            assert!(!surface.0.is_null(), "Could not create test video surface");
            let player = Player::new(surface.0).unwrap();
            player.send(&["set", "pause", "yes"]).unwrap();
            player.send(&["loadfile", path.to_str().unwrap()]).unwrap();
            wait_for(&player, "video decode", || {
                player.get("video-codec").is_some()
                    && player.get("current-vo").as_deref() == Some("gpu-next")
            });
            player.send(&["seek", "1.5", "absolute+exact"]).unwrap();
            wait_for(&player, "exact seek", || {
                player
                    .get("time-pos")
                    .and_then(|v| v.parse::<f64>().ok())
                    .is_some_and(|v| (v - 1.5).abs() < 0.1)
            });
            println!(
                "{codec}: mpv={:?}, vo={:?}, hwdec={:?}, seek={:?}",
                player.get("mpv-version"),
                player.get("current-vo"),
                player.get("hwdec-current"),
                player.get("time-pos")
            );
            player.send(&["set", "hwdec", "no"]).unwrap();
            player.send(&["seek", "1.0", "absolute+exact"]).unwrap();
            wait_for(&player, "software decoding fallback", || {
                player.get("hwdec-current").as_deref() == Some("no")
                    && player
                        .get("time-pos")
                        .and_then(|v| v.parse::<f64>().ok())
                        .is_some_and(|v| (v - 1.0).abs() < 0.1)
            });
            println!("{codec}: forced software decoding and seek passed");
            // Assert where Windows actually loaded the complete non-system closure.
            let expected_directory = library_path()
                .unwrap()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap();
            for name in [
                "libmpv-2.dll",
                "libgcc_s_seh-1.dll",
                "libwinpthread-1.dll",
                "libstdc++-6.dll",
                "libshaderc_shared.dll",
                "libspirv-cross-c-shared.dll",
                "vulkan-1.dll",
            ] {
                let module = GetModuleHandleW(wide(name).as_ptr());
                assert!(!module.is_null(), "Module not loaded: {name}");
                let mut filename = vec![0u16; 32768];
                let length =
                    GetModuleFileNameW(module, filename.as_mut_ptr(), filename.len() as u32);
                assert!(length > 0);
                let loaded = PathBuf::from(String::from_utf16_lossy(&filename[..length as usize]))
                    .canonicalize()
                    .unwrap();
                assert_eq!(
                    loaded,
                    expected_directory.join(name),
                    "Unexpected DLL location for {name}"
                );
            }
            player.send(&["stop"]).unwrap();
            drop(player);
        }
        assert_eq!(before, std::fs::read(path).unwrap());
    }
    drop(sender);
}
