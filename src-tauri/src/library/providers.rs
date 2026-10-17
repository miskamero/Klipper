use super::db::Db;
use super::medal::{parse::Metadata, paths};
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub provider: String,
    pub external_id: String,
    pub relation: String,
    pub imported_ms: i64,
    pub source_path: Option<String>,
    pub data: Metadata,
    pub selected_values: std::collections::BTreeMap<String, serde_json::Value>,
}
impl Db {
    pub fn provider_metadata(&self, id: i64) -> Result<Vec<Imported>, String> {
        let mut statement=self.connection.prepare("SELECT s.kind,e.external_id,l.relation,e.imported_ms,e.metadata_json,i.catalog_path,e.source_id FROM provider_links l JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id JOIN sources s ON s.id=e.source_id LEFT JOIN provider_imports i ON i.source_id=e.source_id WHERE l.file_id=?1 ORDER BY l.relation,e.imported_ms DESC LIMIT 100").map_err(|e|e.to_string())?;
        let rows = statement
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(
                |(provider, external_id, relation, imported_ms, json, source_path, source_id)| {
                    let mut choices=self.connection.prepare("SELECT field,value_json FROM provider_choices WHERE file_id=?1 AND source_id=?2 AND external_id=?3 AND field<>'preview'").map_err(|e|e.to_string())?;
                    let selected=choices.query_map(rusqlite::params![id,source_id,external_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                    Ok(Imported {
                        provider,
                        external_id,
                        relation,
                        imported_ms,
                        source_path,
                        data: serde_json::from_str(&json)
                            .map_err(|_| "Stored provider metadata is invalid")?,
                        selected_values:selected.into_iter().map(|(k,v)|Ok((k,serde_json::from_str(&v).map_err(|_|"Stored field selection is invalid")?))).collect::<Result<_,String>>()?,
                    })
                },
            )
            .collect()
    }
    pub fn provider_file(&mut self, id: i64) -> Result<Option<super::files::File>, String> {
        let expected: Option<String> = self
            .connection
            .query_row("SELECT identity FROM recordings WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        let mut statement=self.connection.prepare("SELECT l.path FROM locations l JOIN provider_access a ON a.location_id=l.id WHERE l.file_id=?1").map_err(|e|e.to_string())?;
        let paths = statement
            .query_map([id], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(statement);
        for path in paths {
            if let Ok(file) = paths::video(&path) {
                if file.identity == expected {
                    self.connection.execute("UPDATE provider_access SET availability='available' WHERE location_id IN (SELECT id FROM locations WHERE file_id=?1 AND path=?2)",rusqlite::params![id,path]).map_err(|e|e.to_string())?;
                    return Ok(Some(file));
                }
            }
            self.unavailable(
                &path,
                matches!(std::path::Path::new(&path).try_exists(), Ok(false)),
            )?;
        }
        Ok(None)
    }
    pub fn provider_preview(
        &self,
        id: i64,
        file: &super::files::File,
    ) -> Result<Option<(String, String)>, String> {
        let selected:Option<String>=self.connection.query_row("SELECT c.value_json FROM provider_choices c WHERE c.file_id=?1 AND c.field='preview'",[id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        if let Some(selected) = selected {
            let (path, revision, size, modified): (String, String, i64, Option<String>) =
                serde_json::from_str(&selected)
                    .map_err(|_| "Invalid selected thumbnail provenance")?;
            return Ok(
                (size == file.size && modified == file.modified_ns).then_some((path, revision))
            );
        }
        // Conflicting preview references are not silently assigned to a recording.
        self.connection.query_row("SELECT min(p.path),min(p.revision) FROM provider_previews p JOIN provider_links l ON l.source_id=p.source_id AND l.external_id=p.external_id AND l.relation=p.relation WHERE l.file_id=?1 AND l.file_size=?2 AND l.file_modified_ns IS ?3 HAVING count(DISTINCT p.path)=1 AND count(DISTINCT p.revision)=1",rusqlite::params![id,file.size,file.modified_ns],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())
    }
}
