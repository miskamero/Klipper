use super::parse::{self, Catalog, Entry, Metadata};
use rusqlite::{
    backup::{Backup, StepResult},
    types::ValueRef,
    Connection, OpenFlags,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MAX_BYTES: i64 = 64 * 1024 * 1024;
const MAX_RECORD: usize = 256 * 1024;

fn numeric(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Integer(value) => json!(value),
        ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn check(cancel: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Import cancelled".into())
    } else if Instant::now() >= deadline {
        Err("Medal database read timed out. Try again when Medal is less busy.".into())
    } else {
        Ok(())
    }
}

pub fn load(path: &Path, cancel: Arc<AtomicBool>) -> Result<Catalog, String> {
    load_with_timeout(path, cancel, Duration::from_secs(20))
}
pub(super) fn load_with_timeout(
    path: &Path,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<Catalog, String> {
    let deadline = Instant::now() + timeout;
    check(&cancel, deadline)?;
    let mut header = [0; 20];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| "The selected database cannot be read.")?;
    if &header[..16] != b"SQLite format 3\0" {
        return Err("The selected file is not a SQLite database.".into());
    }
    // A read-only WAL connection must use existing sidecars, never create provider files.
    if header[18] == 2 {
        for suffix in ["-wal", "-shm"] {
            let sidecar = format!("{}{suffix}", path.display());
            if std::fs::symlink_metadata(&sidecar).is_ok() {
                super::paths::local_file(&sidecar)?;
            }
            if std::fs::File::open(&sidecar).is_err() {
                #[cfg(windows)]
                return read_snapshot(offline_snapshot(path)?, cancel, deadline);
                #[cfg(not(windows))]
                return Err("This WAL database has unavailable sidecars. Select an exported SQLite snapshot, or retry while Medal has its database open.".into());
            }
        }
    }
    let source = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "Medal database could not be opened read-only.")?;
    source
        .busy_timeout(Duration::from_millis(100))
        .map_err(|_| "Could not configure database timeout")?;
    source
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| "Database read-only setup failed")?;
    let source_page_size: i64 = source
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(|_| "Invalid source database")?;
    let mut snapshot = Connection::open_in_memory()
        .map_err(|_| "Could not allocate a private database snapshot")?;
    {
        let backup =
            Backup::new(&source, &mut snapshot).map_err(|_| "Could not snapshot Medal database")?;
        loop {
            check(&cancel, deadline)?;
            match backup
                .step(64)
                .map_err(|_| "Medal database snapshot failed; no data was imported.")?
            {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => return Err("Unsupported SQLite backup result".into()),
            }
            // The source size is also checked before opening. Bound a WAL-expanded snapshot.
            if i64::from(backup.progress().pagecount) * source_page_size > MAX_BYTES {
                return Err("Medal database snapshot exceeds the safety limit.".into());
            }
        }
    }
    drop(source);
    read_snapshot(snapshot, cancel, deadline)
}

// A closed WAL database can have no sidecars. Windows sharing denies concurrent writers
// and deletion while copying; if Medal holds a writer, fail without touching its files.
// Only the private image's journal header is normalized, as documented for deserialize:
// https://sqlite.org/c3ref/deserialize.html
#[cfg(windows)]
fn offline_snapshot(path: &Path) -> Result<Connection, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let file=std::fs::OpenOptions::new().read(true).share_mode(1).open(path).map_err(|_|"Database sidecars are unavailable and Medal still holds a writer. Retry after closing Medal, or select an exported snapshot.")?;
    if std::fs::metadata(format!("{}-wal", path.display())).is_ok_and(|m| m.len() > 0) {
        return Err("The database has pending WAL data but no readable WAL sidecar set. Retry when Medal is idle.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the closed database")?;
    if bytes.len() < 100 || bytes.len() > MAX_BYTES as usize || &bytes[..16] != b"SQLite format 3\0"
    {
        return Err("Invalid or oversized SQLite snapshot".into());
    }
    bytes[18] = 1;
    bytes[19] = 1;
    let mut snapshot = Connection::open_in_memory().map_err(|_| "Could not allocate snapshot")?;
    snapshot
        .deserialize_read_exact(
            rusqlite::MAIN_DB,
            std::io::Cursor::new(&bytes),
            bytes.len(),
            true,
        )
        .map_err(|_| "Could not open the closed database snapshot")?;
    Ok(snapshot)
}

