use super::*;
use rusqlite::{params, Connection};
use std::time::Instant;

const SCHEMA:&str="CREATE TABLE contents(created_at INTEGER NOT NULL,category_id TEXT,video_path TEXT,image_path TEXT,thumbnail_path TEXT,metadata BLOB NOT NULL,remote_content_id TEXT UNIQUE,local_content_id TEXT UNIQUE NOT NULL,parent_id TEXT,film_reel_path TEXT)";

fn source(path: &Path) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    db
}
fn record(db: &Connection, id: &str, path: &Path, title: Option<&str>, game: Option<&str>) {
    let metadata = json!({"title":title,"clipDuration":12.5,"tags":["Synthetic tag"],"bookmarks":[{"time":2.5,"name":"Synthetic event","data":{"account":"DO NOT STORE"}}],
        "wasTrimmed":true,"trimStart":2.0,"trimEnd":14.5,"trimDuration":12.5,"sourceVideoDuration":30.0,
        "remoteSyncedAt":1_700_001_000_000_i64,"remoteContent":{"contentTitle":"Content title","hasTitle":true,"contentDescription":"Synthetic description","contentId":format!("remote-{id}"),"created":1_700_000_123_000_i64,"category":{"categoryId":"game","categoryName":game},"trimStartTime":2.0,"poster":{"token":"DO NOT STORE"},"thumbnailUrl":"https://invalid.example/secret"},
        "recorder":{"sessionId":"DO NOT STORE","microphoneDeviceName":"DO NOT STORE","activeWindowTitle":"DO NOT STORE","eventobj":{"data":{"secret":"DO NOT STORE"}}},"userId":"DO NOT STORE"});
    db.execute(
        "INSERT INTO contents VALUES(1700000000,'game',?1,NULL,NULL,jsonb(?2),?3,?4,'',?5)",
        params![
            path.to_str().unwrap(),
            metadata.to_string(),
            format!("remote-{id}"),
            id,
            path.with_file_name("unverified-film-reel.mp4")
                .to_str()
                .unwrap()
        ],
    )
    .unwrap();
}
fn load(path: &Path) -> parse::Catalog {
    super::super::sqlite::load(path, Arc::new(AtomicBool::new(false))).unwrap()
}
fn apply(db: &mut Db, source: &str, entries: Vec<parse::Entry>) -> store::Counts {
    let (prepared, counts) = store::prepare(
        entries,
        db.medal_index(source).unwrap(),
        &AtomicBool::new(false),
        &AtomicUsize::new(0),
    )
    .unwrap();
    db.commit_medal(store::Commit {
        source,
        path: source,
        revision: "synthetic-v1",
        prepared: &prepared,
        additions: &HashSet::new(),
        cancel: &AtomicBool::new(false),
        done: &AtomicUsize::new(0),
    })
    .unwrap();
    counts
}

#[test]
fn sqlite_snapshot_reads_committed_wal_sanitizes_fields_and_never_changes_source() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("medal.db");
    let db = source(&path);
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    record(
        &db,
        "one",
        &temp.path().join("one-trim.mp4"),
        Some("Local title"),
        Some("Synthetic game"),
    );
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    record(
        &db,
        "uncommitted",
        &temp.path().join("uncommitted.mp4"),
        Some("Uncommitted title"),
        None,
    );
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let before = fs::read(&path).unwrap();
    let before_wal = fs::read(&wal).unwrap();
    assert!(!before_wal.is_empty());
    let catalog = load(&path);
    assert_eq!(catalog.entries.len(), 1);
    let m = &catalog.entries[0].data;
    assert_eq!(m.source_format, "sqlite");
    assert_eq!(m.title.as_deref(), Some("Local title"));
    assert_eq!(m.game.as_deref(), Some("Synthetic game"));
    assert_eq!(m.created_ms, Some(1_700_000_000_000));
    assert_eq!(m.content_created_ms, Some(1_700_000_123_000));
    assert_eq!(m.remote_trim_start, Some(2.0));
    assert_eq!(m.tags, ["Synthetic tag"]);
    assert!(m.original_file_path.is_none());
    let normalized = serde_json::to_string(m).unwrap();
    assert!(!normalized.contains("DO NOT STORE"));
    assert!(!normalized.contains("https://"));
    assert!(!normalized.contains("unverified-film-reel"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&wal).unwrap(), before_wal);
    db.execute_batch("ROLLBACK").unwrap();
    let cancelled = Arc::new(AtomicBool::new(true));
    assert!(super::super::sqlite::load(&path, cancelled)
        .err()
        .unwrap()
        .contains("cancelled"));
}

