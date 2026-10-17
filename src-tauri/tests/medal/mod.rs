use super::*;
use crate::library::{db::Db, files, query::Query};
use serde_json::json;
use std::fs;
mod games;
mod sqlite;
mod thumbnails;
mod timestamps;

fn fixture(path: &Path, image: &Path) -> Vec<u8> {
    serde_json::to_vec(&json!({"synthetic-clip-id":{
    "uuid":"synthetic-clip-id","GameTitle":"Local title","GameCategory":"synthetic-game", "TimeCreated":1_700_000_000,
    "FilePath":path,"Image":image,"duration":12.5,"userId":"DO NOT STORE", "sessionId":"DO NOT STORE",
    "Content":{"hasTitle":true,"contentTitle":"Custom title","contentDescription":"Synthetic description","created":1_700_000_123_000_i64,"category":{"categoryId":"synthetic-game","categoryName":"Synthetic game"},"poster":{"secret":"DO NOT STORE"},"tags":["Synthetic tag"]},
    "metadata":{"eventobj":{"name":"Synthetic event","tags":{"Map":"Synthetic map","playerId":"DO NOT STORE"},"data":{"token":"DO NOT STORE"}}},
    "edit":{"trim":{"trimStartTimeInSeconds":2.0,"trimDurationInSeconds":5.0}},"lastEdit":1_700_000_456_000_i64
}})).unwrap()
}

#[test]
fn parses_verified_fields_and_timestamp_units_without_account_or_arbitrary_event_data() {
    let catalog = parse::parse(&fixture(
        Path::new("C:/synthetic/clip.mp4"),
        Path::new("C:/synthetic/image.png"),
    ))
    .unwrap();
    let m = &catalog.entries[0].data;
    assert_eq!(m.title.as_deref(), Some("Custom title"));
    assert!(m.title_difference);
    assert_eq!(m.game.as_deref(), Some("Synthetic game"));
    assert_eq!(m.created_ms, Some(1_700_000_000_000));
    assert_eq!(m.content_created_ms, Some(1_700_000_123_000));
    assert_eq!(m.last_edit_ms, Some(1_700_000_456_000));
    assert!(!catalog.entries[0].revision.contains("DO NOT STORE"));
    assert!(parse::parse(b"[]").is_err());
    assert!(parse::parse(b"{bad").is_err());
}
#[test]
fn missing_titles_and_conflicting_game_lookups_are_not_invented() {
    let catalog=parse::parse(&serde_json::to_vec(&json!({
        "one":{"FilePath":"C:/synthetic/one.mp4","GameTitle":null,"GameCategory":"game","Content":{"category":{"categoryId":"game","categoryName":"First"}}},
        "two":{"FilePath":"C:/synthetic/two.mp4","GameCategory":"game","Content":{"category":{"categoryId":"game","categoryName":"Second"}}},
        "three":{"FilePath":"C:/synthetic/three.mp4","GameCategory":"game"},"invalid":null
    })).unwrap()).unwrap();
    assert_eq!(catalog.skipped, 1);
    let third = catalog.entries.iter().find(|e| e.id == "three").unwrap();
    assert_eq!(third.data.game, None);
    assert_eq!(third.data.title, None);
    assert!(third.data.game_lookup_ambiguous);
    assert!(!third.data.game_conflict);
}

