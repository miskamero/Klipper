use super::*;
use std::time::Duration;

async fn wait_idle(library: &Library) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !library.is_idle() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("scan failed to become idle");
}

fn helper(directory: &Path) -> PathBuf {
    let output = directory.join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    let result = std::process::Command::new("rustc")
        .arg("--edition=2021")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/analysis/helper.rs"))
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    output
}

#[tokio::test]
async fn actual_background_scan_cancels_recovers_and_reuses_completed_metadata() {
    let (temp, db, root) = setup();
    drop(db);
    let library = Library::open(
        &temp.path().join("library.sqlite3"),
        &temp.path().join("cache"),
    )
    .unwrap();
    let analysis = Arc::new(Analysis::default());
    let executable = helper(temp.path());
    let file = fixture(&root, "slow-cancel.mp4");
    library
        .start(root.id, executable.clone(), analysis.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !Path::new(&file.path).with_extension("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    library.cancel(&analysis);
    wait_idle(&library).await;
    assert_eq!(library.progress().phase, "cancelled");
    assert!(analysis.is_idle());
    // Only synthetic test fixtures are renamed. A new scan reuses the cancelled file's identity.
    let good = Path::new(&root.path).join("good.mp4");
    fs::rename(&file.path, &good).unwrap();
    library
        .start(root.id, executable.clone(), analysis.clone())
        .await
        .unwrap();
    wait_idle(&library).await;
    assert_eq!(library.progress().phase, "complete");
    assert_eq!(library.progress().analyzed, 1);
    library
        .start(root.id, executable.clone(), analysis.clone())
        .await
        .unwrap();
    wait_idle(&library).await;
    assert_eq!(library.progress().analyzed, 0);
    assert_eq!(library.progress().reused, 1);
    let page = library
        .database(|db| db.query(Query::default()))
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "ready");
    fixture(&root, "failure.mp4");
    library
        .start(root.id, executable, analysis.clone())
        .await
        .unwrap();
    wait_idle(&library).await;
    assert_eq!(library.progress().phase, "partial");
    let page = library
        .database(|db| {
            db.query(Query {
                filter: "error".into(),
                ..Default::default()
            })
        })
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    library.shutdown(&analysis).await;
    analysis.shutdown().await;
    assert!(!Path::new(&root.path).join("overlap").exists());
}

#[tokio::test]
async fn unavailable_root_fails_without_losing_recording_history() {
    let (temp, mut db, root) = setup();
    let file = fixture(&root, "clip.mp4");
    db.discover(root.id, 1, &file).unwrap();
    drop(db);
    fs::rename(&root.path, temp.path().join("moved-root")).unwrap();
    let library = Library::open(
        &temp.path().join("library.sqlite3"),
        &temp.path().join("cache"),
    )
    .unwrap();
    let analysis = Arc::new(Analysis::default());
    library
        .start(root.id, temp.path().join("unused-helper"), analysis.clone())
        .await
        .unwrap();
    wait_idle(&library).await;
    assert_eq!(library.progress().phase, "failed");
    assert_eq!(
        library
            .database(|db| db.query(Query::default()))
            .await
            .unwrap()
            .total,
        1
    );
    library.shutdown(&analysis).await;
}
