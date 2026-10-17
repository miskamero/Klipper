use super::*;

#[test]
fn games_are_collection_scoped_case_insensitive_and_filter_before_pagination() {
    let temp = tempfile::tempdir().unwrap();
    let videos = temp.path().join("videos");
    let child = videos.join("nested");
    let outside = temp.path().join("other");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&outside).unwrap();
    let database = temp.path().join("library.sqlite3");
    let mut db = Db::open(&database).unwrap();
    let root = db
        .add_root(&files::local_directory(&videos).unwrap())
        .unwrap();
    let overlap = db
        .add_root(&files::local_directory(&child).unwrap())
        .unwrap();
    let other = db
        .add_root(&files::local_directory(&outside).unwrap())
        .unwrap();
    let mut entries = serde_json::Map::new();
    for i in 0..48 {
        let path = if i == 47 { &outside } else { &child }.join(format!("clip-{i:02}.mp4"));
        fs::write(&path, b"synthetic video").unwrap();
        let file = files::describe(&path.canonicalize().unwrap()).unwrap();
        db.discover(if i == 47 { other.id } else { root.id }, 1, &file)
            .unwrap();
        if i != 47 {
            db.discover(overlap.id, 1, &file).unwrap();
        }
        let game = match i {
            0..=44 => Some(if i % 2 == 0 { "Arena" } else { "arena" }),
            45 => Some("Counter-Strike 2"),
            47 => Some("Different game"),
            _ => None,
        };
        entries.insert(format!("clip-{i}"), json!({
            "FilePath":path,"GameTitle":format!("Highlight {i}"),
            "GameCategory": if game.is_some() { format!("game-{i}") } else { "unmapped-id".into() },
            "Content":{"category":{"categoryName":game}}
        }));
    }
    let (plan, _) = store::prepare(
        parse::parse(&serde_json::to_vec(&entries).unwrap())
            .unwrap()
            .entries,
        db.medal_index("source").unwrap(),
        &AtomicBool::new(false),
        &AtomicUsize::new(0),
    )
    .unwrap();
    db.commit_medal(store::Commit {
        source: "source",
        path: "synthetic.json",
        revision: "v1",
        prepared: &plan,
        additions: &HashSet::new(),
        cancel: &AtomicBool::new(false),
        done: &AtomicUsize::new(0),
    })
    .unwrap();
    let scope = Query {
        root_id: Some(root.id),
        game: Some("ARENA".into()),
        sort: "name".into(),
        ..Default::default()
    };
    let first = db.query(scope.clone()).unwrap();
    assert_eq!(first.total, 45);
    assert_eq!(first.items.len(), 40);
    assert_eq!(first.games, ["Arena", "Counter-Strike 2"]);
    assert!(first.unknown_game_available);
    let second = db
        .query(Query {
            offset: 40,
            ..scope.clone()
        })
        .unwrap();
    assert_eq!(second.items.len(), 5);
    assert!(second
        .items
        .iter()
        .all(|r| !first.items.iter().any(|a| a.id == r.id)));
    assert_eq!(
        db.query(Query {
            search: "clip-04.mp4".into(),
            filter: "pending".into(),
            ..scope.clone()
        })
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        db.query(Query {
            codec: "hevc".into(),
            ..scope.clone()
        })
        .unwrap()
        .total,
        0
    );
    let (inspected, _) = db.resolve_file(first.items[0].id).unwrap();
    let media = serde_json::from_value(json!({"container":"mp4","videoCodec":"hevc","width":1920,"height":1080,"frameRate":60.0,"duration":12.5,"fileSize":15,"bitrate":null,"audio":[]})).unwrap();
    db.save_media(first.items[0].id, &inspected, &media)
        .unwrap();
    assert_eq!(
        db.query(Query {
            codec: "hevc".into(),
            filter: "available".into(),
            ..scope.clone()
        })
        .unwrap()
        .total,
        1
    );
    assert_eq!(
        db.query(Query {
            filter: "pending".into(),
            ..scope.clone()
        })
        .unwrap()
        .total,
        44
    );
    // Facets stay available even when the active search/codec has no results.
    assert_eq!(
        db.query(Query {
            search: "no match".into(),
            codec: "hevc".into(),
            ..scope
        })
        .unwrap()
        .games,
        ["Arena", "Counter-Strike 2"]
    );
    let folder = db
        .navigation()
        .unwrap()
        .folders
        .into_iter()
        .find(|f| f.parent_id.is_some())
        .unwrap();
    let unknown = db
        .query(Query {
            folder_id: Some(folder.id),
            unknown_game: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(unknown.total, 1);
    assert_eq!(unknown.items[0].name, "clip-46.mp4");
    assert!(unknown.items[0].game.is_none());
    let all = db.query(Query::default()).unwrap();
    assert_eq!(all.total, 48);
    assert_eq!(all.games, ["Arena", "Counter-Strike 2", "Different game"]);
    assert_eq!(
        db.query(Query {
            provider: Some("medal".into()),
            game: Some("Arena".into()),
            ..Default::default()
        })
        .unwrap()
        .total,
        45
    );
    assert_eq!(
        db.query(Query {
            root_id: Some(overlap.id),
            ..Default::default()
        })
        .unwrap()
        .total,
        47
    );
    let other_page = db
        .query(Query {
            root_id: Some(other.id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(other_page.games, ["Different game"]);
    assert!(!other_page.unknown_game_available);
    assert!(db
        .query(Query {
            unknown_game: true,
            game: Some("Arena".into()),
            ..Default::default()
        })
        .is_err());
    drop(db);
    let db = Db::open(&database).unwrap();
    assert_eq!(
        db.query(Query {
            game: Some("arena".into()),
            ..Default::default()
        })
        .unwrap()
        .total,
        45
    );
}
