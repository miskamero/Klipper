use super::*;
use std::fs;

fn setup() -> (tempfile::TempDir, Arc<Library>, i64, PathBuf, PathBuf) {
    let temp = tempdir().unwrap();
    let root = temp.path().join("recordings");
    fs::create_dir(&root).unwrap();
    let path = files::local_directory(&root).unwrap().join("jpeg.mp4");
    fs::write(&path, b"synthetic input").unwrap();
    let database = temp.path().join("library.sqlite3");
    let mut db = crate::library::db::Db::open(&database).unwrap();
    let root = db.add_root(path.parent().unwrap()).unwrap();
    db.discover(root.id, 1, &files::describe(&path).unwrap())
        .unwrap();
    let id = db.query(Default::default()).unwrap().items[0].id;
    drop(db);
    let library = Library::open(&database, &temp.path().join("cache")).unwrap();
    let executable = temp
        .path()
        .join(format!("helper{}", std::env::consts::EXE_SUFFIX));
    let output = std::process::Command::new("rustc")
        .arg("--edition=2021")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/analysis/helper.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (temp, library, id, path, executable)
}

#[tokio::test]
async fn library_reset_reaps_active_thumbnail_work_and_does_not_interrupt_foreground_analysis() {
    let (_temp, library, id, path, executable) = setup();
    let slow = path.with_file_name("slow-reset.mp4");
    fs::rename(&path, &slow).unwrap();
    let source = slow.clone();
    library
        .database(move |db| {
            let root = db.roots()?[0].id;
            db.discover(root, 2, &files::describe(&source)?)?;
            Ok(())
        })
        .await
        .unwrap();
    let analysis = Arc::new(Analysis::default());
    let owner = library.clone();
    let process = analysis.clone();
    let tool = executable.clone();
    let worker = tokio::spawn(async move {
        owner
            .thumbnail(id, "reset-test".into(), tool, process)
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !slow.with_extension("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        library.reset(analysis.clone(), || async { Ok(()) }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(worker.await.unwrap().unwrap().is_none());
    assert!(analysis.is_idle());
    assert_eq!(
        library
            .database(|db| db.query(Default::default()))
            .await
            .unwrap()
            .total,
        0
    );
    fs::remove_file(slow.with_extension("started")).unwrap();
    let foreground = analysis.request(executable, slow.clone());
    tokio::time::timeout(Duration::from_secs(10), async {
        while !slow.with_extension("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    library
        .reset(analysis.clone(), || async { Ok(()) })
        .await
        .unwrap();
    assert!(
        !analysis.is_idle(),
        "independent foreground analysis was cancelled by Library reset"
    );
    analysis.supersede();
    assert!(foreground.await.unwrap().unwrap().is_none());
    assert_eq!(fs::read(&slow).unwrap(), b"synthetic input");
    assert!(!slow.parent().unwrap().join("overlap").exists());
    library.shutdown(&analysis).await;
    analysis.shutdown().await;
}

#[tokio::test]
async fn cached_images_survive_restart_invalidate_changed_inputs_and_never_bypass_roots() {
    let (temp, library, id, path, executable) = setup();
    let analysis = Arc::new(Analysis::default());
    let image = library
        .thumbnail(id, "first".into(), executable.clone(), analysis.clone())
        .await
        .unwrap()
        .unwrap();
    drop(library);
    let library = Library::open(
        &temp.path().join("library.sqlite3"),
        &temp.path().join("cache"),
    )
    .unwrap();
    let missing = temp.path().join("not-installed");
    assert_eq!(
        library
            .thumbnail(id, "cached".into(), missing.clone(), analysis.clone())
            .await
            .unwrap()
            .unwrap(),
        image
    );
    fs::write(&path, b"changed synthetic fixture with another size").unwrap();
    assert!(library.cached_thumbnail(id).await.unwrap().is_none());
    assert!(library
        .thumbnail(id, "changed".into(), missing.clone(), analysis.clone())
        .await
        .unwrap_err()
        .contains("PATH"));
    assert!(library
        .thumbnail(id, "new".into(), executable, analysis.clone())
        .await
        .unwrap()
        .is_some());
    fs::rename(&path, temp.path().join("outside.mp4")).unwrap();
    assert!(library
        .thumbnail(id, "missing".into(), missing.clone(), analysis.clone())
        .await
        .is_err());
    fs::rename(temp.path().join("outside.mp4"), &path).unwrap();
    library
        .database(|db| {
            let root = db.roots()?[0].id;
            db.remove_root(root)
        })
        .await
        .unwrap();
    assert!(library
        .thumbnail(id, "removed".into(), missing, analysis.clone())
        .await
        .is_err());
    library.shutdown(&analysis).await;
    analysis.shutdown().await;
}

#[tokio::test]
async fn disk_cache_hits_bypass_an_active_extraction_without_spawning_or_cancelling_it() {
    let (_temp, library, id, path, executable) = setup();
    let analysis = Arc::new(Analysis::default());
    let image = library
        .thumbnail(id, "warm".into(), executable.clone(), analysis.clone())
        .await
        .unwrap()
        .unwrap();
    let slow = path.with_file_name("slow-uncached.mp4");
    fs::write(&slow, b"synthetic slow fixture").unwrap();
    let source = slow.clone();
    let other_id = library
        .database(move |db| {
            let root = db.roots()?[0].id;
            db.discover(root, 2, &files::describe(&source)?)?;
            Ok(db
                .query(Default::default())?
                .items
                .into_iter()
                .find(|row| row.name == "slow-uncached.mp4")
                .unwrap()
                .id)
        })
        .await
        .unwrap();
    let worker = library.clone();
    let owner = analysis.clone();
    let extraction = tokio::spawn(async move {
        worker
            .thumbnail(other_id, "blocked".into(), executable, owner)
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !slow.with_extension("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let cached = tokio::time::timeout(Duration::from_secs(2), library.cached_thumbnail(id))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(cached, image);
    assert!(!analysis.is_idle());
    assert!(!library.thumbnails.is_idle());
    library.thumbnails.cancel("blocked", &analysis);
    assert!(extraction.await.unwrap().unwrap().is_none());
    analysis.shutdown().await;
    assert!(analysis.is_idle());
    assert!(library.thumbnails.is_idle());
}

#[tokio::test]
async fn shutdown_cancels_active_thumbnail_and_waits_for_process_cleanup() {
    let (_temp, library, id, path, executable) = setup();
    let slow = path.with_file_name("slow-shutdown.mp4");
    fs::rename(&path, &slow).unwrap();
    let scan = slow.clone();
    library
        .database(move |db| {
            let root = db.roots()?[0].id;
            db.discover(root, 2, &files::describe(&scan)?).map(|_| ())
        })
        .await
        .unwrap();
    let analysis = Arc::new(Analysis::default());
    let worker = library.clone();
    let owner = analysis.clone();
    let request = tokio::spawn(async move {
        worker
            .thumbnail(id, "shutdown".into(), executable, owner)
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while !slow.with_extension("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), library.shutdown(&analysis))
        .await
        .unwrap();
    assert!(request.await.unwrap().unwrap().is_none());
    assert!(library.thumbnails.is_idle());
    analysis.shutdown().await;
    assert!(analysis.is_idle());
}
