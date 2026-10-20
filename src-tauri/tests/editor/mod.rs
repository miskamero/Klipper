use super::*;

#[test]
fn rejects_invalid_ranges_and_existing_destinations() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("original.mp4");
    std::fs::write(&file, b"original").unwrap();
    assert!(export::Destination::new(&file).is_err());
    assert!(export::Destination::new(&directory.path().join("new.mkv")).is_err());
    assert!(export::Destination::new(&directory.path().join("new.mp4")).is_ok());
    assert!(export::Destination::new(&directory.path().join("source.mp4:stream.mp4")).is_err());
    assert!(export::Destination::new(&directory.path().join("NUL.mp4")).is_err());
    assert_eq!(std::fs::read(file).unwrap(), b"original");
}

fn metadata() -> serde_json::Value {
    serde_json::json!({"format":{"start_time":"5.000000","duration":"4.000000"},"streams":[
        {"index":2,"codec_type":"video","codec_name":"hevc","pix_fmt":"yuv420p","width":160,"height":96,"time_base":"1/90000","start_time":"5","duration":"4","field_order":"progressive"},
        {"index":5,"codec_type":"audio","codec_name":"aac","channels":1,"sample_rate":"48000","start_time":"5.5","duration":"3.5"}
    ]})
}

#[test]
fn nvenc_uses_canonical_quality_options_supported_by_the_pinned_ffmpeg() {
    let mut output = Output::default();
    output.encoder = encoders::Encoder::Nvenc;
    let project = Project {
        output,
        version: 1,
        token: 1,
        recording_id: 1,
        revision: "synthetic".into(),
        start: 0.0,
        end: 1.0,
        info: media::parse(&serde_json::to_vec(&metadata()).unwrap()).unwrap(),
    };
    let args = export::arguments(Path::new("input.mp4"), Path::new("output.mp4"), &project);
    for option in ["-spatial-aq", "-temporal-aq"] {
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == option && pair[1] == "1"));
    }
    assert!(!args
        .iter()
        .any(|arg| arg == "-spatial_aq" || arg == "-temporal_aq"));
}

#[test]
fn validates_compatibility_and_keeps_absolute_stream_indices_and_offsets() {
    let mut data = metadata();
    let info = media::parse(&serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(info.video_index, 2);
    assert_eq!(info.audio[0].index, 5);
    assert_eq!(info.audio[0].start, 0.5);
    let project = Project {
        output: Output::default(),
        version: 1,
        token: 1,
        recording_id: 1,
        revision: "synthetic".into(),
        start: 0.2,
        end: 1.7,
        info,
    };
    let args = export::arguments(Path::new("source.mp4"), Path::new("output.mp4"), &project);
    let strings: Vec<_> = args.iter().map(|a| a.to_string_lossy()).collect();
    let graph = &strings[strings.iter().position(|s| s == "-filter_complex").unwrap() + 1];
    assert!(graph.contains("[0:2]trim=start_pts=468000:end_pts=603000"));
    assert!(graph.contains("[0:5]atrim=start=5.200000:end=6.700000"));
    assert!(graph.contains("asetpts=PTS-(5.200000)/TB"));
    assert!(range(
        &project,
        &Range {
            start: f64::NAN,
            end: 1.0
        }
    )
    .is_err());
    assert!(range(
        &project,
        &Range {
            start: 1.0,
            end: 1.0
        }
    )
    .is_err());
    data["streams"][0]["pix_fmt"] = "yuv420p10le".into();
    assert!(media::parse(&serde_json::to_vec(&data).unwrap())
        .unwrap_err()
        .contains("HDR"));
    data = metadata();
    data["streams"][1]["channels"] = 6.into();
    assert!(media::parse(&serde_json::to_vec(&data).unwrap()).is_err());
}

#[test]
fn publication_never_overwrites_a_file_created_after_the_save_dialog() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("new.mp4");
    let destination = export::Destination::new(&path).unwrap();
    std::fs::write(&path, b"someone else's file").unwrap();
    assert!(destination.verify().is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"someone else's file");
}