#[test]
fn json_and_sqlite_keep_first_selected_claim_in_both_orders_and_never_erase_it() {
    for sqlite_first in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("clip.mp4");
        fs::write(&video, b"synthetic").unwrap();
        let file = files::describe(&video.canonicalize().unwrap()).unwrap();
        let database = temp.path().join("library.sqlite3");
        let mut db = Db::open(&database).unwrap();
        let root = db
            .add_root(&files::local_directory(temp.path()).unwrap())
            .unwrap();
        db.discover(root.id, 1, &file).unwrap();
        let source_path = temp.path().join("medal.db");
        let source_db = source(&source_path);
        record(
            &source_db,
            "same-id",
            &video,
            Some("SQLite title"),
            Some("SQLite game"),
        );
        drop(source_db);
        let json_bytes=serde_json::to_vec(&json!({"same-id":{"FilePath":video,"GameTitle":"JSON title","TimeCreated":1_700_000_000,"GameCategory":"game","Content":{"contentTitle":"JSON remote title","hasTitle":false,"category":{"categoryId":"game","categoryName":"JSON game"},"tags":["Other tag"],"trimStartTime":0.0}}})).unwrap();
        let json_entries = || parse::parse(&json_bytes).unwrap().entries;
        let sql_entries = || load(&source_path).entries;
        let first = if sqlite_first {
            apply(&mut db, "sqlite", sql_entries())
        } else {
            apply(&mut db, "json", json_entries())
        };
        assert_eq!(first.recovered, 1);
        let second = if sqlite_first {
            apply(&mut db, "json", json_entries())
        } else {
            apply(&mut db, "sqlite", sql_entries())
        };
        assert_eq!(second.recovered, 0);
        assert_eq!(second.identity_conflicts, 0);
        assert_eq!(second.conflicts, 0);
        assert_eq!(second.metadata_differences, 1);
        let (preview, _) = store::prepare(
            if sqlite_first {
                json_entries()
            } else {
                sql_entries()
            },
            db.medal_index(if sqlite_first { "json" } else { "sqlite" })
                .unwrap(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        )
        .unwrap();
        let fields: HashSet<_> = preview[0].references[0]
            .row
            .differences
            .iter()
            .map(|d| d.field.as_str())
            .collect();
        for field in ["game", "title", "hasTitle", "tags", "remoteTrimStart"] {
            assert!(fields.contains(field));
        }
        let expected_title = if sqlite_first {
            "SQLite title"
        } else {
            "JSON title"
        };
        let expected_game = if sqlite_first {
            "SQLite game"
        } else {
            "JSON game"
        };
        let row = db.query(Query::default()).unwrap().items.remove(0);
        let id = row.id;
        assert_eq!(row.title.as_deref(), Some(expected_title));
        assert_eq!(row.game.as_deref(), Some(expected_game));
        assert_eq!(db.provider_metadata(id).unwrap().len(), 2);
        for _ in 0..2 {
            assert_eq!(apply(&mut db, "sqlite", sql_entries()).unchanged, 1);
            assert_eq!(apply(&mut db, "json", json_entries()).unchanged, 1);
        }
        let mut empty = sql_entries();
        empty[0].data.title = None;
        empty[0].data.game = None;
        empty[0].data.created_ms = None;
        apply(&mut db, "sqlite", empty);
        assert_eq!(
            db.query(Query::default()).unwrap().items[0]
                .title
                .as_deref(),
            Some(expected_title)
        );
        apply(&mut db, "sqlite", sql_entries());
        let mut changed = json_entries();
        changed[0].data.title = Some("Updated other claim".into());
        apply(&mut db, "json", changed);
        assert_eq!(
            db.query(Query::default()).unwrap().items[0]
                .title
                .as_deref(),
            Some(expected_title)
        );
        assert_eq!(
            db.query(Query {
                game: Some(expected_game.into()),
                search: expected_title.into(),
                ..Default::default()
            })
            .unwrap()
            .total,
            1
        );
        assert_eq!(db.navigation().unwrap().sources[0].recordings, 1);
        let generation = db.begin_scan(root.id).unwrap();
        db.discover(root.id, generation, &file).unwrap();
        db.finish_discovery(root.id, generation, true).unwrap();
        drop(db);
        fs::rename(&source_path, temp.path().join("medal-unavailable.db")).unwrap();
        let mut db = Db::open(&database).unwrap();
        assert_eq!(
            db.query(Query::default()).unwrap().items[0]
                .title
                .as_deref(),
            Some(expected_title)
        );
        assert_eq!(db.provider_metadata(id).unwrap().len(), 2);
        db.reset_library().unwrap();
        assert_eq!(db.query(Query::default()).unwrap().total, 0);
        assert_eq!(
            db.connection
                .query_row("SELECT count(*) FROM provider_choices", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(fs::read(&video).unwrap(), b"synthetic");
    }
}

#[test]
fn sqlite_only_updates_missing_fields_and_unique_game_lookups_without_inventing_relationships() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("medal.db");
    let source_db = source(&path);
    record(
        &source_db,
        "one",
        &temp.path().join("one.mp4"),
        Some("First title"),
        Some("Named game"),
    );
    record(&source_db, "two", &temp.path().join("two.mp4"), None, None);
    drop(source_db);
    let catalog = load(&path);
    assert_eq!(catalog.entries.len(), 2);
    let second = &catalog.entries[1].data;
    assert_eq!(second.game.as_deref(), Some("Named game"));
    assert_eq!(second.local_title, None);
    assert_eq!(second.title.as_deref(), Some("Content title"));
    assert!(second.original_file_path.is_none());
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(temp.path()).unwrap())
        .unwrap();
    for name in ["one.mp4", "two.mp4"] {
        let file = temp.path().join(name);
        fs::write(&file, b"synthetic").unwrap();
        db.discover(root.id, 1, &files::describe(&file).unwrap())
            .unwrap();
    }
    apply(&mut db, "sqlite", catalog.entries);
    let mut updated = load(&path).entries;
    updated[0].data.title = Some("Updated title".into());
    apply(&mut db, "sqlite", updated);
    assert_eq!(
        db.query(Query {
            search: "Updated title".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
}

#[test]
fn unsupported_malformed_and_duplicate_provider_databases_fail_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("malformed.db");
    fs::write(&path, b"not sqlite").unwrap();
    assert!(super::super::sqlite::load(&path, Arc::new(AtomicBool::new(false))).is_err());
    let path = temp.path().join("unsupported.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE unrelated(secret TEXT)")
        .unwrap();
    drop(db);
    assert!(
        super::super::sqlite::load(&path, Arc::new(AtomicBool::new(false)))
            .err()
            .unwrap()
            .contains("Unsupported")
    );
    let path = temp.path().join("duplicates.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(&SCHEMA.replace(" UNIQUE", "")).unwrap();
    for _ in 0..2 {
        record(
            &db,
            "duplicate",
            &temp.path().join("same.mp4"),
            Some("title"),
            None,
        );
    }
    drop(db);
    assert!(
        super::super::sqlite::load(&path, Arc::new(AtomicBool::new(false)))
            .err()
            .unwrap()
            .contains("duplicate")
    );
}

#[test]
fn many_distinct_files_import_incrementally_without_rewriting_unchanged_entries() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("medal.db");
    let source_db = source(&path);
    let videos = temp.path().join("videos");
    fs::create_dir(&videos).unwrap();
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db.add_root(&videos).unwrap();
    for i in 0..400 {
        let video = videos.join(format!("distinct-{i:04}.mp4"));
        fs::write(&video, format!("synthetic {i}")).unwrap();
        db.discover(root.id, 1, &files::describe(&video).unwrap())
            .unwrap();
        record(
            &source_db,
            &format!("id-{i:04}"),
            &video,
            Some("Title"),
            Some("Game"),
        );
    }
    drop(source_db);
    let start = Instant::now();
    let entries = load(&path).entries;
    let parsed = start.elapsed();
    let counts = apply(&mut db, "sqlite", entries);
    let cold = start.elapsed();
    assert_eq!(counts.recovered, 400);
    let sum: i64 = db
        .connection
        .query_row("SELECT sum(imported_ms) FROM provider_entries", [], |r| {
            r.get(0)
        })
        .unwrap();
    let warm = Instant::now();
    let before_changes = db.connection.total_changes();
    assert_eq!(apply(&mut db, "sqlite", load(&path).entries).unchanged, 400);
    assert_eq!(
        db.connection.total_changes() - before_changes,
        1,
        "Unchanged refresh only updates import history"
    );
    assert_eq!(
        db.connection
            .query_row("SELECT sum(imported_ms) FROM provider_entries", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        sum
    );
    assert_eq!(db.query(Query::default()).unwrap().total, 400);
    println!("400 distinct synthetic file identities: parse={parsed:?}, cold={cold:?}, warm={:?}, provider bytes={}",warm.elapsed(),fs::metadata(path).unwrap().len());
}

#[test]
#[cfg(windows)]
fn closed_wal_without_sidecars_is_read_without_creating_provider_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("closed.db");
    let db = source(&path);
    db.execute_batch("PRAGMA journal_mode=WAL").unwrap();
    record(
        &db,
        "one",
        &temp.path().join("one.mp4"),
        Some("Title"),
        None,
    );
    drop(db);
    assert!(!PathBuf::from(format!("{}-wal", path.display())).exists());
    let before = fs::read(&path).unwrap();
    assert_eq!(before[18], 2);
    assert_eq!(load(&path).entries.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), before);
    for suffix in ["-wal", "-shm"] {
        assert!(!PathBuf::from(format!("{}{suffix}", path.display())).exists());
    }
}

#[test]
fn timeouts_and_cancellation_while_a_provider_is_locked_leave_no_library_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("locked.db");
    let db = source(&path);
    record(
        &db,
        "one",
        &temp.path().join("one.mp4"),
        Some("Title"),
        None,
    );
    let result = super::super::sqlite::load_with_timeout(
        &path,
        Arc::new(AtomicBool::new(false)),
        std::time::Duration::ZERO,
    );
    assert!(result.err().unwrap().contains("timed out"));
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let before = fs::read(&path).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let stop = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        signal.store(true, Ordering::Release);
    });
    let started = Instant::now();
    assert!(super::super::sqlite::load(&path, cancel).is_err());
    stop.join().unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(fs::read(&path).unwrap(), before);
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(load(&path).entries.len(), 1);
}