#[test]
fn title_fields_and_direct_game_names_are_not_false_conflicts_or_copied_to_originals() {
    let temp = tempfile::tempdir().unwrap();
    let paths: Vec<_> = ["one.mp4", "two.mp4", "three.mp4"]
        .map(|name| temp.path().join(name))
        .into();
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(temp.path()).unwrap())
        .unwrap();
    for path in &paths {
        fs::write(path, b"synthetic").unwrap();
        db.discover(
            root.id,
            1,
            &files::describe(&path.canonicalize().unwrap()).unwrap(),
        )
        .unwrap();
    }
    let bytes = serde_json::to_vec(&json!({
        "custom": {"FilePath":paths[0],"originalFilePath":paths[2],"GameTitle":"Synthetic game", "GameCategory":"shared",
            "Content":{"hasTitle":true,"contentTitle":"Synthetic highlight","category":{"categoryId":"shared","categoryName":"Synthetic game"}}},
        "local": {"FilePath":paths[1],"GameTitle":"Local highlight", "GameCategory":"shared",
            "Content":{"hasTitle":false,"contentTitle":"Default content label","category":{"categoryId":"shared","categoryName":"Other direct name"}}},
        "lookup": {"FilePath":paths[2],"GameTitle":"Third highlight","GameCategory":"shared"}
    })).unwrap();
    let catalog = parse::parse(&bytes).unwrap();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let (prepared, counts) = store::prepare(
        catalog.entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    assert_eq!(counts.matched, 4);
    assert_eq!(counts.conflicts, 0);
    assert_eq!(counts.warnings, 1);
    let custom = prepared.iter().find(|p| p.entry.id == "custom").unwrap();
    assert!(custom.entry.data.title_difference);
    assert_eq!(custom.entry.data.game.as_deref(), Some("Synthetic game"));
    assert!(custom
        .references
        .iter()
        .all(|r| !r.row.conflict && !r.row.warning));
    let local = prepared.iter().find(|p| p.entry.id == "local").unwrap();
    assert_eq!(local.entry.data.title.as_deref(), Some("Local highlight"));
    assert_eq!(local.entry.data.game.as_deref(), Some("Other direct name"));
    db.commit_medal(store::Commit {
        source: "source",
        path: "synthetic.json",
        revision: "v2",
        prepared: &prepared,
        additions: &HashSet::new(),
        cancel: &cancel,
        done: &done,
    })
    .unwrap();
    let page = db.query(Query::default()).unwrap();
    assert_eq!(page.total, 3);
    assert!(page
        .items
        .iter()
        .all(|r| !r.metadata_conflict && r.title.is_some()));
    assert!(page
        .items
        .iter()
        .any(|r| r.title.as_deref() == Some("Synthetic highlight")));
    assert!(page
        .items
        .iter()
        .any(|r| r.title.as_deref() == Some("Third highlight") && r.game.is_none()));
}

#[test]
fn duplicate_identical_references_are_not_conflicts_and_deduplicate_physical_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("new.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let bytes = serde_json::to_vec(&json!({
        "one":{"FilePath":path,"GameTitle":"Same title"},
        "two":{"FilePath":path,"GameTitle":"Same title"}
    }))
    .unwrap();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let (prepared, counts) = store::prepare(
        parse::parse(&bytes).unwrap().entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    assert_eq!(counts.new_files, 2);
    assert_eq!(counts.conflicts, 0);
    let additions = prepared
        .iter()
        .flat_map(|p| &p.references)
        .map(|r| r.row.index)
        .collect();
    db.commit_medal(store::Commit {
        source: "source",
        path: "synthetic.json",
        revision: "v2",
        prepared: &prepared,
        additions: &additions,
        cancel: &cancel,
        done: &done,
    })
    .unwrap();
    let page = db.query(Query::default()).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Same title"));
    assert!(!page.items[0].metadata_conflict);
}

#[test]
fn contradictory_direct_game_fields_are_preserved_without_selecting_a_name() {
    let bytes = serde_json::to_vec(&json!({"one":{"GameTitle":"Title","GameCategory":"game",
        "Content":{"category":{"categoryId":"game","categoryName":"First"}},
        "metadata":{"attributedCategoryId":"game","attributedCategoryName":"Second"}}}))
    .unwrap();
    let catalog = parse::parse(&bytes).unwrap();
    assert!(catalog.entries[0].data.game_conflict);
    assert!(!catalog.entries[0].data.game_lookup_ambiguous);
    assert!(catalog.entries[0].data.game.is_none());
    assert_eq!(
        catalog.entries[0].data.extra["gameNames"]["categoryName"],
        "First"
    );
    assert_eq!(
        catalog.entries[0].data.extra["gameNames"]["attributedCategoryName"],
        "Second"
    );
}

