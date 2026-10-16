use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    #[serde(default)]
    pub search: String,
    pub root_id: Option<i64>,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub codec: String,
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
}

#[derive(Serialize)]
pub struct Page {
    pub items: Vec<RecordingRow>,
    pub total: i64,
    pub limit: u32,
}

pub fn query(connection: &Connection, query: Query) -> Result<Page, String> {
    if query.search.chars().count() > 200 || query.offset > 10_000_000 {
        return Err("Library query is too large".into());
    }
    let sort = match query.sort.as_str() {
        "" | "date" => "modified_ms",
        "name" => "name COLLATE NOCASE",
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
        SELECT f.id,l.name,l.path,f.size,f.modified_ms,f.modified_ns,f.status,e.availability,f.error,
        CASE WHEN f.status='ready' THEN m.duration END AS duration,
        CASE WHEN f.status='ready' THEN m.video_codec END AS video_codec,
        CASE WHEN f.status='ready' THEN m.width END AS width,
        CASE WHEN f.status='ready' THEN m.height END AS height,
        CASE WHEN f.status='ready' THEN m.frame_rate END AS frame_rate,
        CASE WHEN f.status='ready' THEN m.audio_count END AS audio_count,
        row_number() OVER(PARTITION BY f.id ORDER BY
            CASE e.availability WHEN 'available' THEN 0 WHEN 'inaccessible' THEN 1 WHEN 'unverified' THEN 2 ELSE 3 END,l.id) AS preferred
        FROM recordings f JOIN locations l ON l.file_id=f.id JOIN root_entries e ON e.location_id=l.id
        LEFT JOIN media m ON m.file_id=f.id
        WHERE (?1 IS NULL OR e.root_id=?1) AND (l.name LIKE ?2 ESCAPE '\\' OR l.path LIKE ?2 ESCAPE '\\')
    ), filtered AS (SELECT * FROM candidates WHERE preferred=1
        AND (?3 IN ('','all') OR availability=?3 OR status=?3)
        AND (?4 IN ('','all') OR video_codec=?4)) ";
    let total = connection
        .query_row(
            &format!("{cte} SELECT count(*) FROM filtered"),
            params![query.root_id, search, query.filter, query.codec],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let direction = if query.descending { "DESC" } else { "ASC" };
    // Only the fixed whitelist above contributes SQL syntax. All user input stays parameterized.
    let mut statement = connection
        .prepare(&format!(
            "{cte} SELECT id,name,path,size,modified_ms,status,availability,
        duration,video_codec,width,height,frame_rate,audio_count,error,modified_ns FROM filtered
        ORDER BY {sort} {direction},id ASC LIMIT 40 OFFSET ?5"
        ))
        .map_err(|e| e.to_string())?;
    let items = statement
        .query_map(
            params![
                query.root_id,
                search,
                query.filter,
                query.codec,
                query.offset
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
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Page {
        items,
        total,
        limit: 40,
    })
}