#[test]
fn version_three_migration_retains_selected_values_and_preview_before_sqlite_enrichment() {
    let temp = tempfile::tempdir().unwrap();
    let video = temp.path().join("clip.mp4");
    fs::write(&video, b"synthetic").unwrap();
    let image = temp.path().join("preview.jpg");
    fs::write(&image, b"synthetic image bytes").unwrap();
    let database = temp.path().join("library.sqlite3");
    let mut db = Db::open(&database).unwrap();
    let root = db.add_root(temp.path()).unwrap();
    let file = files::describe(&video).unwrap();
    db.discover(root.id, 1, &file).unwrap();
    apply(
        &mut db,
        "json",
        parse::parse(&fixture(&video, &image)).unwrap().entries,
    );
    let old_preview = db.provider_preview(1, &file).unwrap();
    assert!(old_preview.is_some());
    let old_view = include_str!("../../src/library/migrations/002.sql")
        .split("CREATE VIEW provider_display AS")
        .nth(1)
        .unwrap();
    db.connection.execute_batch(&format!("DROP VIEW provider_display; DROP TABLE provider_choices; CREATE VIEW provider_display AS{old_view} PRAGMA user_version=3;")).unwrap();
    drop(db);
    let mut db = Db::open(&database).unwrap();
    assert_eq!(
        db.query(Query::default()).unwrap().items[0]
            .title
            .as_deref(),
        Some("Custom title")
    );
    assert_eq!(db.provider_preview(1, &file).unwrap(), old_preview);
    let source_path = temp.path().join("medal.db");
    let source_db = source(&source_path);
    record(
        &source_db,
        "synthetic-clip-id",
        &video,
        Some("Other title"),
        Some("Other game"),
    );
    drop(source_db);
    apply(&mut db, "sqlite", load(&source_path).entries);
    let row = &db.query(Query::default()).unwrap().items[0];
    assert_eq!(row.title.as_deref(), Some("Custom title"));
    assert_eq!(row.game.as_deref(), Some("Synthetic game"));
    assert_eq!(db.provider_preview(1, &file).unwrap(), old_preview);
}

