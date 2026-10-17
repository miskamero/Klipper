use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    #[serde(default)]
    pub search: String,
    pub root_id: Option<i64>,
    pub folder_id: Option<String>,
    pub provider: Option<String>,
    #[serde(default)]
    pub unknown_game: bool,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub codec: String,
    pub game: Option<String>,
    #[serde(default)]
    pub offset: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingRow {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size: i64,
    pub modified_ms: Option<i64>,
    pub revision: Option<String>,
    pub status: String,
    pub availability: String,
    pub duration: Option<f64>,
    pub video_codec: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_rate: Option<f64>,
    pub audio_count: Option<i64>,
    pub error: Option<String>,
    pub title: Option<String>,
    pub game: Option<String>,
    pub medal_created_ms: Option<i64>,
    pub metadata_conflict: bool,
    pub preview_revision: Option<String>,
}

#[derive(Serialize)]
pub struct Page {
    pub items: Vec<RecordingRow>,
    pub total: i64,
    pub limit: u32,
    pub games: Vec<String>,
    #[serde(rename = "unknownGameAvailable")]
    pub unknown_game_available: bool,
}

pub fn query(db: &super::db::Db, query: Query) -> Result<Page, String> {
    let connection = &db.connection;
    let mut root_id = query.root_id;
    let prefix = if let Some(folder) = &query.folder_id {
        let (root, path) = db.folder_scope(folder)?;
        if root_id.is_some_and(|id| id != root) {
            return Err("Folder does not belong to the selected root".into());
        }
        root_id = Some(root);
        Some(path)
    } else {
        None
    };
    if (query.unknown_game && query.game.is_some())
        || query.search.chars().count() > 200
        || query.offset > 10_000_000
        || query.game.as_ref().is_some_and(|s| s.chars().count() > 512)
        || query.provider.as_ref().is_some_and(|s| {
            s.len() > 64 || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err("Library query is too large".into());
    }
    let sort = match query.sort.as_str() {
        "" | "date" => "coalesce(medal_created_ms,modified_ms)",
        "name" => "coalesce(title,name) COLLATE NOCASE",
        "duration" => "duration",
        "size" => "size",
        _ => return Err("Unknown sort order".into()),
    };
    if !["", "all", "available", "missing", "pending", "error"].contains(&query.filter.as_str())
        || !["", "all", "hevc", "h264"].contains(&query.codec.as_str())
    {
        return Err("Unknown library filter".into());
    }
    let search = format!(
        "%{}%",
        query
            .search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let cte = "WITH candidates AS (
        SELECT f.id,l.name,l.path,f.size,f.modified_ms,f.modified_ns,f.status,l.availability,f.error,
        coalesce(o.title,p.title) AS title,nullif(trim(coalesce(o.game,p.game)),'') AS game,p.created_ms AS medal_created_ms,coalesce(p.conflict,0) AS metadata_conflict,
        coalesce((SELECT json_extract(value_json,'$[1]') FROM provider_choices c WHERE c.file_id=f.id AND c.field='preview'),(SELECT min(pp.revision) FROM provider_previews pp JOIN provider_links pl ON pl.source_id=pp.source_id AND pl.external_id=pp.external_id AND pl.relation=pp.relation WHERE pl.file_id=f.id AND pl.file_size=f.size AND pl.file_modified_ns IS f.modified_ns HAVING count(DISTINCT pp.path)=1 AND count(DISTINCT pp.revision)=1)) AS preview_revision,
        CASE WHEN f.status='ready' THEN m.duration END AS duration,
        CASE WHEN f.status='ready' THEN m.video_codec END AS video_codec,
        CASE WHEN f.status='ready' THEN m.width END AS width,
        CASE WHEN f.status='ready' THEN m.height END AS height,
        CASE WHEN f.status='ready' THEN m.frame_rate END AS frame_rate,
        CASE WHEN f.status='ready' THEN m.audio_count END AS audio_count,
        row_number() OVER(PARTITION BY f.id ORDER BY
            CASE l.availability WHEN 'available' THEN 0 WHEN 'inaccessible' THEN 1 WHEN 'unverified' THEN 2 ELSE 3 END,l.location_id) AS preferred
        FROM recordings f JOIN library_locations l ON l.file_id=f.id
        LEFT JOIN media m ON m.file_id=f.id
        LEFT JOIN provider_display p ON p.file_id=f.id LEFT JOIN recording_overrides o ON o.file_id=f.id
        WHERE (?1 IS NULL OR l.root_id=?1)
        AND (?7 IS NULL OR (l.availability='available' AND EXISTS(SELECT 1 FROM provider_links pl JOIN sources ps ON ps.id=pl.source_id WHERE pl.file_id=f.id AND ps.kind=?7)))
        AND (?8 IS NULL OR substr(replace(l.path,'\\','/'),1,length(?8))=?8 COLLATE FOLDERCASE)
        AND (l.name LIKE ?2 ESCAPE '\\' OR l.path LIKE ?2 ESCAPE '\\' OR coalesce(o.title,p.title) LIKE ?2 ESCAPE '\\' OR coalesce(o.game,p.game) LIKE ?2 ESCAPE '\\' OR EXISTS(SELECT 1 FROM provider_links sl JOIN provider_entries se ON se.source_id=sl.source_id AND se.external_id=sl.external_id WHERE sl.file_id=f.id AND sl.relation='primary' AND (se.title LIKE ?2 ESCAPE '\\' OR se.game LIKE ?2 ESCAPE '\\')))
    ), filtered AS (SELECT * FROM candidates WHERE preferred=1
        AND (?3 IN ('','all') OR availability=?3 OR status=?3)
        AND (?4 IN ('','all') OR video_codec=?4)
        AND (?6 IS NULL OR game=?6 COLLATE NOCASE) AND (?9=0 OR game IS NULL)) ";
    let cte = cte.replace(
        "FOLDERCASE",
        if cfg!(windows) { "NOCASE" } else { "BINARY" },
    );
    let total = connection
        .query_row(
            &format!("{cte} SELECT count(*) FROM filtered"),
            params![
                root_id,
                search,
                query.filter,
                query.codec,
                query.offset,
                query.game,
                query.provider,
                prefix,
                query.unknown_game
            ],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let direction = if query.descending { "DESC" } else { "ASC" };
    // Only the fixed whitelist above contributes SQL syntax. All user input stays parameterized.
    let mut statement = connection
        .prepare(&format!(
            "{cte} SELECT id,name,path,size,modified_ms,status,availability,
        duration,video_codec,width,height,frame_rate,audio_count,error,modified_ns,title,game,medal_created_ms,metadata_conflict,preview_revision FROM filtered
        ORDER BY {sort} {direction},id ASC LIMIT 40 OFFSET ?5"
        ))
        .map_err(|e| e.to_string())?;
    let items = statement
        .query_map(
            params![
                root_id,
                search,
                query.filter,
                query.codec,
                query.offset,
                query.game,
                query.provider,
                prefix,
                query.unknown_game
            ],
            |r| {
                Ok(RecordingRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: r.get(2)?,
                    size: r.get(3)?,
                    modified_ms: r.get(4)?,
                    revision: r.get(14)?,
                    status: r.get(5)?,
                    availability: r.get(6)?,
                    duration: r.get(7)?,
                    video_codec: r.get(8)?,
                    width: r.get(9)?,
                    height: r.get(10)?,
                    frame_rate: r.get(11)?,
                    audio_count: r.get(12)?,
                    error: r.get(13)?,
                    title: r.get(15)?,
                    game: r.get(16)?,
                    medal_created_ms: r.get(17)?,
                    metadata_conflict: r.get(18)?,
                    preview_revision: r.get(19)?,
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    // Facets describe the selected collection, independent of the current page or filters.
    // NOCASE merges ASCII case variants only; aliases and unverified IDs are never guessed.
    let mut statement = connection
        .prepare(&format!("{cte} SELECT min(game COLLATE BINARY) FROM candidates WHERE preferred=1 GROUP BY game COLLATE NOCASE ORDER BY 1 COLLATE NOCASE"))
        .map_err(|e| e.to_string())?;
    let facets = statement
        .query_map(
            params![
                root_id,
                "%",
                "all",
                "all",
                0,
                Option::<String>::None,
                query.provider,
                prefix,
                false
            ],
            |r| r.get::<_, Option<String>>(0),
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let unknown_game_available = facets.iter().any(Option::is_none);
    Ok(Page {
        items,
        total,
        limit: 40,
        games: facets.into_iter().flatten().collect(),
        unknown_game_available,
    })
}