fn tool(exe: &str, args: &[&str]) -> Vec<u8> {
    let mut command = std::process::Command::new(exe);
    command.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
pub(super) fn make_source(path: &Path, hevc: bool, vfr: bool, offset: bool) {
    let mut args = vec![
        "-v",
        "error",
        "-nostdin",
        "-n",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x96:rate=30:duration=4",
    ];
    for tone in [
        "sine=frequency=440:sample_rate=48000:duration=4",
        "sine=frequency=880:sample_rate=48000:duration=4",
        "sine=frequency=1320:sample_rate=48000:duration=4",
        "sine=frequency=1760:sample_rate=48000:duration=3.5",
    ] {
        args.extend(["-f", "lavfi", "-i", tone]);
    }
    args.extend([
        "-map",
        "1:a",
        "-map",
        "0:v",
        "-map",
        "2:a",
        "-map",
        "3:a",
        "-map",
        "4:a",
        "-filter:a:3",
        "asetpts=PTS+0.5/TB",
        "-c:a",
        "aac",
        "-c:v",
        if hevc { "libx265" } else { "libx264" },
        "-preset",
        "ultrafast",
        "-threads",
        "2",
        "-pix_fmt",
        "yuv420p",
        "-fps_mode",
        "vfr",
    ]);
    if hevc {
        args.extend(["-x265-params", "pools=1:frame-threads=1:log-level=error"]);
    }
    let number_filter = if cfg!(windows) {
        "drawtext=fontfile='C\\:/Windows/Fonts/consola.ttf':text='%{n}':x=8:y=8:fontsize=20:fontcolor=white:box=1:boxcolor=black"
    } else {
        "drawtext=text='%{n}':x=8:y=8:fontsize=20:fontcolor=white:box=1:boxcolor=black"
    };
    let vf = if vfr {
        format!("{number_filter},select='if(lt(t,2),1,not(mod(n,2)))'")
    } else {
        number_filter.to_owned()
    };
    args.extend(["-vf", &vf]);
    if offset {
        args.extend(["-output_ts_offset", "5"]);
    }
    let text = path.to_str().unwrap();
    args.push(text);
    tool("ffmpeg", &args);
}
fn timestamps(path: &Path) -> Vec<f64> {
    let bytes = tool(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_frames",
            "-show_entries",
            "frame=best_effort_timestamp_time",
            "-of",
            "json",
            path.to_str().unwrap(),
        ],
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            f["best_effort_timestamp_time"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}
fn frame(path: &Path, n: usize) -> Vec<u8> {
    tool(
        "ffmpeg",
        &[
            "-v",
            "error",
            "-i",
            path.to_str().unwrap(),
            "-vf",
            &format!("select=eq(n\\,{n})"),
            "-frames:v",
            "1",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "pipe:1",
        ],
    )
}
fn audible_onset(path: &Path) -> f64 {
    let data = tool(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "a:3",
            "-read_intervals",
            "%+1",
            "-show_frames",
            "-show_entries",
            "frame=best_effort_timestamp_time",
            "-of",
            "json",
            path.to_str().unwrap(),
        ],
    );
    let frames: serde_json::Value = serde_json::from_slice(&data).unwrap();
    let pts = frames["frames"][0]["best_effort_timestamp_time"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let pcm = tool(
        "ffmpeg",
        &[
            "-v",
            "error",
            "-i",
            path.to_str().unwrap(),
            "-map",
            "0:a:3",
            "-t",
            "1",
            "-ac",
            "1",
            "-ar",
            "48000",
            "-f",
            "f32le",
            "pipe:1",
        ],
    );
    let samples: Vec<f32> = pcm
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    let block = samples
        .chunks(240)
        .position(|v| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt() > 0.04)
        .unwrap();
    pts + (block * 240) as f64 / 48000.0
}
async fn export_fixture(
    source: &Path,
    out: &Path,
    start: f64,
    end: f64,
    encoder: encoders::Encoder,
) -> (media::Info, Status) {
    let (_sender, mut cancel) = watch::channel(false);
    let info = media::inspect(Path::new("ffprobe"), source, &mut cancel)
        .await
        .unwrap();
    let start = media::boundary(Path::new("ffprobe"), source, &info, start, &mut cancel)
        .await
        .unwrap();
    let end = media::boundary(Path::new("ffprobe"), source, &info, end, &mut cancel)
        .await
        .unwrap();
    let project = Project {
        output: Output {
            encoder,
            ..Output::default()
        },
        version: 1,
        token: 1,
        recording_id: 1,
        revision: "synthetic".into(),
        start,
        end,
        info,
    };
    let status = Arc::new(Mutex::new(Status::default()));
    let began = std::time::Instant::now();
    export::run(
        Path::new("ffmpeg"),
        Path::new("ffprobe"),
        &files::describe(source).unwrap(),
        export::Destination::new(out).unwrap(),
        &project,
        &status,
        &mut cancel,
    )
    .await
    .unwrap();
    eprintln!(
        "synthetic export elapsed={:.3}s selected={:.6}s",
        began.elapsed().as_secs_f64(),
        end - start
    );
    let info = media::inspect(Path::new("ffprobe"), out, &mut cancel)
        .await
        .unwrap();
    let result = status.lock().unwrap().clone();
    (info, result)
}

#[tokio::test]
#[ignore = "generates only synthetic media; requires FFmpeg/FFprobe with libx264/libx265"]
async fn exports_h264_hevc_vfr_and_offset_sources_with_four_distinct_audio_tracks() {
    let directory = tempfile::tempdir().unwrap();
    let (_send, mut cancel) = watch::channel(false);
    let hardware = encoders::detect(Path::new("ffmpeg"), &mut cancel)
        .await
        .nvenc;
    for (hevc, vfr, offset, encoder) in [
        (false, false, false, encoders::Encoder::Cpu),
        (true, false, false, encoders::Encoder::Cpu),
        (false, true, true, encoders::Encoder::Cpu),
        (false, false, false, encoders::Encoder::Nvenc),
        (true, false, false, encoders::Encoder::Nvenc),
        (false, true, true, encoders::Encoder::Nvenc),
    ] {
        if encoder == encoders::Encoder::Nvenc && !hardware {
            eprintln!("NVENC fixture skipped: no verified encoder available");
            continue;
        }
        let source = directory
            .path()
            .join(format!("source-{hevc}-{vfr}-{encoder:?}.mp4"));
        make_source(&source, hevc, vfr, offset);
        let before = std::fs::read(&source).unwrap();
        let output = directory
            .path()
            .join(format!("export-{hevc}-{vfr}-{encoder:?}.mp4"));
        let (info, status) = export_fixture(&source, &output, 0.233, 3.1, encoder).await;
        assert_eq!(info.audio.len(), 4);
        assert!(
            (info.duration - 2.866667).abs() < 0.04,
            "duration {}",
            info.duration
        );
        eprintln!(
            "progress={:.6} duration={:.6}",
            status.seconds, info.duration
        );
        let input_pts = timestamps(&source);
        let output_pts = timestamps(&output);
        let origin = input_pts[0];
        let first = input_pts
            .iter()
            .position(|t| t - origin + 0.000001 >= 0.233)
            .unwrap();
        let last = input_pts
            .iter()
            .position(|t| t - origin + 0.000001 >= 3.1)
            .unwrap();
        assert_eq!(
            output_pts.len(),
            last - first,
            "VFR frames may not be duplicated or dropped"
        );
        for (i, pts) in output_pts.iter().enumerate() {
            assert!(
                (pts - (input_pts[first + i] - input_pts[first])).abs() < 0.00001,
                "frame {i} timing {pts}"
            );
        }
        for (input, output_frame) in [(first, 0), (last - 1, output_pts.len() - 1)] {
            let original = frame(&source, input);
            let encoded = frame(&output, output_frame);
            assert_eq!(original.len(), encoded.len());
            let mse = original
                .iter()
                .zip(encoded)
                .map(|(a, b)| (*a as f64 - b as f64).powi(2))
                .sum::<f64>()
                / original.len() as f64;
            let psnr = 10.0 * (255.0_f64.powi(2) / mse).log10();
            eprintln!("boundary frame {input} PSNR={psnr:.2}dB");
            assert!(
                psnr > 32.0,
                "wrong boundary frame or damaged export: {psnr}"
            );
        }
        let source_onset = audible_onset(&source);
        let output_onset = audible_onset(&output);
        let sync_error = (output_onset - (source_onset - input_pts[first])).abs();
        eprintln!("delayed audible onset error={sync_error:.6}s");
        assert!(
            sync_error < 0.006,
            "delayed audio content shifted by {sync_error}s"
        );
        for (index, frequency) in [440.0, 880.0, 1320.0, 1760.0].iter().enumerate() {
            let mapping = format!("0:a:{index}");
            let pcm = tool(
                "ffmpeg",
                &[
                    "-v",
                    "error",
                    "-i",
                    output.to_str().unwrap(),
                    "-map",
                    &mapping,
                    "-ss",
                    "1",
                    "-t",
                    "0.5",
                    "-ac",
                    "1",
                    "-ar",
                    "48000",
                    "-f",
                    "f32le",
                    "pipe:1",
                ],
            );
            let samples: Vec<f32> = pcm
                .chunks_exact(4)
                .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
                .collect();
            let crossings = samples
                .windows(2)
                .filter(|s| s[0] <= 0.0 && s[1] > 0.0)
                .count() as f64;
            let actual = crossings * 48000.0 / samples.len() as f64;
            assert!(
                (actual - frequency).abs() < 5.0,
                "track {index} frequency {actual}"
            );
        }
        assert_eq!(before, std::fs::read(&source).unwrap());
        assert_eq!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".klipper-export"))
                .count(),
            0
        );
    }
}

