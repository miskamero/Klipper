//! Opt-in integration test: generates its own one-second video in a temp directory.
//! Never reads existing recordings. Requires FFmpeg and FFprobe on PATH.
use std::{path::Path, process::Command};

#[tokio::test]
#[ignore = "requires FFmpeg and FFprobe on PATH; cargo test --test synthetic_media -- --ignored"]
async fn probes_generated_video_with_four_tracks_without_modification() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("synthetic [four] & tracks-ä.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "1:a",
            "-map",
            "1:a",
            "-map",
            "1:a",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:0",
            "title=Synthetic test",
            "-movflags",
            "+faststart",
        ])
        .arg(&file)
        .status()
        .expect("FFmpeg required for opt-in synthetic test");
    assert!(status.success());
    let before = std::fs::read(&file).unwrap();
    let info = klipper_lib::media::probe(Path::new("ffprobe"), &file)
        .await
        .unwrap();
    let json = serde_json::to_value(info).unwrap();
    assert_eq!(json["videoCodec"], "h264");
    assert_eq!(json["audio"].as_array().unwrap().len(), 4);
    assert_eq!(json["audio"][0]["tags"]["language"], "eng");
    assert_eq!(json["width"], 96);
    assert_eq!(json["height"], 64);
    assert_eq!(before, std::fs::read(&file).unwrap());

    let corrupt = directory.path().join("corrupt.mp4");
    std::fs::write(&corrupt, b"synthetic invalid container").unwrap();
    let error = klipper_lib::media::probe(Path::new("ffprobe"), &corrupt)
        .await
        .unwrap_err();
    assert!(error.contains("could not analyze"));
}
