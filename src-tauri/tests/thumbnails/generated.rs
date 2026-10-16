use super::*;
use std::{fs, process::Command, time::Instant};

#[tokio::test]
#[ignore = "generates synthetic H.264/HEVC media; requires FFmpeg and FFprobe on PATH"]
async fn extracts_real_frames_and_reuses_cache_without_changing_sources() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("recordings");
    fs::create_dir(&root).unwrap();
    for (name, size, codec, filter) in [
        ("h264", "1280x720", "libx264", "setsar=1"),
        ("hevc-portrait", "720x1280", "libx265", "setsar=1"),
        ("anamorphic", "640x360", "libx264", "setsar=2"),
    ] {
        let path = root.join(format!("{name}.mp4"));
        let mut command = Command::new("ffmpeg");
        command
            .args(["-v", "error", "-nostdin", "-n", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={size}:rate=30"))
            .args([
                "-t",
                "2",
                "-vf",
                filter,
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
        assert!(command.arg(&path).status().unwrap().success());
    }
    let database = temp.path().join("library.sqlite3");
    let cache = temp.path().join("cache");
    let library = Library::open(&database, &cache).unwrap();
    let selected = files::local_directory(&root).unwrap();
    library
        .database(move |db| {
            let root = db.add_root(&selected)?;
            for item in fs::read_dir(&selected).map_err(|e| e.to_string())? {
                let file = files::describe(&item.map_err(|e| e.to_string())?.path())?;
                db.discover(root.id, 1, &file)?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let rows = library
        .database(|db| db.query(Default::default()))
        .await
        .unwrap()
        .items;
    let analysis = Arc::new(Analysis::default());
    for row in rows {
        let original = fs::read(&row.path).unwrap();
        let start = Instant::now();
        let image = library
            .thumbnail(
                row.id,
                format!("extract-{}", row.id),
                "ffmpeg".into(),
                analysis.clone(),
            )
            .await
            .unwrap()
            .unwrap();
        let cold = start.elapsed();
        let start = Instant::now();
        let cached = library
            .thumbnail(
                row.id,
                format!("cached-{}", row.id),
                "missing-ffmpeg".into(),
                analysis.clone(),
            )
            .await
            .unwrap()
            .unwrap();
        println!(
            "{}: extraction={}ms cached={}ms JPEG={} bytes",
            row.name,
            cold.as_millis(),
            start.elapsed().as_millis(),
            image.len()
        );
        assert_eq!(image, cached);
        let thumbnail = temp.path().join(format!("{}.jpg", row.id));
        fs::write(&thumbnail, &image).unwrap();
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=width,height,sample_aspect_ratio",
                "-of",
                "json",
            ])
            .arg(&thumbnail)
            .output()
            .unwrap();
        assert!(output.status.success());
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let video = &metadata["streams"][0];
        let width = video["width"].as_u64().unwrap();
        let height = video["height"].as_u64().unwrap();
        assert!(width <= 320 && height <= 180);
        if row.name.starts_with("hevc") {
            assert!(height > width);
        }
        if row.name.starts_with("anamorphic") {
            assert_eq!((width, height), (320, 90));
        }
        assert_eq!(original, fs::read(&row.path).unwrap());
    }
    println!(
        "Cache file={} bytes",
        fs::metadata(cache.join("previews-v1.sqlite3"))
            .unwrap()
            .len()
    );
    library.shutdown(&analysis).await;
    analysis.shutdown().await;
}