#[test]
fn import_is_idempotent_transactional_persistent_searchable_and_preserves_inspected_media() {
    let temp = tempfile::tempdir().unwrap();
    let videos = temp.path().join("videos");
    fs::create_dir(&videos).unwrap();
    let original = videos.join("original.mp4");
    let edited = videos.join("edited.mp4");
    let outside = temp.path().join("new.mp4");
    let image = temp.path().join("preview.png");
    for path in [&original, &edited, &outside] {
        fs::write(path, b"synthetic video bytes").unwrap();
    }
    fs::write(&image, b"invalid image, fallback tested separately").unwrap();
    let database = temp.path().join("library.sqlite3");
    let mut db = Db::open(&database).unwrap();
    let root = db
        .add_root(&files::local_directory(&videos).unwrap())
        .unwrap();
    let original_file = files::describe(&original.canonicalize().unwrap()).unwrap();
    let edited_file = files::describe(&edited.canonicalize().unwrap()).unwrap();
    db.discover(root.id, 1, &original_file).unwrap();
    db.discover(root.id, 1, &edited_file).unwrap();
    let id = db
        .query(Query {
            search: "edited".into(),
            ..Default::default()
        })
        .unwrap()
        .items[0]
        .id;
    let media:crate::media::MediaInfo=serde_json::from_value(json!({"container":"mp4","videoCodec":"hevc","width":1920,"height":1080,"frameRate":60.0,"duration":12.5,"fileSize":21,"bitrate":null,"audio":[]})).unwrap();
    db.save_media(id, &edited_file, &media).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&fixture(&edited, &image)).unwrap();
    value["synthetic-clip-id"]["originalFilePath"] = json!(original);
    value["new"] = json!({"FilePath":outside,"GameTitle":"New title"});
    let bytes = serde_json::to_vec(&value).unwrap();
    let catalog_path = temp.path().join("catalog.json");
    fs::write(&catalog_path, &bytes).unwrap();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let prepare = |db: &Db| {
        store::prepare(
            parse::parse(&bytes).unwrap().entries,
            db.medal_index("synthetic-source").unwrap(),
            &cancel,
            &done,
        )
        .unwrap()
    };
    let (prepared, counts) = prepare(&db);
    assert_eq!(counts.matched, 2);
    assert_eq!(counts.new_files, 1);
    let commit = |db: &mut Db, prepared: &[store::Prepared], additions: &HashSet<usize>| {
        db.commit_medal(store::Commit {
            source: "synthetic-source",
            path: catalog_path.to_str().unwrap(),
            revision: "v1",
            prepared,
            additions,
            cancel: &cancel,
            done: &done,
        })
    };
    assert_eq!(commit(&mut db, &prepared, &HashSet::new()).unwrap(), 2);
    let rows = db
        .query(Query {
            search: "Custom title".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(rows.items[0].id, id);
    assert_eq!(rows.items[0].duration, Some(12.5));
    assert_eq!(rows.items[0].title.as_deref(), Some("Custom title"));
    assert_eq!(
        db.query(Query {
            search: "Synthetic game".into(),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
    let original_row = db
        .query(Query {
            search: "original".into(),
            ..Default::default()
        })
        .unwrap()
        .items
        .remove(0);
    assert!(original_row.title.is_none());
    let (again, counts) = prepare(&db);
    assert_eq!(counts.unchanged, 2);
    commit(&mut db, &again, &HashSet::new()).unwrap();
    assert_eq!(db.query(Query::default()).unwrap().total, 2);
    let approved = again
        .iter()
        .flat_map(|p| &p.references)
        .filter(|r| r.row.status == "new")
        .map(|r| r.row.index)
        .collect();
    commit(&mut db, &again, &approved).unwrap();
    assert_eq!(db.query(Query::default()).unwrap().total, 3);
    cancel.store(true, Ordering::Release);
    assert!(commit(&mut db, &again, &HashSet::new()).is_err());
    assert_eq!(db.query(Query::default()).unwrap().total, 3);
    cancel.store(false, Ordering::Release);
    // A normal folder rescan preserves provider-owned metadata and inspected media.
    let generation = db.begin_scan(root.id).unwrap();
    db.discover(root.id, generation, &original_file).unwrap();
    db.discover(root.id, generation, &edited_file).unwrap();
    db.finish_discovery(root.id, generation, true).unwrap();
    db.finish_scan(root.id, "complete", None).unwrap();
    assert_eq!(
        db.provider_metadata(id).unwrap()[0].data.title.as_deref(),
        Some("Custom title")
    );
    assert!(db.cached(id, &edited_file).unwrap().is_some());
    let stored: String = db
        .connection
        .query_row(
            "SELECT metadata_json FROM provider_entries WHERE external_id='synthetic-clip-id'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!stored.contains("DO NOT STORE"));
    fs::rename(&catalog_path, temp.path().join("catalog-unavailable.json")).unwrap();
    db.remove_root(root.id).unwrap();
    assert!(db.resolve_file(id).is_ok());
    drop(db);
    let db = Db::open(&database).unwrap();
    assert_eq!(db.provider_metadata(id).unwrap().len(), 1);
    assert_eq!(
        db.query(Query {
            game: Some("Synthetic game".into()),
            ..Default::default()
        })
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        db.provider_metadata(id).unwrap()[0]
            .data
            .description
            .as_deref(),
        Some("Synthetic description")
    );
    assert!(db.cached(id, &edited_file).unwrap().is_some());
    assert_eq!(fs::read(&edited).unwrap(), b"synthetic video bytes");
}

#[test]
fn path_matching_is_absolute_and_does_not_guess_filenames() {
    assert!(
        parse::parse(br#"{"same":{"GameTitle":"First"},"same":{"GameTitle":"Second"}}"#).is_err()
    );
    assert_eq!(
        paths::key(r"\\?\C:\Clips\one.MP4"),
        paths::key("c:/clips/one.mp4")
    );
    assert!(paths::key("relative.mp4").is_none());
    assert!(paths::key(r"\\server\share\file.mp4").is_none());
    assert_ne!(paths::key("C:/one/clip.mp4"), paths::key("C:/two/clip.mp4"));
}

#[test]
fn version_one_migration_preserves_existing_library_and_rejects_newer_schemas() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("legacy.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!("../../src/library/migrations/001.sql"))
        .unwrap();
    connection
        .execute(
            "INSERT INTO recordings(id,size,status) VALUES(42,123,'ready')",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO media(file_id,duration,audio_count,payload) VALUES(42,8,4,'synthetic-payload')",[]).unwrap();
    drop(connection);
    let db = Db::open(&path).unwrap();
    assert_eq!(
        db.connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.connection
            .query_row("SELECT payload FROM media WHERE file_id=42", [], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "synthetic-payload"
    );
    db.connection
        .pragma_update(None, "user_version", 5)
        .unwrap();
    drop(db);
    assert!(Db::open(&path).is_err());
}

#[tokio::test]
async fn shutdown_cancels_native_import_work_and_waits_for_its_owned_completion() {
    let state = Arc::new(ImportState::default());
    state.begin("preparing").unwrap();
    assert!(state.begin("preparing").is_err());
    let worker = state.clone();
    let finish = tokio::spawn(async move {
        while !worker.cancel.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        assert!(!worker.is_idle());
        worker.finish(&Err("Import cancelled".into()));
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), state.shutdown())
        .await
        .unwrap();
    finish.await.unwrap();
    assert!(state.is_idle());
    assert!(state.begin("preparing").is_err());
}

#[tokio::test]
async fn reset_cancels_an_owned_import_and_allows_the_same_catalog_as_a_fresh_source() {
    let temp = tempfile::tempdir().unwrap();
    let video = temp.path().join("synthetic.mp4");
    fs::write(&video, b"synthetic video").unwrap();
    let bytes = fixture(&video, &temp.path().join("missing.png"));
    let library = super::super::Library::open(
        &temp.path().join("library.sqlite3"),
        &temp.path().join("cache"),
    )
    .unwrap();
    let analysis = Arc::new(crate::analysis::Analysis::default());
    let operation = library.operation().await.unwrap();
    library.medal.begin("importing").unwrap();
    let owner = library.clone();
    let worker = tauri::async_runtime::spawn(async move {
        let _operation = operation;
        let state = owner.medal.clone();
        let result = owner
            .database(move |db| {
                let catalog = parse::parse(&bytes)?;
                let (prepared, _) = store::prepare(
                    catalog.entries,
                    db.medal_index("synthetic-source")?,
                    &AtomicBool::new(false),
                    &AtomicUsize::new(0),
                )?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !state.cancel.load(Ordering::Acquire) && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                db.commit_medal(store::Commit {
                    source: "synthetic-source",
                    path: "synthetic.json",
                    revision: "v1",
                    prepared: &prepared,
                    additions: &HashSet::from([0]),
                    cancel: &state.cancel,
                    done: &state.done,
                })
            })
            .await;
        owner.medal.finish(&result);
        result
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        library.reset(analysis.clone(), || async { Ok(()) }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(worker.await.unwrap().is_err());
    assert!(library.medal.work.lock().unwrap().plan.is_none());
    assert_eq!(library.medal.done.load(Ordering::Acquire), 0);
    let bytes = fixture(&video, &temp.path().join("missing.png"));
    library
        .database(move |db| {
            assert_eq!(
                db.connection
                    .query_row("SELECT count(*) FROM provider_entries", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            let (prepared, counts) = store::prepare(
                parse::parse(&bytes)?.entries,
                db.medal_index("synthetic-source")?,
                &AtomicBool::new(false),
                &AtomicUsize::new(0),
            )?;
            assert_eq!(counts.new_files, 1);
            assert_eq!(counts.unchanged, 0);
            db.commit_medal(store::Commit {
                source: "synthetic-source",
                path: "synthetic.json",
                revision: "v2",
                prepared: &prepared,
                additions: &HashSet::from([0]),
                cancel: &AtomicBool::new(false),
                done: &AtomicUsize::new(0),
            })?;
            assert_eq!(db.query(Query::default())?.total, 1);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(fs::read(video).unwrap(), b"synthetic video");
    library.shutdown(&analysis).await;
}

#[test]
fn updates_preserve_overrides_missing_links_and_ffprobe_and_do_not_guess_other_directories() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("recordings");
    fs::create_dir(&root_path).unwrap();
    let path = root_path.join("clip.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let mut db = Db::open(&temp.path().join("library.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(&root_path).unwrap())
        .unwrap();
    db.discover(
        root.id,
        1,
        &files::describe(&path.canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let id = db.query(Query::default()).unwrap().items[0].id;
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let import = |db: &mut Db, title: &str| {
        let bytes=serde_json::to_vec(&json!({"one":{"FilePath":path,"GameTitle":title,"GameCategory":"game","Content":{"category":{"categoryId":"game","categoryName":"Synthetic game"}}}})).unwrap();
        let (plan, _) = store::prepare(
            parse::parse(&bytes).unwrap().entries,
            db.medal_index("source").unwrap(),
            &cancel,
            &done,
        )
        .unwrap();
        db.commit_medal(store::Commit {
            source: "source",
            path: "catalog.json",
            revision: "v1",
            prepared: &plan,
            additions: &HashSet::new(),
            cancel: &cancel,
            done: &done,
        })
        .unwrap();
    };
    import(&mut db, "First");
    db.connection
        .execute(
            "INSERT INTO recording_overrides VALUES(?1,'My own title','My own game')",
            [id],
        )
        .unwrap();
    import(&mut db, "Updated");
    let page = db
        .query(Query {
            search: "My own title".into(),
            game: Some("My own game".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(
        db.provider_metadata(id).unwrap()[0].data.title.as_deref(),
        Some("Updated")
    );
    fs::rename(&path, temp.path().join("temporarily-away.mp4")).unwrap();
    import(&mut db, "While missing");
    assert_eq!(
        db.provider_metadata(id).unwrap()[0].data.title.as_deref(),
        Some("While missing")
    );
    assert!(db.resolve_file(id).is_err());
    let bytes=serde_json::to_vec(&json!({"other":{"FilePath":temp.path().join("other/clip.mp4"),"GameTitle":"Must not match by basename"}})).unwrap();
    let (plan, counts) = store::prepare(
        parse::parse(&bytes).unwrap().entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    assert_eq!(counts.missing, 1);
    assert!(plan[0].references[0].file_id.is_none());
}

#[test]
fn conflicting_entries_are_visible_in_preview_and_do_not_choose_an_arbitrary_title() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clip.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let mut db = Db::open(&temp.path().join("db.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(temp.path()).unwrap())
        .unwrap();
    db.discover(
        root.id,
        1,
        &files::describe(&path.canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let edit = temp.path().join("edit.mp4");
    fs::write(&edit, b"synthetic edit").unwrap();
    let bytes=serde_json::to_vec(&json!({"one":{"FilePath":path,"GameTitle":"First"},"two":{"FilePath":path,"GameTitle":"Different"},"edit":{"FilePath":edit,"originalFilePath":path,"GameTitle":"Related edit"}})).unwrap();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let (plan, counts) = store::prepare(
        parse::parse(&bytes).unwrap().entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    assert_eq!(counts.conflicts, 2);
    assert!(
        !plan
            .iter()
            .find(|p| p.entry.id == "edit")
            .unwrap()
            .references
            .iter()
            .find(|r| r.row.relation == "original")
            .unwrap()
            .row
            .conflict
    );
    db.commit_medal(store::Commit {
        source: "source",
        path: "catalog.json",
        revision: "v1",
        prepared: &plan,
        additions: &HashSet::new(),
        cancel: &cancel,
        done: &done,
    })
    .unwrap();
    let page = db.query(Query::default()).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].title, None);
    assert!(page.items[0].metadata_conflict);
}

#[test]
fn contradictory_games_for_one_file_are_reported_without_choosing_a_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clip.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let mut db = Db::open(&temp.path().join("db.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(temp.path()).unwrap())
        .unwrap();
    db.discover(
        root.id,
        1,
        &files::describe(&path.canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let bytes=serde_json::to_vec(&json!({"one":{"FilePath":path,"GameTitle":"Same title","GameCategory":"one","Content":{"category":{"categoryId":"one","categoryName":"First game"}}},"two":{"FilePath":path,"GameTitle":"Same title","GameCategory":"two","Content":{"category":{"categoryId":"two","categoryName":"Other game"}}}})).unwrap();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let (plan, counts) = store::prepare(
        parse::parse(&bytes).unwrap().entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    assert_eq!(counts.conflicts, 2);
    db.commit_medal(store::Commit {
        source: "source",
        path: "catalog.json",
        revision: "v1",
        prepared: &plan,
        additions: &HashSet::new(),
        cancel: &cancel,
        done: &done,
    })
    .unwrap();
    let row = db.query(Query::default()).unwrap().items.remove(0);
    assert_eq!(row.title.as_deref(), Some("Same title"));
    assert!(row.game.is_none());
    assert!(row.metadata_conflict);
}

#[test]
fn cancellation_after_transaction_writes_rolls_everything_back() {
    let temp = tempfile::tempdir().unwrap();
    let mut db = Db::open(&temp.path().join("db.sqlite3")).unwrap();
    let values: serde_json::Map<_, _> = (0..5000)
        .map(|i| {
            (
                format!("synthetic-{i}"),
                json!({"GameTitle":"Synthetic title"}),
            )
        })
        .collect();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let (plan, _) = store::prepare(
        parse::parse(&serde_json::to_vec(&values).unwrap())
            .unwrap()
            .entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    done.store(0, Ordering::Release);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let start = std::time::Instant::now();
            while done.load(Ordering::Acquire) == 0 {
                assert!(
                    start.elapsed() < std::time::Duration::from_secs(10),
                    "Import did not reach its first transaction write"
                );
                std::thread::yield_now();
            }
            cancel.store(true, Ordering::Release);
        });
        assert!(db
            .commit_medal(store::Commit {
                source: "source",
                path: "catalog.json",
                revision: "v1",
                prepared: &plan,
                additions: &HashSet::new(),
                cancel: &cancel,
                done: &done
            })
            .is_err());
    });
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM provider_entries", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.connection
            .query_row("SELECT count(*) FROM sources WHERE kind='medal'", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn recording_replacements_after_preview_are_rejected_without_partial_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clip.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let mut db = Db::open(&temp.path().join("db.sqlite3")).unwrap();
    let bytes = fixture(&path, &temp.path().join("none.jpg"));
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let (plan, _) = store::prepare(
        parse::parse(&bytes).unwrap().entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    fs::write(&path, b"changed synthetic fixture").unwrap();
    assert!(db
        .commit_medal(store::Commit {
            source: "source",
            path: "catalog.json",
            revision: "v1",
            prepared: &plan,
            additions: &HashSet::from([0]),
            cancel: &cancel,
            done: &done
        })
        .is_err());
    assert_eq!(db.query(Query::default()).unwrap().total, 0);
}

#[test]
fn catalog_revisions_detect_changes_and_size_and_path_limits_are_enforced() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("clips.json");
    fs::write(&file, b"{}").unwrap();
    let before = catalog_file(&file).unwrap().1;
    fs::write(&file, b"{\"synthetic\":{\"GameTitle\":\"test\"}}").unwrap();
    assert_ne!(catalog_file(&file).unwrap().1, before);
    let oversized = temp.path().join("oversized.json");
    fs::File::create(&oversized)
        .unwrap()
        .set_len(MAX_CATALOG + 1)
        .unwrap();
    assert!(catalog_file(&oversized).is_err());
    assert!(catalog_file(Path::new(r"\\server\share\clips.json")).is_err());
}

#[test]
fn large_catalog_snapshots_reuse_sanitized_records_without_resurrecting_historical_entries() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clip.mp4");
    fs::write(&path, b"synthetic").unwrap();
    let mut db = Db::open(&temp.path().join("db.sqlite3")).unwrap();
    let root = db
        .add_root(&files::local_directory(temp.path()).unwrap())
        .unwrap();
    db.discover(
        root.id,
        1,
        &files::describe(&path.canonicalize().unwrap()).unwrap(),
    )
    .unwrap();
    let values:serde_json::Map<_,_>=(0..3000).map(|i|(format!("synthetic-{i:04}"),json!({"FilePath":path,"GameTitle":"Synthetic title","TimeCreated":1_700_000_000,"userId":"DO NOT STORE"}))).collect();
    let raw = serde_json::to_vec(&values).unwrap();
    let cancel = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let start = std::time::Instant::now();
    let (plan, _) = store::prepare(
        parse::parse(&raw).unwrap().entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    let prepared = start.elapsed();
    let snapshot = serde_json::to_string(&(
        parse::NORMALIZATION_VERSION,
        "fingerprint",
        1,
        plan.iter().map(|p| &p.entry.id).collect::<Vec<_>>(),
    ))
    .unwrap();
    let start = std::time::Instant::now();
    db.commit_medal(store::Commit {
        source: "source",
        path: "catalog.json",
        revision: &snapshot,
        prepared: &plan,
        additions: &HashSet::new(),
        cancel: &cancel,
        done: &done,
    })
    .unwrap();
    let committed = start.elapsed();
    let cached = db
        .cached_medal_catalog("source", "fingerprint", u64::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(cached.skipped, 1);
    assert_eq!(cached.entries.len(), 3000);
    assert!(db
        .cached_medal_catalog("source", "fingerprint", 0)
        .unwrap()
        .is_none());
    let start = std::time::Instant::now();
    let warm_source = db
        .cached_medal_catalog("source", "fingerprint", raw.len() as u64)
        .unwrap()
        .unwrap_or_else(|| parse::parse(&raw).unwrap());
    let (again, counts) = store::prepare(
        warm_source.entries,
        db.medal_index("source").unwrap(),
        &cancel,
        &done,
    )
    .unwrap();
    assert_eq!(counts.unchanged, 3000);
    let warm_prepared = start.elapsed();
    let start = std::time::Instant::now();
    db.commit_medal(store::Commit {
        source: "source",
        path: "catalog.json",
        revision: &snapshot,
        prepared: &again,
        additions: &HashSet::new(),
        cancel: &cancel,
        done: &done,
    })
    .unwrap();
    let warm_committed = start.elapsed();
    assert_eq!(db.query(Query::default()).unwrap().total, 1);
    assert!(db
        .cached_medal_catalog("source", "changed-fingerprint", u64::MAX)
        .unwrap()
        .is_none());
    let last = plan.last().unwrap().entry.id.clone();
    let revision = serde_json::to_string(&(
        parse::NORMALIZATION_VERSION,
        "smaller-fingerprint",
        0,
        vec![last.clone()],
    ))
    .unwrap();
    db.connection
        .execute("UPDATE provider_imports SET revision=?1", [revision])
        .unwrap();
    let smaller = db
        .cached_medal_catalog("source", "smaller-fingerprint", u64::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(smaller.entries.len(), 1);
    assert_eq!(smaller.entries[0].id, last);
    println!("3000 synthetic references to one file: cold prepare={}ms commit={}ms; warm prepare={}ms commit={}ms",prepared.as_millis(),committed.as_millis(),warm_prepared.as_millis(),warm_committed.as_millis());
}