fn read_snapshot(
    snapshot: Connection,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Catalog, String> {
    let pages: i64 = snapshot
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .map_err(|_| "Invalid snapshot")?;
    let page_size: i64 = snapshot
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(|_| "Invalid snapshot")?;
    if pages * page_size > MAX_BYTES {
        return Err("Medal database snapshot exceeds 64 MiB.".into());
    }
    snapshot
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| "Snapshot setup failed")?;
    let interrupt = cancel.clone();
    snapshot
        .progress_handler(
            1000,
            Some(move || interrupt.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| "Could not configure cancellable snapshot reading")?;
    parse_snapshot(&snapshot, &cancel, deadline)
}

fn parse_snapshot(
    db: &Connection,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Catalog, String> {
    let table: Option<String> = db
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name='contents'",
            [],
            |r| r.get(0),
        )
        .ok();
    if table.as_deref() != Some("table") {
        return Err("Unsupported Medal database: contents must be a table.".into());
    }
    let definition: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='contents'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "Invalid Medal schema")?;
    if definition
        .to_ascii_lowercase()
        .split_whitespace()
        .take(3)
        .eq(["create", "virtual", "table"])
    {
        return Err("Unsupported Medal database: virtual contents tables are not allowed.".into());
    }
    let mut schema = db
        .prepare("PRAGMA table_info(contents)")
        .map_err(|_| "Invalid Medal schema")?;
    let columns = schema
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|_| "Invalid Medal schema")?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|_| "Invalid Medal schema")?;
    if [
        "created_at",
        "category_id",
        "video_path",
        "image_path",
        "thumbnail_path",
        "metadata",
        "remote_content_id",
        "local_content_id",
        "parent_id",
        "film_reel_path",
    ]
    .iter()
    .any(|name| !columns.contains(*name))
    {
        return Err(
            "Unsupported Medal database schema: required recording fields are missing.".into(),
        );
    }
    // Decode the optional remote timestamp as a SQLite scalar, separately from
    // the JSON object: even an overflowing JSONB number must not discard the clip.
    let mut statement = db.prepare("WITH bounded AS (
        SELECT local_content_id,remote_content_id,video_path,thumbnail_path,created_at,category_id,
        CASE WHEN length(metadata)<=262144 AND (json_valid(metadata,8) OR json_valid(metadata)) THEN metadata END AS payload
        FROM contents LIMIT 100001
    ) SELECT local_content_id,remote_content_id,video_path,thumbnail_path,created_at,category_id,length(payload),
        json_remove(payload,'$.remoteContent.created'),json_extract(payload,'$.remoteContent.created'),json_type(payload,'$.remoteContent.created')
        FROM bounded").map_err(|_| "Unsupported Medal contents schema")?;
    let mut rows = statement
        .query([])
        .map_err(|_| "Medal records could not be read")?;
    let mut entries = Vec::new();
    let mut skipped = 0;
    let mut invalid_identities = 0;
    let mut ids = HashSet::new();
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut normalized_bytes = 0;
    while let Some(row) = rows
        .next()
        .map_err(|_| "Invalid or interrupted Medal record data")?
    {
        check(cancel, deadline)?;
        if entries.len() + skipped >= 100_000 {
            return Err("Medal database exceeds 100,000 records.".into());
        }
        let id = match row.get_ref(0).map_err(|_| "Unreadable Medal identity")? {
            ValueRef::Text(bytes) => std::str::from_utf8(bytes)
                .ok()
                .filter(|s| !s.trim().is_empty() && s.len() <= 256),
            _ => None,
        };
        let Some(id) = id.map(str::to_owned) else {
            skipped += 1;
            invalid_identities += 1;
            continue;
        };
        if !ids.insert(id.clone()) {
            return Err("Medal database contains invalid or duplicate local content IDs.".into());
        }
        let raw: Option<String> = row.get(7).map_err(|_| "Invalid Medal metadata")?;
        let Some(raw) = raw.filter(|s| s.len() <= MAX_RECORD) else {
            skipped += 1;
            continue;
        };
        let m: Value = serde_json::from_str(&raw).map_err(|_| "Invalid Medal metadata")?;
        if !m.is_object() {
            skipped += 1;
            continue;
        }
        let remote = &m["remoteContent"];
        let local_title = parse::text(&m["title"], 512);
        let content_title = parse::text(&remote["contentTitle"], 512);
        let has_title = remote["hasTitle"].as_bool();
        let title = local_title.clone().or_else(|| {
            if has_title == Some(true) {
                content_title.clone()
            } else {
                None
            }
        });
        let title_field = if local_title.is_some() {
            Some("metadata.title".into())
        } else if title.is_some() {
            Some("remoteContent.contentTitle".into())
        } else {
            None
        };
        let category: Option<String> = row.get(5).map_err(|_| "Invalid category ID")?;
        let game_id = category.filter(|s| !s.trim().is_empty() && s.len() <= 128);
        let direct_id = parse::text(&remote["category"]["categoryId"], 128)
            .or_else(|| parse::text(&remote["categoryId"], 128));
        let game = if game_id.is_some() && game_id == direct_id {
            parse::text(&remote["category"]["categoryName"], 512)
        } else {
            None
        };
        if let (Some(id), Some(name)) = (&game_id, &game) {
            names.entry(id.clone()).or_default().insert(name.clone());
        }
        let tags = m["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .take(256)
            .filter_map(|v| parse::text(v, 256))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut extra = BTreeMap::new();
        for (name, value) in [
            (
                "trim",
                parse::select(
                    &m,
                    &[
                        "wasTrimmed",
                        "trimStart",
                        "trimEnd",
                        "trimDuration",
                        "sourceVideoDuration",
                        "trimStartTime",
                    ],
                ),
            ),
            (
                "providerTimes",
                parse::select(&m, &["remoteSyncedAt", "migratedAt", "thumbnailUploadedAt"]),
            ),
        ] {
            if value.as_object().is_some_and(|o| !o.is_empty()) {
                extra.insert(name.into(), value);
            }
        }
        if let Some(bookmarks) = m["bookmarks"].as_array() {
            extra.insert(
                "bookmarks".into(),
                Value::Array(
                    bookmarks
                        .iter()
                        .take(256)
                        .map(|b| parse::select(b, &["time", "name", "timestamp", "priority"]))
                        .filter(|b| b.as_object().is_some_and(|b| !b.is_empty()))
                        .collect(),
                ),
            );
        }
        // created_at is Unix seconds. SQLite INTEGER affinity also permits REAL
        // fractional seconds; do not coerce strings/blobs or infer units by magnitude.
        let created = numeric(
            row.get_ref(4)
                .map_err(|_| "Unreadable local creation field")?,
        );
        let created_ms = parse::epoch(&created, 1000);
        let remote_kind: Option<String> =
            row.get(9).map_err(|_| "Unreadable remote creation field")?;
        let remote_created = if matches!(remote_kind.as_deref(), Some("integer" | "real")) {
            numeric(
                row.get_ref(8)
                    .map_err(|_| "Unreadable remote creation field")?,
            )
        } else {
            Value::Null
        };
        let content_created_ms = parse::epoch(&remote_created, 1);
        let mut timestamp_warnings = Vec::new();
        if created_ms.is_none() {
            timestamp_warnings.push("Local creation time unavailable: expected a positive numeric Unix-second timestamp within the supported date range.".into());
        } else {
            // Preserve source precision alongside the normalized millisecond value.
            extra
                .entry("providerTimes".into())
                .or_insert_with(|| json!({}))["localCreatedUnixSeconds"] = created;
        }
        if remote_kind.as_deref().is_some_and(|kind| kind != "null") && content_created_ms.is_none()
        {
            timestamp_warnings.push("Remote content creation time unavailable: expected a positive numeric Unix-millisecond timestamp within the supported date range.".into());
        }
        let data = Metadata {
            source_format: "sqlite".into(),
            timestamp_warnings,
            local_id: Some(id.clone()),
            remote_id: row
                .get::<_, Option<String>>(1)
                .map_err(|_| "Invalid remote content ID")?
                .filter(|v| !v.is_empty() && v.len() <= 256),
            title,
            title_field,
            title_difference: local_title.is_some()
                && content_title.is_some()
                && local_title != content_title,
            local_title,
            content_title,
            content_has_title: has_title,
            remote_trim_start: remote["trimStartTime"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0),
            game_field: game
                .as_ref()
                .map(|_| "remoteContent.category.categoryName".into()),
            game,
            game_id,
            description: parse::text(&remote["contentDescription"], 4096),
            created_ms,
            content_created_ms,
            duration: m["clipDuration"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0),
            tags,
            file_path: row
                .get::<_, Option<String>>(2)
                .map_err(|_| "Invalid video reference")?
                .filter(|v| v.len() <= 32768),
            image: row
                .get::<_, Option<String>>(3)
                .map_err(|_| "Invalid thumbnail reference")?
                .filter(|v| v.len() <= 32768),
            extra,
            ..Default::default()
        };
        normalized_bytes += serde_json::to_vec(&data)
            .map_err(|_| "Could not normalize Medal data")?
            .len();
        if normalized_bytes > MAX_BYTES as usize {
            return Err("Normalized Medal metadata exceeds 64 MiB.".into());
        }
        entries.push(Entry {
            id,
            data,
            revision: String::new(),
        });
    }
    for entry in &mut entries {
        if entry.data.game.is_none() {
            if let Some(found) = entry.data.game_id.as_ref().and_then(|id| names.get(id)) {
                if found.len() == 1 {
                    entry.data.game = found.first().cloned();
                    entry.data.game_field = Some("Unique SQLite category-ID lookup".into());
                } else {
                    entry.data.game_lookup_ambiguous = true;
                }
            }
        }
    }
    if entries.is_empty() && skipped > 0 {
        return Err("No supported Medal metadata records could be read.".into());
    }
    Ok(Catalog {
        entries,
        skipped,
        invalid_identities,
    })
}