async fn generate(path: &Path, input: &str, image: bool) {
    let mut command = tokio::process::Command::new("ffmpeg");
    command.args(["-v", "error", "-nostdin", "-n", "-f", "lavfi", "-i", input]);
    if image {
        command.args(["-frames:v", "1", "-threads", "1"]);
    } else {
        command.args(["-t", "0.4", "-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    }
    command.arg(path);
    let (_cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
    let result = crate::media_process::capture(
        &mut command,
        &mut cancel,
        std::time::Duration::from_secs(10),
        1024 * 1024,
        "ffmpeg",
        false,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result.1.success());
}

#[tokio::test]
#[ignore = "uses FFmpeg only for synthetic temporary images/videos"]
async fn confirmed_sqlite_previews_materialize_survive_source_loss_and_reuse_without_ffmpeg() {
    let temp = tempfile::tempdir().unwrap();
    let video = temp.path().join("clip.mp4");
    let image = temp.path().join("provider.jpg");
    generate(&video, "testsrc2=size=96x64:rate=10", false).await;
    generate(&image, "color=c=green:s=96x64", true).await;
    let original = fs::read(&video).unwrap();
    let original_image = fs::read(&image).unwrap();
    let source_path = temp.path().join("medal.db");
    let source_db = source(&source_path);
    record(&source_db, "one", &video, Some("SQLite clip"), Some("Game"));
    source_db
        .execute(
            "UPDATE contents SET thumbnail_path=?1",
            [image.to_str().unwrap()],
        )
        .unwrap();
    drop(source_db);
    let before = fs::read(&source_path).unwrap();
    let entries = load(&source_path).entries;
    let database = temp.path().join("library.sqlite3");
    let cache = temp.path().join("cache");
    let library = crate::library::Library::open(&database, &cache).unwrap();
    let v = video.clone();
    library
        .database(move |db| {
            let root = db.add_root(v.parent().unwrap())?;
            db.discover(root.id, 1, &files::describe(&v)?)?;
            apply(db, "sqlite", entries);
            Ok(())
        })
        .await
        .unwrap();
    let analysis = Arc::new(crate::analysis::Analysis::default());
    assert!(library
        .materialize_medal_previews("sqlite".into(), "ffmpeg".into(), analysis.clone())
        .await
        .is_none());
    let image_bytes = library.cached_thumbnail(1).await.unwrap().unwrap();
    assert!(crate::library::thumbnails::valid_image(&image_bytes));
    assert!(library
        .materialize_medal_previews("sqlite".into(), "no-such-ffmpeg".into(), analysis.clone())
        .await
        .is_none());
    library.shutdown(&analysis).await;
    drop(library);
    fs::rename(&image, temp.path().join("provider-unavailable.jpg")).unwrap();
    fs::rename(&source_path, temp.path().join("medal-unavailable.db")).unwrap();
    let library = crate::library::Library::open(&database, &cache).unwrap();
    assert_eq!(
        library.cached_thumbnail(1).await.unwrap().unwrap(),
        image_bytes
    );
    assert_eq!(fs::read(&video).unwrap(), original);
    assert_eq!(
        fs::read(temp.path().join("provider-unavailable.jpg")).unwrap(),
        original_image
    );
    assert_eq!(
        fs::read(temp.path().join("medal-unavailable.db")).unwrap(),
        before
    );
    let analysis = Arc::new(crate::analysis::Analysis::default());
    library.medal.cancel.store(true, Ordering::Release);
    assert!(library
        .materialize_medal_previews("sqlite".into(), "ffmpeg".into(), analysis.clone())
        .await
        .is_some());
    library.shutdown(&analysis).await;
}

#[tokio::test]
#[ignore = "performance fixture: 600 distinct generated temporary video files; requires FFmpeg"]
async fn real_distinct_synthetic_recordings_performance() {
    let temp = tempfile::tempdir().unwrap();
    let seed = temp.path().join("generated.mp4");
    generate(&seed, "testsrc2=size=96x64:rate=10", false).await;
    let videos = temp.path().join("videos");
    fs::create_dir(&videos).unwrap();
    let source_path = temp.path().join("medal.db");
    let source_db = source(&source_path);
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db.add_root(&videos).unwrap();
    let original = fs::read(&seed).unwrap();
    for i in 0..600 {
        let video = videos.join(format!("synthetic-{i:04}.mp4"));
        fs::copy(&seed, &video).unwrap();
        db.discover(root.id, 1, &files::describe(&video).unwrap())
            .unwrap();
        record(
            &source_db,
            &format!("id-{i:04}"),
            &video,
            Some("Synthetic recording"),
            Some("Synthetic game"),
        );
    }
    drop(source_db);
    let before = fs::read(&source_path).unwrap();
    let started = Instant::now();
    let entries = load(&source_path).entries;
    let parsed = started.elapsed();
    apply(&mut db, "sqlite", entries);
    let cold = started.elapsed();
    let warm = Instant::now();
    assert_eq!(
        apply(&mut db, "sqlite", load(&source_path).entries).unchanged,
        600
    );
    assert_eq!(db.query(Query::default()).unwrap().total, 600);
    assert_eq!(
        db.connection
            .query_row("SELECT count(DISTINCT identity) FROM recordings", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        600
    );
    println!("600 distinct real synthetic videos: parse={parsed:?}, cold={cold:?}, warm={:?}, source={} bytes, videos={} bytes",warm.elapsed(),before.len(),original.len()*600);
    assert_eq!(fs::read(&source_path).unwrap(), before);
    assert_eq!(fs::read(&seed).unwrap(), original);
}
