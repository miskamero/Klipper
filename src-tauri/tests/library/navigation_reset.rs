use super::*;
use std::time::Duration;

#[test]
fn recursive_folder_gallery_preserves_pagination_filters_and_deduplicates_overlaps() {
    let (_temp, mut db, root) = setup();
    let child = Path::new(&root.path).join("child");
    let grandchild = child.join("nested");
    fs::create_dir_all(&grandchild).unwrap();
    let overlap = db.add_root(&child).unwrap();
    let file = fixture(&root, "root.mp4");
    db.discover(root.id, 1, &file).unwrap();
    for i in 0..45 {
        let file = fixture(&overlap, &format!("clip-{i:02}.mp4"));
        db.discover(root.id, 1, &file).unwrap();
        db.discover(overlap.id, 1, &file).unwrap();
    }
    let path = grandchild.join("deep.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let file = files::describe(&path.canonicalize().unwrap()).unwrap();
    db.discover(root.id, 1, &file).unwrap();
    db.discover(overlap.id, 1, &file).unwrap();
    assert_eq!(db.query(Query::default()).unwrap().total, 47);
    let recursive = db
        .query(Query {
            root_id: Some(root.id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(recursive.total, 47);
    let folder = db
        .navigation()
        .unwrap()
        .folders
        .into_iter()
        .find(|f| f.name == "child" && f.parent_id.is_some())
        .unwrap();
    assert_eq!(folder.recordings, 46);
    let scope = Query {
        root_id: Some(root.id),
        folder_id: Some(folder.id),
        sort: "name".into(),
        filter: "pending".into(),
        ..Default::default()
    };
    let first = db.query(scope.clone()).unwrap();
    assert_eq!(first.total, 46);
    assert_eq!(first.items.len(), 40);
    assert_eq!(first.items[0].name, "clip-00.mp4");
    let second = db
        .query(Query {
            offset: 40,
            ..scope.clone()
        })
        .unwrap();
    assert_eq!(second.items.len(), 6);
    assert_eq!(second.items[0].name, "clip-40.mp4");
    assert_eq!(second.items.last().unwrap().name, "deep.mp4");
    assert_eq!(
        db.query(Query {
            search: "clip-01.mp4".into(),
            ..scope
        })
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        db.query(Query {
            root_id: Some(overlap.id),
            ..Default::default()
        })
        .unwrap()
        .total,
        46
    );
}

fn enrich(db: &mut Db, id: i64, path: &str) {
    db.connection.execute("INSERT INTO sources(kind,external_key) VALUES('medal','synthetic catalog') ON CONFLICT DO NOTHING",[]).unwrap();
    let source: i64 = db
        .connection
        .query_row("SELECT id FROM sources WHERE kind='medal'", [], |r| {
            r.get(0)
        })
        .unwrap();
    db.connection.execute("INSERT INTO provider_entries VALUES(?1,'linked','v1','Imported title','Synthetic game',123,'{}',456)",[source]).unwrap();
    db.connection.execute("INSERT INTO provider_links(source_id,external_id,file_id,relation) VALUES(?1,'linked',?2,'primary')",[source,id]).unwrap();
    db.connection.execute("INSERT INTO provider_entries VALUES(?1,'unresolved','v1','Missing title',NULL,NULL,'{}',456)",[source]).unwrap();
    db.connection
        .execute(
            "INSERT INTO provider_imports VALUES(?1,'synthetic.json','v1',456)",
            [source],
        )
        .unwrap();
    db.connection
        .execute(
            "INSERT INTO provider_previews VALUES(?1,'linked','primary',?2,'v1')",
            rusqlite::params![source, path],
        )
        .unwrap();
}

#[test]
fn collections_use_actual_provider_links_and_indexed_descendants_without_duplicates() {
    let (_temp, mut db, root) = setup();
    let sub = Path::new(&root.path).join("game");
    fs::create_dir_all(sub.join("nested")).unwrap();
    fs::create_dir_all(Path::new(&root.path).join("game-sibling")).unwrap();
    let overlap = db.add_root(&sub).unwrap();
    for (name, parent) in [
        ("one.mp4", &sub),
        ("two.mp4", &sub.join("nested")),
        ("not-medal.mp4", &Path::new(&root.path).join("game-sibling")),
    ] {
        let path = parent.join(name);
        fs::write(&path, b"synthetic").unwrap();
        let file = files::describe(&path.canonicalize().unwrap()).unwrap();
        db.discover(root.id, 1, &file).unwrap();
        if parent.starts_with(&sub) {
            db.discover(overlap.id, 1, &file).unwrap();
        }
    }
    let id = db
        .query(Query {
            search: "one.mp4".into(),
            ..Default::default()
        })
        .unwrap()
        .items[0]
        .id;
    enrich(&mut db, id, "/synthetic/provider.png");
    let nav = db.navigation().unwrap();
    assert_eq!(nav.total, 3);
    assert_eq!(nav.sources.len(), 1);
    assert_eq!(nav.sources[0].recordings, 1);
    let folder = nav
        .folders
        .iter()
        .find(|f| f.name == "game" && f.parent_id.is_some())
        .unwrap();
    assert_eq!(folder.recordings, 2);
    let page = db
        .query(Query {
            folder_id: Some(folder.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    let medal = db
        .query(Query {
            provider: Some("medal".into()),
            search: "Imported".into(),
            sort: "name".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(medal.total, 1);
    assert_eq!(medal.items[0].id, id);
    assert_eq!(
        db.query(Query {
            provider: Some("medal".into()),
            offset: 40,
            ..Default::default()
        })
        .unwrap()
        .items
        .len(),
        0
    );
    assert!(db
        .query(Query {
            folder_id: Some("1:../../source:1".into()),
            ..Default::default()
        })
        .is_err());
    assert!(db
        .query(Query {
            folder_id: Some(folder.id.clone()),
            root_id: Some(overlap.id),
            ..Default::default()
        })
        .is_err());
    db.connection.execute("UPDATE root_entries SET availability='missing' WHERE location_id IN(SELECT id FROM locations WHERE file_id=?1)",[id]).unwrap();
    assert_eq!(
        db.query(Query {
            provider: Some("medal".into()),
            ..Default::default()
        })
        .unwrap()
        .total,
        0
    );
    assert!(db.navigation().unwrap().sources.is_empty());
    db.remove_root(root.id).unwrap();
    assert_eq!(db.navigation().unwrap().total, 2);
    assert!(db.folder_scope(&folder.id).is_err());
}

#[tokio::test]
async fn reset_is_transactional_preserves_sources_and_unrelated_tables_and_recovers_on_restart() {
    let (temp, mut db, root) = setup();
    let file = fixture(&root, "source.mp4");
    db.discover(root.id, 1, &file).unwrap();
    let image = temp.path().join("provider-original.png");
    fs::write(&image, b"original provider image").unwrap();
    let before = fs::read(&file.path).unwrap();
    let image_before = fs::read(&image).unwrap();
    let id = first_id(&db);
    enrich(&mut db, id, image.to_str().unwrap());
    db.connection.execute_batch("CREATE TABLE unrelated_preference(value TEXT); INSERT INTO unrelated_preference VALUES('keep');").unwrap();
    // A failure part-way through the transaction must preserve every preceding table.
    db.connection.execute_batch("CREATE TRIGGER fail_reset BEFORE DELETE ON recordings BEGIN SELECT RAISE(ABORT,'synthetic reset failure'); END;").unwrap();
    assert!(db.reset_library().is_err());
    assert_eq!(db.query(Query::default()).unwrap().total, 1);
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM provider_entries", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(!db.cache_cleanup_pending().unwrap());
    db.connection
        .execute_batch("DROP TRIGGER fail_reset;")
        .unwrap();
    drop(db);
    let cache = temp.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let preview = rusqlite::Connection::open(cache.join("previews-v1.sqlite3")).unwrap();
    preview.execute_batch("CREATE TABLE images(key TEXT PRIMARY KEY,image BLOB,used_at INTEGER);CREATE TABLE failures(key TEXT PRIMARY KEY,used_at INTEGER);INSERT INTO images VALUES('old',X'FFD8FFD9',1);INSERT INTO failures VALUES('bad',1);").unwrap();
    drop(preview);
    let library = Library::open(&temp.path().join("library.sqlite3"), &cache).unwrap();
    let analysis = Arc::new(Analysis::default());
    let old_epoch = library.epoch();
    let result = library
        .reset(analysis.clone(), || async { Ok(()) })
        .await
        .unwrap();
    assert!(result.cache_warning.is_none());
    assert!(library.check_epoch(old_epoch).is_err());
    library
        .database(|db| {
            assert_eq!(db.navigation()?.total, 0);
            for table in [
                "roots",
                "recordings",
                "locations",
                "root_entries",
                "media",
                "sources",
                "source_recordings",
                "recording_overrides",
                "provider_entries",
                "provider_links",
                "provider_access",
                "provider_imports",
                "provider_previews",
            ] {
                assert_eq!(
                    db.connection
                        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                            .get::<_, i64>(0))
                        .unwrap(),
                    0,
                    "{table}"
                );
            }
            assert_eq!(
                db.connection
                    .query_row("SELECT value FROM unrelated_preference", [], |r| r
                        .get::<_, String>(0))
                    .unwrap(),
                "keep"
            );
            assert!(!db.cache_cleanup_pending()?);
            Ok(())
        })
        .await
        .unwrap();
    let preview = rusqlite::Connection::open(cache.join("previews-v1.sqlite3")).unwrap();
    assert_eq!(
        preview
            .query_row("SELECT count(*) FROM images", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        preview
            .query_row("SELECT count(*) FROM failures", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(preview);
    drop(library);
    let library = Library::open(&temp.path().join("library.sqlite3"), &cache).unwrap();
    assert_eq!(
        library
            .database(|db| db.query(Query::default()))
            .await
            .unwrap()
            .total,
        0
    );
    let path = PathBuf::from(&root.path);
    library
        .database(move |db| db.add_root(&path))
        .await
        .unwrap();
    assert_eq!(fs::read(file.path).unwrap(), before);
    assert_eq!(fs::read(image).unwrap(), image_before);
    library.shutdown(&analysis).await;
}

#[tokio::test]
async fn reset_cancels_an_actual_scan_and_drains_admitted_requests_before_deleting() {
    let (temp, db, root) = setup();
    let file = fixture(&root, "slow-reset.mp4");
    drop(db);
    let library = Library::open(
        &temp.path().join("library.sqlite3"),
        &temp.path().join("cache"),
    )
    .unwrap();
    let analysis = Arc::new(Analysis::default());
    library
        .start(root.id, super::scan::helper(temp.path()), analysis.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !Path::new(&file.path).with_extension("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let admitted = library.operation().await.unwrap();
    let owner = library.clone();
    let worker = analysis.clone();
    let reset = tokio::spawn(async move { owner.reset(worker, || async { Ok(()) }).await });
    while !library.resetting.load(std::sync::atomic::Ordering::Acquire) {
        tokio::task::yield_now().await;
    }
    assert!(library.operation().await.is_err());
    assert!(library
        .start(root.id, temp.path().join("unused"), analysis.clone())
        .await
        .is_err());
    assert!(!reset.is_finished());
    drop(admitted);
    tokio::time::timeout(Duration::from_secs(10), reset)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(analysis.is_idle());
    assert_eq!(
        library
            .database(|db| db.query(Query::default()))
            .await
            .unwrap()
            .total,
        0
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        library
            .database(|db| db.query(Query::default()))
            .await
            .unwrap()
            .total,
        0
    );
    assert!(Path::new(&file.path).exists());
    assert!(!Path::new(&root.path).join("overlap").exists());
    library.shutdown(&analysis).await;
    analysis.shutdown().await;
}

#[tokio::test]
async fn failed_cache_cleanup_is_reported_disabled_and_retried_after_restart() {
    let (temp, mut db, root) = setup();
    db.discover(root.id, 1, &fixture(&root, "clip.mp4"))
        .unwrap();
    drop(db);
    let cache = temp.path().join("cache");
    fs::create_dir_all(cache.join("previews-v1.sqlite3")).unwrap();
    let library = Library::open(&temp.path().join("library.sqlite3"), &cache).unwrap();
    let analysis = Arc::new(Analysis::default());
    let result = library
        .reset(analysis.clone(), || async { Ok(()) })
        .await
        .unwrap();
    assert!(result.cache_warning.unwrap().contains("pending"));
    assert!(library
        .database(|db| db.cache_cleanup_pending())
        .await
        .unwrap());
    assert!(library.operation().await.is_ok());
    drop(library);
    // Only our temporary synthetic directory is removed to simulate resolving the failure.
    fs::remove_dir(cache.join("previews-v1.sqlite3")).unwrap();
    let library = Library::open(&temp.path().join("library.sqlite3"), &cache).unwrap();
    assert!(!library
        .database(|db| db.cache_cleanup_pending())
        .await
        .unwrap());
    assert_eq!(
        library
            .database(|db| db.query(Query::default()))
            .await
            .unwrap()
            .total,
        0
    );
    library.shutdown(&analysis).await;
}
