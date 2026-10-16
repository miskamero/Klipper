use super::{db::Db, files, query::Query, *};
use std::{fs, path::PathBuf};
use tempfile::TempDir;
mod scan;

fn setup() -> (TempDir, Db, Root) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recordings");
    fs::create_dir(&path).unwrap();
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(&path).unwrap())
        .unwrap();
    (temp, db, root)
}

fn fixture(root: &Root, name: &str) -> files::File {
    let path = Path::new(&root.path).join(name);
    fs::write(&path, b"synthetic, not a recording").unwrap();
    files::describe(&path).unwrap()
}

fn media() -> crate::media::MediaInfo {
    serde_json::from_value(serde_json::json!({
        "container":"mp4","videoCodec":"hevc","width":1920,"height":1080,
        "frameRate":60.0,"duration":12.5,"fileSize":24,"bitrate":null,"audio":[]
    }))
    .unwrap()
}

fn first_id(db: &Db) -> i64 {
    db.query(Query::default()).unwrap().items[0].id
}

#[test]
fn overlapping_roots_deduplicate_and_removal_preserves_other_association_and_history() {
    let (_temp, mut db, root) = setup();
    let sub = Path::new(&root.path).join("nested");
    fs::create_dir(&sub).unwrap();
    let other = db.add_root(&sub).unwrap();
    let file = fixture(&other, "clip.mp4");
    db.discover(root.id, 1, &file).unwrap();
    db.discover(other.id, 1, &file).unwrap();
    assert_eq!(db.query(Query::default()).unwrap().total, 1);
    assert_eq!(db.add_root(Path::new(&root.path)).unwrap().id, root.id);
    db.remove_root(root.id).unwrap();
    assert_eq!(db.query(Query::default()).unwrap().total, 1);
    db.remove_root(other.id).unwrap();
    assert_eq!(db.query(Query::default()).unwrap().total, 0);
    assert!(Path::new(&file.path).exists());
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM recordings", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn fingerprints_skip_unchanged_analysis_and_invalidate_changed_media() {
    let (_temp, mut db, root) = setup();
    let mut file = fixture(&root, "clip.mp4");
    assert!(!db.discover(root.id, 1, &file).unwrap());
    let id = first_id(&db);
    db.save_media(id, &file, &media()).unwrap();
    assert!(db.discover(root.id, 2, &file).unwrap());
    assert!(db.cached(id, &file).unwrap().is_some());
    fs::write(
        &file.path,
        b"changed synthetic contents with a different size",
    )
    .unwrap();
    file = files::describe(Path::new(&file.path)).unwrap();
    assert!(!db.discover(root.id, 3, &file).unwrap());
    assert!(db.cached(id, &file).unwrap().is_none());
    assert_eq!(db.query(Query::default()).unwrap().items[0].duration, None);
}

#[test]
fn missing_partial_and_interrupted_scans_retain_metadata_and_persist() {
    let (temp, mut db, root) = setup();
    let file = fixture(&root, "clip.mp4");
    let generation = db.begin_scan(root.id).unwrap();
    db.discover(root.id, generation, &file).unwrap();
    db.save_media(first_id(&db), &file, &media()).unwrap();
    let next = db.begin_scan(root.id).unwrap();
    db.finish_discovery(root.id, next, false).unwrap();
    assert_eq!(
        db.query(Query::default()).unwrap().items[0].availability,
        "unverified"
    );
    db.finish_discovery(root.id, next, true).unwrap();
    let page = db.query(Query::default()).unwrap();
    assert_eq!(page.items[0].availability, "missing");
    assert_eq!(page.items[0].duration, Some(12.5));
    drop(db);
    let db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    assert_eq!(db.root(root.id).unwrap().scan_state, "interrupted");
    assert_eq!(db.query(Query::default()).unwrap().total, 1);
}

