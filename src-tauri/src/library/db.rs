use super::{
    files::File,
    query::{Page, Query},
    Root,
};
use crate::media::MediaInfo;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;
fn error(e: rusqlite::Error) -> String {
    e.to_string()
}

pub struct Db {
    pub(super) connection: Connection,
}

impl Db {
    // Shared read-only authorization for playback and thumbnails. No caller supplies a path.
    pub fn resolve_file(&mut self, id: i64) -> Result<(File, Option<Root>)> {
        let mut last_error = "Recording has no remaining indexed folder.".to_string();
        for (path, root) in self.candidates(id)? {
            let validated = super::files::within(
                Path::new(&path),
                Path::new(&root.path),
                root.identity.as_deref(),
            )
            .and_then(|p| super::files::describe(&p));
            match validated {
                Ok(file) => {
                    let known: Option<String> = self
                        .connection
                        .query_row("SELECT identity FROM recordings WHERE id=?1", [id], |r| {
                            r.get(0)
                        })
                        .map_err(error)?;
                    if known != file.identity {
                        return Err(
                            "This path now refers to a different file. Rescan its folder.".into(),
                        );
                    }
                    return Ok((file, Some(root)));
                }
                Err(message) => {
                    self.unavailable(&path, matches!(Path::new(&path).try_exists(), Ok(false)))?;
                    last_error = message;
                }
            }
        }
        if let Some(file) = self.provider_file(id)? {
            return Ok((file, None));
        }
        Err(format!("Recording could not be opened: {last_error}"))
    }
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path).map_err(error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;",
            )
            .map_err(error)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(error)?;
        if version > 3 {
            return Err("Library database was created by a newer Klipper version".into());
        }
        if version == 0 {
            let tx = connection.transaction().map_err(error)?;
            tx.execute_batch(include_str!("migrations/001.sql"))
                .map_err(error)?;
            tx.commit().map_err(error)?;
        }
        if version < 2 {
            let tx = connection.transaction().map_err(error)?;
            tx.execute_batch(include_str!("migrations/002.sql"))
                .map_err(error)?;
            tx.commit().map_err(error)?;
        }
        if version < 3 {
            let tx = connection.transaction().map_err(error)?;
            tx.execute_batch(include_str!("migrations/003.sql"))
                .map_err(error)?;
            tx.commit().map_err(error)?;
        }
        // Never silently resume scans after an unclean exit.
        connection.execute("UPDATE roots SET scan_state='interrupted',error='Previous scan was interrupted; rescan this folder.' WHERE scan_state IN ('discovering','analyzing')", []).map_err(error)?;
        Ok(Self { connection })
    }

    pub fn roots(&self) -> Result<Vec<Root>> {
        let mut statement = self.connection.prepare("SELECT r.id,r.path,r.identity,r.scan_state,r.error,r.last_scan,
            (SELECT count(DISTINCT l.file_id) FROM root_entries e JOIN locations l ON l.id=e.location_id WHERE e.root_id=r.id)
            FROM roots r ORDER BY r.path COLLATE NOCASE").map_err(error)?;
        let roots = statement
            .query_map([], |r| {
                Ok(Root {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    identity: r.get(2)?,
                    scan_state: r.get(3)?,
                    error: r.get(4)?,
                    last_scan: r.get(5)?,
                    recordings: r.get(6)?,
                })
            })
            .map_err(error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(error);
        roots
    }

    pub fn root(&self, id: i64) -> Result<Root> {
        self.roots()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("Indexed folder no longer exists".into())
    }

    pub fn add_root(&mut self, path: &Path) -> Result<Root> {
        let path_text = path
            .to_str()
            .ok_or("Folder path cannot be represented as text")?;
        let tx = self.connection.transaction().map_err(error)?;
        tx.execute(
            "INSERT INTO sources(kind,external_key) VALUES('folder',?1) ON CONFLICT DO NOTHING",
            [path_text],
        )
        .map_err(error)?;
        let id: i64 = tx
            .query_row(
                "SELECT id FROM sources WHERE kind='folder' AND external_key=?1",
                [path_text],
                |r| r.get(0),
            )
            .map_err(error)?;
        tx.execute(
            "INSERT INTO roots(id,path,identity) VALUES(?1,?2,?3) ON CONFLICT(path) DO NOTHING",
            params![id, path_text, super::files::identity(path)],
        )
        .map_err(error)?;
        tx.commit().map_err(error)?;
        self.root(id)
    }

    pub fn remove_root(&mut self, id: i64) -> Result<()> {
        // Cascades only this source's associations, never physical files or other sources.
        self.connection
            .execute("DELETE FROM sources WHERE id=?1 AND kind='folder'", [id])
            .map_err(error)?;
        Ok(())
    }

    pub fn begin_scan(&mut self, id: i64) -> Result<i64> {
        let changed = self.connection.execute("UPDATE roots SET generation=generation+1,scan_state='discovering',error=NULL WHERE id=?1", [id]).map_err(error)?;
        if changed == 0 {
            return Err("Folder was removed".into());
        }
        self.connection
            .query_row("SELECT generation FROM roots WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(error)
    }

    pub fn finish_discovery(&mut self, id: i64, generation: i64, complete: bool) -> Result<()> {
        let availability = if complete { "missing" } else { "unverified" };
        self.connection
            .execute(
                "UPDATE root_entries SET availability=?3 WHERE root_id=?1 AND generation<>?2",
                params![id, generation, availability],
            )
            .map_err(error)?;
        self.connection
            .execute("UPDATE roots SET scan_state='analyzing' WHERE id=?1", [id])
            .map_err(error)?;
        Ok(())
    }

    pub fn finish_scan(&mut self, id: i64, state: &str, message: Option<&str>) -> Result<()> {
        self.connection
            .execute(
                "UPDATE roots SET scan_state=?2,error=?3,last_scan=?4 WHERE id=?1",
                params![id, state, message, super::now_ms()],
            )
            .map_err(error)?;
        Ok(())
    }

    pub fn discover(&mut self, root: i64, generation: i64, file: &File) -> Result<bool> {
        let tx = self.connection.transaction().map_err(error)?;
        let by_path: Option<(i64, Option<String>)> = tx.query_row(
            "SELECT f.id,f.identity FROM locations l JOIN recordings f ON f.id=l.file_id WHERE l.path=?1",
            [&file.path], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(error)?;
        let by_identity: Option<i64> = if let Some(identity) = &file.identity {
            tx.query_row(
                "SELECT id FROM recordings WHERE identity=?1",
                [identity],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?
        } else {
            None
        };
        // Match a physical identity, or the same path when the filesystem cannot provide identity.
        let id = by_identity.or_else(|| {
            by_path
                .filter(|(_, identity)| *identity == file.identity)
                .map(|(id, _)| id)
        });
        let id = match id {
            Some(id) => {
                tx.execute("UPDATE recordings SET
                    status=CASE WHEN size<>?2 OR modified_ns IS NOT ?3 OR ?3 IS NULL THEN 'pending' ELSE status END,
                    error=CASE WHEN size<>?2 OR modified_ns IS NOT ?3 OR ?3 IS NULL THEN NULL ELSE error END,
                    size=?2,modified_ms=?4,modified_ns=?3 WHERE id=?1",
                    params![id,file.size,file.modified_ns,file.modified_ms]).map_err(error)?;
                id
            }
            None => {
                tx.execute("INSERT INTO recordings(identity,size,modified_ms,modified_ns) VALUES(?1,?2,?3,?4)",
                    params![file.identity,file.size,file.modified_ms,file.modified_ns]).map_err(error)?;
                tx.last_insert_rowid()
            }
        };
        tx.execute(
            "INSERT INTO locations(file_id,path,name) VALUES(?1,?2,?3)
            ON CONFLICT(path) DO UPDATE SET file_id=excluded.file_id,name=excluded.name",
            params![id, file.path, file.name],
        )
        .map_err(error)?;
        tx.execute("INSERT INTO root_entries(root_id,location_id,generation,availability)
            SELECT ?1,id,?2,'available' FROM locations WHERE path=?3
            ON CONFLICT(root_id,location_id) DO UPDATE SET generation=excluded.generation,availability='available'",
            params![root,generation,file.path]).map_err(error)?;
        // A genuine probe failure is retryable on the next explicit rescan.
        tx.execute(
            "UPDATE recordings SET status='pending',error=NULL WHERE id=?1 AND status='error'",
            [id],
        )
        .map_err(error)?;
        let ready = tx
            .query_row(
                "SELECT status='ready' FROM recordings WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(ready)
    }

    pub fn pending(&self, root: i64) -> Result<Option<(i64, PathBuf)>> {
        self.connection.query_row("SELECT f.id,l.path FROM recordings f JOIN locations l ON l.file_id=f.id
            JOIN root_entries e ON e.location_id=l.id WHERE e.root_id=?1 AND e.availability='available'
            AND f.status='pending' ORDER BY f.id LIMIT 1", [root],
            |r| Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?)))).optional().map_err(error)
    }

    pub fn save_media(&mut self, id: i64, file: &File, media: &MediaInfo) -> Result<()> {
        let tx = self.connection.transaction().map_err(error)?;
        // Ignore results if another scan observed a replacement or a changed fingerprint.
        let valid: bool = tx.query_row("SELECT size=?2 AND modified_ns IS ?3 AND identity IS ?4 FROM recordings WHERE id=?1",
            params![id,file.size,file.modified_ns,file.identity], |r| r.get(0)).map_err(error)?;
        if valid {
            let payload = serde_json::to_string(media).map_err(|e| e.to_string())?;
            let value = serde_json::to_value(media).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO media(file_id,duration,video_codec,width,height,frame_rate,audio_count,payload)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(file_id) DO UPDATE SET
                duration=excluded.duration,video_codec=excluded.video_codec,width=excluded.width,height=excluded.height,
                frame_rate=excluded.frame_rate,audio_count=excluded.audio_count,payload=excluded.payload",
                params![id,value["duration"].as_f64(),value["videoCodec"].as_str(),value["width"].as_i64(),
                    value["height"].as_i64(),value["frameRate"].as_f64(),value["audio"].as_array().map_or(0, |a| a.len()) as i64,payload]).map_err(error)?;
            tx.execute(
                "UPDATE recordings SET status='ready',error=NULL WHERE id=?1",
                [id],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        Ok(())
    }

    pub fn fail_media(&mut self, id: i64, message: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE recordings SET status='error',error=?2 WHERE id=?1",
                params![id, message],
            )
            .map_err(error)?;
        Ok(())
    }

    pub fn candidates(&self, id: i64) -> Result<Vec<(String, Root)>> {
        let mut statement = self.connection.prepare("SELECT DISTINCT l.path,e.root_id FROM locations l
            JOIN root_entries e ON e.location_id=l.id WHERE l.file_id=?1 ORDER BY e.availability='available' DESC").map_err(error)?;
        let paths = statement
            .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(error)?;
        paths
            .into_iter()
            .map(|(path, root)| Ok((path, self.root(root)?)))
            .collect()
    }

    pub fn cached(&self, id: i64, file: &File) -> Result<Option<MediaInfo>> {
        let payload: Option<String> = self.connection.query_row("SELECT m.payload FROM media m JOIN recordings f ON f.id=m.file_id
            WHERE f.id=?1 AND f.status='ready' AND f.size=?2 AND f.modified_ns IS ?3 AND f.identity IS ?4",
            params![id,file.size,file.modified_ns,file.identity], |r| r.get(0)).optional().map_err(error)?;
        payload
            .map(|p| serde_json::from_str(&p).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn unavailable(&mut self, path: &str, missing: bool) -> Result<()> {
        self.connection.execute("UPDATE provider_access SET availability=?2 WHERE location_id=(SELECT id FROM locations WHERE path=?1)",
            params![path,if missing { "missing" } else { "inaccessible" }]).map_err(error)?;
        self.connection.execute("UPDATE root_entries SET availability=?2 WHERE location_id=(SELECT id FROM locations WHERE path=?1)",
            params![path,if missing { "missing" } else { "inaccessible" }]).map_err(error)?;
        Ok(())
    }

    pub fn query(&self, query: Query) -> Result<Page> {
        super::query::query(self, query)
    }
}
