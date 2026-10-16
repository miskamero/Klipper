use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub const MAX_IMAGE: usize = 128 * 1024;
pub const MAX_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_ENTRIES: i64 = 2000;

pub struct Cache {
    db: Connection,
    bytes: i64,
    entries: i64,
}
impl Cache {
    pub fn open(directory: &Path) -> Result<Self, String> {
        Self::with_limits(directory, MAX_BYTES, MAX_ENTRIES)
    }
    pub(super) fn with_limits(directory: &Path, bytes: i64, entries: i64) -> Result<Self, String> {
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let db =
            Connection::open(directory.join("previews-v1.sqlite3")).map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA page_size=4096; PRAGMA max_page_count=20480;
            PRAGMA auto_vacuum=INCREMENTAL; PRAGMA journal_mode=DELETE;
            PRAGMA journal_size_limit=0; PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS images (
                key TEXT PRIMARY KEY, image BLOB NOT NULL, used_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS images_lru ON images(used_at);
            PRAGMA user_version=1;",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { db, bytes, entries })
    }
    pub fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let image: Option<Vec<u8>> = self
            .db
            .query_row("SELECT image FROM images WHERE key=?1", [key], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        if image.is_some() {
            self.db.execute("UPDATE images SET used_at=(SELECT coalesce(max(used_at),0)+1 FROM images) WHERE key=?1",[key]).map_err(|e| e.to_string())?;
        }
        Ok(image)
    }
    pub fn put(&mut self, key: &str, image: &[u8]) -> Result<(), String> {
        if !super::valid_image(image) || image.len() as i64 > self.bytes {
            return Err("Thumbnail exceeded its size limit or was not a JPEG.".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM images WHERE key=?1", [key])
            .map_err(|e| e.to_string())?;
        // Evict before insertion. SQLite reuses freed pages; the file also has an 80 MiB hard ceiling.
        loop {
            let (count, size): (i64, i64) = tx
                .query_row(
                    "SELECT count(*),coalesce(sum(length(image)),0) FROM images",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            if count < self.entries && size + image.len() as i64 <= self.bytes {
                break;
            }
            if count == 0 {
                return Err("Thumbnail cache has no space.".into());
            }
            tx.execute("DELETE FROM images WHERE key=(SELECT key FROM images ORDER BY used_at,key LIMIT 1)",[]).map_err(|e| e.to_string())?;
        }
        tx.execute("INSERT INTO images(key,image,used_at) VALUES(?1,?2,(SELECT coalesce(max(used_at),0)+1 FROM images))",
            params![key,image]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.db
            .execute_batch("PRAGMA incremental_vacuum(32)")
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