#[test]
fn pagination_search_sort_and_filters_are_bounded_and_parameterized() {
    let (_temp, mut db, root) = setup();
    for index in 0..45 {
        let file = fixture(&root, &format!("clip-{index:02}.mp4"));
        db.discover(root.id, 1, &file).unwrap();
    }
    let page = db
        .query(Query {
            sort: "name".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!((page.total, page.items.len()), (45, 40));
    assert_eq!(page.items[0].name, "clip-00.mp4");
    assert_eq!(
        db.query(Query {
            offset: 40,
            ..Default::default()
        })
        .unwrap()
        .items
        .len(),
        5
    );
    assert_eq!(
        db.query(Query {
            search: "clip-04".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        db.query(Query {
            search: "%".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        0
    );
    assert_eq!(
        db.query(Query {
            search: "' OR 1=1 --".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        0
    );
    assert_eq!(
        db.query(Query {
            filter: "error".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        0
    );
    let file = files::describe(&Path::new(&root.path).join("clip-00.mp4")).unwrap();
    db.save_media(first_id(&db), &file, &media()).unwrap();
    assert_eq!(
        db.query(Query {
            codec: "hevc".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
    assert!(db
        .query(Query {
            sort: "name; DROP TABLE recordings".into(),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn renamed_or_hardlinked_files_use_physical_identity_without_content_hashing() {
    let (_temp, mut db, root) = setup();
    let file = fixture(&root, "before.mp4");
    db.discover(root.id, 1, &file).unwrap();
    let id = first_id(&db);
    db.save_media(id, &file, &media()).unwrap();
    let next = Path::new(&root.path).join("after.mp4");
    fs::rename(&file.path, &next).unwrap();
    let renamed = files::describe(&next).unwrap();
    db.discover(root.id, 2, &renamed).unwrap();
    db.finish_discovery(root.id, 2, true).unwrap();
    if file.identity.is_some() {
        assert_eq!(db.query(Query::default()).unwrap().items[0].id, id);
        assert_eq!(db.query(Query::default()).unwrap().total, 1);
        assert!(db.cached(id, &renamed).unwrap().is_some());
    }
    let hardlink = Path::new(&root.path).join("link.mp4");
    fs::hard_link(&next, &hardlink).unwrap();
    db.discover(root.id, 2, &files::describe(&hardlink).unwrap())
        .unwrap();
    if file.identity.is_some() {
        assert_eq!(db.query(Query::default()).unwrap().total, 1);
    }
}

#[tokio::test]
async fn walker_is_recursive_supported_only_and_cancellable() {
    let (_temp, _db, root) = setup();
    fs::create_dir(Path::new(&root.path).join("nested")).unwrap();
    fixture(&root, "one.MP4");
    fixture(&root, "nested/two.webm");
    fs::write(Path::new(&root.path).join("private.json"), b"{}").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let path = PathBuf::from(&root.path);
    let walker = tokio::task::spawn_blocking(move || {
        files::walk(&path, Arc::new(AtomicBool::new(false)), tx)
    });
    let mut count = 0;
    while let Some(entry) = rx.recv().await {
        match entry {
            files::Entry::File(_) => count += 1,
            files::Entry::Issue(e) => panic!("{e}"),
            _ => {}
        }
    }
    walker.await.unwrap();
    assert_eq!(count, 2);
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let path = PathBuf::from(&root.path);
    tokio::task::spawn_blocking(move || files::walk(&path, Arc::new(AtomicBool::new(true)), tx))
        .await
        .unwrap();
    assert!(rx.recv().await.is_none());
}

#[test]
fn authorization_rejects_outside_paths_and_replaced_roots() {
    let (temp, _db, root) = setup();
    let outside = temp.path().join("outside.mp4");
    fs::write(&outside, b"synthetic").unwrap();
    assert!(files::within(&outside, Path::new(&root.path), root.identity.as_deref()).is_err());
    assert!(files::within(
        Path::new(&root.path),
        Path::new(&root.path),
        Some("wrong-identity")
    )
    .is_err());
}
