use super::*;
use rusqlite::{params, types::Value as SqlValue, Connection};
use serde_json::Value;

// A nullable declaration lets the fixture exercise incomplete historical records too.
const SCHEMA:&str="CREATE TABLE contents(created_at INTEGER,category_id TEXT,video_path TEXT,image_path TEXT,thumbnail_path TEXT,metadata BLOB NOT NULL,remote_content_id TEXT,local_content_id TEXT,parent_id TEXT,film_reel_path TEXT)";

fn insert(db: &Connection, id: SqlValue, time: SqlValue, path: &Path, remote: serde_json::Value) {
    let metadata = json!({"title":"Synthetic timestamp clip","remoteContent":{"created":remote}});
    db.execute("INSERT INTO contents(created_at,video_path,metadata,local_content_id) VALUES(?1,?2,jsonb(?3),?4)",params![time,path.to_str().unwrap(),metadata.to_string(),id]).unwrap();
}

#[test]
fn sqlite_fractional_seconds_and_optional_timestamp_failures_do_not_abort_valid_records() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("medal.db");
    let source = Connection::open(&path).unwrap();
    source.execute_batch(SCHEMA).unwrap();
    let cases = vec![
        (SqlValue::Integer(1_700_000_000), Some(1_700_000_000_000)),
        (SqlValue::Real(1_700_000_000.1256), Some(1_700_000_000_126)),
        (SqlValue::Null, None),
        (SqlValue::Integer(0), None),
        (SqlValue::Integer(-1), None),
        (SqlValue::Text("not a timestamp".into()), None),
        (SqlValue::Blob(vec![1, 2]), None),
        (SqlValue::Integer(i64::MAX), None),
        (SqlValue::Real(f64::INFINITY), None),
        (SqlValue::Real(1e100), None),
        (SqlValue::Integer(32_503_680_000), None),
        // Milliseconds in this seconds field are not silently reinterpreted.
        (SqlValue::Integer(1_700_000_000_000), None),
    ];
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db.add_root(temp.path()).unwrap();
    for (i, (value, _)) in cases.iter().enumerate() {
        let video = temp.path().join(format!("synthetic-{i}.mp4"));
        fs::write(&video, b"synthetic media identity").unwrap();
        db.discover(root.id, 1, &files::describe(&video).unwrap())
            .unwrap();
        insert(
            &source,
            SqlValue::Text(format!("id-{i}")),
            value.clone(),
            &video,
            Value::Null,
        );
    }
    insert(
        &source,
        SqlValue::Null,
        SqlValue::Integer(1_700_000_000),
        &temp.path().join("no-id.mp4"),
        Value::Null,
    );
    insert(
        &source,
        SqlValue::Text("".into()),
        SqlValue::Integer(1_700_000_000),
        &temp.path().join("empty-id.mp4"),
        Value::Null,
    );
    drop(source);
    let before = fs::read(&path).unwrap();
    let catalog = super::super::sqlite::load(&path, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(catalog.entries.len(), cases.len());
    assert_eq!(catalog.invalid_identities, 2);
    assert_eq!(catalog.skipped, 2);
    for (entry, (_, expected)) in catalog.entries.iter().zip(&cases) {
        assert_eq!(entry.data.created_ms, *expected);
        assert_eq!(
            entry.data.timestamp_warnings.len(),
            usize::from(expected.is_none())
        );
        assert!(entry.data.content_created_ms.is_none());
    }
    assert_eq!(
        catalog.entries[1].data.extra["providerTimes"]["localCreatedUnixSeconds"],
        json!(1_700_000_000.1256)
    );
    let (prepared, counts) = store::prepare(
        catalog.entries,
        db.medal_index("sqlite").unwrap(),
        &AtomicBool::new(false),
        &AtomicUsize::new(0),
    )
    .unwrap();
    assert_eq!(counts.matched, cases.len());
    assert_eq!(counts.timestamp_warnings, cases.len() - 2);
    assert_eq!(counts.conflicts, 0);
    assert_eq!(prepared[2].references[0].row.timestamp_warnings.len(), 1);
    db.commit_medal(store::Commit {
        source: "sqlite",
        path: "synthetic",
        revision: "one",
        prepared: &prepared,
        additions: &HashSet::new(),
        cancel: &AtomicBool::new(false),
        done: &AtomicUsize::new(0),
    })
    .unwrap();
    assert_eq!(
        db.query(Query::default()).unwrap().total,
        cases.len() as i64
    );
    let rows = db.query(Query::default()).unwrap();
    let fractional = rows
        .items
        .iter()
        .find(|r| r.name == "synthetic-1.mp4")
        .unwrap();
    assert_eq!(fractional.medal_created_ms, Some(1_700_000_000_126));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn remote_milliseconds_are_separate_and_invalid_updates_preserve_last_valid_dates() {
    let temp = tempfile::tempdir().unwrap();
    let video = temp.path().join("clip.mp4");
    fs::write(&video, b"synthetic").unwrap();
    let path = temp.path().join("medal.db");
    let source = Connection::open(&path).unwrap();
    source.execute_batch(SCHEMA).unwrap();
    insert(
        &source,
        SqlValue::Text("same-id".into()),
        SqlValue::Real(1_700_000_000.1256),
        &video,
        json!(1_700_000_123_456_i64),
    );
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db.add_root(temp.path()).unwrap();
    db.discover(root.id, 1, &files::describe(&video).unwrap())
        .unwrap();
    let load = || {
        super::super::sqlite::load(&path, Arc::new(AtomicBool::new(false)))
            .unwrap()
            .entries
    };
    let entries = load();
    assert_eq!(entries[0].data.content_created_ms, Some(1_700_000_123_456));
    assert!(entries[0].data.timestamp_warnings.is_empty());
    let apply = |db: &mut Db, entries| {
        let (prepared, counts) = store::prepare(
            entries,
            db.medal_index("sqlite").unwrap(),
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        )
        .unwrap();
        db.commit_medal(store::Commit {
            source: "sqlite",
            path: "synthetic",
            revision: "revision",
            prepared: &prepared,
            additions: &HashSet::new(),
            cancel: &AtomicBool::new(false),
            done: &AtomicUsize::new(0),
        })
        .unwrap();
        counts
    };
    apply(&mut db, entries);
    for remote in [
        Value::Null,
        json!(0),
        json!(-1),
        json!("invalid"),
        json!(true),
        json!(u64::MAX),
        json!(32_503_680_000_000_i64),
    ] {
        source.execute("UPDATE contents SET created_at=NULL,metadata=jsonb_set(metadata,'$.remoteContent.created',jsonb(?1))",[remote.to_string()]).unwrap();
        let entries = load();
        assert!(entries[0].data.created_ms.is_none());
        assert!(entries[0].data.content_created_ms.is_none());
        assert_eq!(
            entries[0].data.timestamp_warnings.len(),
            if remote.is_null() { 1 } else { 2 }
        );
        assert_eq!(apply(&mut db, entries).timestamp_warnings, 1);
        let saved = &db.provider_metadata(1).unwrap()[0].data;
        assert_eq!(saved.created_ms, Some(1_700_000_000_126));
        assert_eq!(saved.content_created_ms, Some(1_700_000_123_456));
        assert_eq!(
            saved.retained_timestamps,
            vec!["createdMs", "contentCreatedMs"]
        );
    }
    // An absent remote value is not a claim for the epoch and never replaces an older value.
    source
        .execute(
            "UPDATE contents SET metadata=jsonb_remove(metadata,'$.remoteContent.created')",
            [],
        )
        .unwrap();
    apply(&mut db, load());
    source.execute("UPDATE contents SET metadata=jsonb_set(metadata,'$.remoteContent.created',jsonb('9e999'))",[]).unwrap();
    let entries = load();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].data.content_created_ms.is_none());
    assert_eq!(entries[0].data.timestamp_warnings.len(), 2);
    apply(&mut db, entries);
    drop(db);
    let db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    assert_eq!(
        db.provider_metadata(1).unwrap()[0].data.content_created_ms,
        Some(1_700_000_123_456)
    );
}
