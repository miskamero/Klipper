use super::*;
use crate::{analysis::Analysis, library::Library};
use std::{process::Command, time::Instant};

#[tokio::test]
#[ignore = "generates only synthetic video/images; requires FFmpeg on PATH"]
async fn provider_images_supersede_generated_cache_survive_restart_and_fall_back_safely() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("videos");
    fs::create_dir(&root_path).unwrap();
    let video = root_path.join("synthetic.mp4");
    assert!(Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=30",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p"
        ])
        .arg(&video)
        .status()
        .unwrap()
        .success());
    let image = temp.path().join("provider.png");
    assert!(Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "color=c=green:s=640x360",
            "-frames:v",
            "1",
            "-threads",
            "1"
        ])
        .arg(&image)
        .status()
        .unwrap()
        .success());
    let original = fs::read(&video).unwrap();
    let original_image = fs::read(&image).unwrap();
    let database = temp.path().join("library.sqlite3");
    let cache = temp.path().join("cache");
    let library = Library::open(&database, &cache).unwrap();
    let root_copy = root_path.clone();
    let video_copy = video.clone();
    let id = library
        .database(move |db| {
            let root = db.add_root(&files::local_directory(&root_copy)?)?;
            db.discover(
                root.id,
                1,
                &files::describe(&video_copy.canonicalize().unwrap())?,
            )?;
            Ok(db.query(Query::default())?.items[0].id)
        })
        .await
        .unwrap();
    let analysis = Arc::new(Analysis::default());
    let ffmpeg = PathBuf::from("ffmpeg");
    let generated = library
        .thumbnail(id, "generated".into(), ffmpeg.clone(), analysis.clone())
        .await
        .unwrap()
        .unwrap();
    async fn import(library: &Arc<Library>, video: &Path, image: &Path) {
        let bytes = fixture(video, image);
        library
            .database(move |db| {
                let cancel = AtomicBool::new(false);
                let done = AtomicUsize::new(0);
                let (plan, _) = store::prepare(
                    parse::parse(&bytes)?.entries,
                    db.medal_index("synthetic-source")?,
                    &cancel,
                    &done,
                )?;
                db.commit_medal(store::Commit {
                    source: "synthetic-source",
                    path: "synthetic.json",
                    revision: "v1",
                    prepared: &plan,
                    additions: &HashSet::new(),
                    cancel: &cancel,
                    done: &done,
                })
                .map(|_| ())
            })
            .await
            .unwrap();
    }
    import(&library, &video, &image).await;
    assert!(library.cached_thumbnail(id).await.unwrap().is_none());
    let start = Instant::now();
    let provider = library
        .thumbnail(id, "provider".into(), ffmpeg.clone(), analysis.clone())
        .await
        .unwrap()
        .unwrap();
    println!(
        "provider conversion={}ms JPEG={} bytes",
        start.elapsed().as_millis(),
        provider.len()
    );
    assert_ne!(provider, generated);
    assert!(crate::library::thumbnails::valid_image(&provider));
    drop(library);
    fs::rename(&image, temp.path().join("original-image-preserved.png")).unwrap();
    let library = Library::open(&database, &cache).unwrap();
    assert_eq!(
        library.cached_thumbnail(id).await.unwrap().unwrap(),
        provider
    );
    // Explicit metadata refresh must not discard an already managed preview merely because
    // Medal cleaned its original cache file. A changed video still invalidates that preview.
    import(&library, &video, &image).await;
    assert_eq!(
        library.cached_thumbnail(id).await.unwrap().unwrap(),
        provider
    );
    let corrupt = temp.path().join("corrupt.png");
    fs::write(&corrupt, b"not an image").unwrap();
    import(&library, &video, &corrupt).await;
    assert!(library.cached_thumbnail(id).await.unwrap().is_none());
    let fallback = library
        .thumbnail(id, "corrupt".into(), ffmpeg.clone(), analysis.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fallback, generated);
    assert_eq!(
        library.cached_thumbnail(id).await.unwrap().unwrap(),
        generated
    );
    // Unchanged invalid references reuse a bounded persisted failure entry instead of decoding again.
    import(&library, &video, &corrupt).await;
    assert_eq!(
        library
            .thumbnail(
                id,
                "unchanged".into(),
                PathBuf::from("not-installed-ffmpeg"),
                analysis.clone()
            )
            .await
            .unwrap()
            .unwrap(),
        generated
    );
    assert_eq!(fs::read(&video).unwrap(), original);
    assert_eq!(
        fs::read(temp.path().join("original-image-preserved.png")).unwrap(),
        original_image
    );
    library.shutdown(&analysis).await;
    analysis.shutdown().await;
}