#[tokio::test]
#[ignore = "generates synthetic media; requires FFmpeg/FFprobe"]
async fn cancelled_and_failed_exports_leave_sources_and_destinations_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    make_source(&source, false, false, false);
    let before = std::fs::read(&source).unwrap();
    let (sender, mut cancel) = watch::channel(false);
    let info = media::inspect(Path::new("ffprobe"), &source, &mut cancel)
        .await
        .unwrap();
    let project = Project {
        output: Output::default(),
        version: 1,
        token: 1,
        recording_id: 1,
        revision: "test".into(),
        start: 0.0,
        end: 3.0,
        info,
    };
    let output = directory.path().join("cancelled.mp4");
    let status = Arc::new(Mutex::new(Status::default()));
    let cancel_task = async {
        while status.lock().unwrap().phase != "encoding" {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        sender.send_replace(true);
    };
    let source_info = files::describe(&source).unwrap();
    let (result, ()) = tokio::join!(
        export::run(
            Path::new("ffmpeg"),
            Path::new("ffprobe"),
            &source_info,
            export::Destination::new(&output).unwrap(),
            &project,
            &status,
            &mut cancel
        ),
        cancel_task
    );
    assert!(result.is_err());
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, ms: u32) -> u32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let pid = status.lock().unwrap().process_id.unwrap();
        unsafe {
            let handle = OpenProcess(0x00100000, 0, pid);
            if !handle.is_null() {
                assert_eq!(
                    WaitForSingleObject(handle, 0),
                    0,
                    "cancelled encoder is still running"
                );
                CloseHandle(handle);
            }
        }
    }
    assert!(!output.exists());
    sender.send_replace(false);
    assert!(export::run(
        &directory.path().join("absent-ffmpeg"),
        Path::new("ffprobe"),
        &files::describe(&source).unwrap(),
        export::Destination::new(&output).unwrap(),
        &project,
        &status,
        &mut cancel
    )
    .await
    .is_err());
    assert!(!output.exists());
    assert_eq!(before, std::fs::read(source).unwrap());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn interrupted_output_recovery_removes_only_the_recorded_owned_identity() {
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("managed");
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"source unchanged").unwrap();
    let owned = directory
        .path()
        .canonicalize()
        .unwrap()
        .join(".klipper-export-test.mp4");
    std::fs::write(&owned, b"interrupted synthetic output").unwrap();
    let id = files::identity(&owned).unwrap();
    let receipt = recovery::Recovery::create(&cache, &owned, &id).unwrap();
    std::mem::forget(receipt);
    recovery::recover(&cache).unwrap();
    assert!(!owned.exists());
    assert_eq!(std::fs::read(&source).unwrap(), b"source unchanged");
    assert!(!cache.join("pending-output.json").exists());
    std::fs::write(&owned, b"old identity").unwrap();
    let receipt =
        recovery::Recovery::create(&cache, &owned, &files::identity(&owned).unwrap()).unwrap();
    std::mem::forget(receipt);
    std::fs::rename(&owned, directory.path().join("previous-owned.mp4")).unwrap();
    std::fs::write(&owned, b"different file must be preserved").unwrap();
    assert!(recovery::recover(&cache).is_err());
    assert_eq!(
        std::fs::read(&owned).unwrap(),
        b"different file must be preserved"
    );
}

#[tokio::test]
async fn session_tokens_and_shutdown_cancel_only_owned_work() {
    let state = EditorState::default();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic.mp4");
    std::fs::write(&path, b"fixture").unwrap();
    let (cancel, receiver) = watch::channel(false);
    *state.0.session.lock().unwrap() = Some(Session {
        token: 7,
        source: files::describe(&path).unwrap(),
        project: None,
        cancel,
        destination: None,
    });
    assert!(state.0.session(999, |_| Ok(())).is_err());
    assert!(!*receiver.borrow());
    state.0.shutdown().await;
    assert!(*receiver.borrow());
    assert!(state.0.session(7, |_| Ok(())).is_err());
    assert!(state.0.is_idle());
}
