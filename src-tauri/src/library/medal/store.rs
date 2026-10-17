use super::{parse::Entry, paths};
use crate::library::{db::Db, files};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub index: usize,
    pub clip_id: String,
    pub relation: String,
    pub title: Option<String>,
    pub game: Option<String>,
    pub path: Option<String>,
    pub status: String,
    pub conflict: bool,
    pub warning: bool,
    pub identity_conflict: bool,
    pub differences: Vec<super::selection::Difference>,
    pub timestamp_warnings: Vec<String>,
}
#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub matched: usize,
    pub unchanged: usize,
    pub updated: usize,
    pub new_files: usize,
    pub missing: usize,
    pub ambiguous: usize,
    pub unmatched: usize,
    pub conflicts: usize,
    pub warnings: usize,
    pub skipped: usize,
    pub recovered: usize,
    pub identity_conflicts: usize,
    pub metadata_differences: usize,
    pub timestamp_warnings: usize,
    pub invalid_identities: usize,
}
#[derive(Clone)]
pub struct Reference {
    pub row: Row,
    pub file_id: Option<i64>,
    pub file: Option<files::File>,
    pub thumbnail: Option<(String, String)>,
}
pub struct Prepared {
    pub entry: Entry,
    pub references: Vec<Reference>,
}
pub struct Index {
    paths: HashMap<String, HashSet<i64>>,
    identities: HashMap<String, i64>,
    fingerprints: HashMap<i64, Option<String>>,
    previous: HashMap<String, String>,
    previous_dates: HashMap<String, [Option<i64>; 3]>,
    other_titles: HashMap<i64, HashSet<String>>,
    other_games: HashMap<i64, HashSet<String>>,
    historical: Vec<(i64, String, Option<String>, Option<String>)>,
    previous_images: HashMap<(String, String), (String, String)>,
    other_claims: HashMap<i64, Vec<super::parse::Metadata>>,
    associated: HashSet<i64>,
}
impl Db {
    pub fn cached_medal_catalog(
        &self,
        source: &str,
        fingerprint: &str,
        source_bytes: u64,
    ) -> Result<Option<super::parse::Catalog>, String> {
        let previous:Option<(i64,String)>=self.connection.query_row("SELECT i.source_id,i.revision FROM provider_imports i JOIN sources s ON s.id=i.source_id WHERE s.kind='medal' AND s.external_key=?1",[source],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        let Some((source_id, revision)) = previous else {
            return Ok(None);
        };
        let Ok((version, file, skipped, ids)) =
            serde_json::from_str::<(String, String, usize, Vec<String>)>(&revision)
        else {
            return Ok(None);
        };
        if version != super::parse::NORMALIZATION_VERSION
            || file != fingerprint
            || ids.len() > 100_000
        {
            return Ok(None);
        }
        // The snapshot ID list excludes historical records absent from the current catalog.
        let selected = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
        let stored_bytes:i64=self.connection.query_row("SELECT coalesce(sum(length(e.metadata_json)),0) FROM provider_entries e WHERE e.source_id=?1 AND e.external_id IN (SELECT value FROM json_each(?2))",params![source_id,&selected],|r|r.get(0)).map_err(|e|e.to_string())?;
        // Small catalogs can be cheaper to parse than their expanded normalized representation.
        if stored_bytes as u64 >= source_bytes {
            return Ok(None);
        }
        let mut statement=self.connection.prepare("SELECT e.external_id,e.metadata_json FROM provider_entries e WHERE e.source_id=?1 AND e.external_id IN (SELECT value FROM json_each(?2)) ORDER BY e.external_id").map_err(|e|e.to_string())?;
        let rows = statement
            .query_map(params![source_id, selected], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        for (id, json) in rows {
            let Ok(data) = serde_json::from_str(&json) else {
                return Ok(None);
            };
            entries.push(Entry {
                id,
                data,
                revision: String::new(),
            });
        }
        if entries.len() != ids.len() {
            return Ok(None);
        }
        Ok(Some(super::parse::Catalog {
            entries,
            skipped,
            invalid_identities: 0,
        }))
    }
    pub fn medal_index(&self, source: &str) -> Result<Index, String> {
        let mut index = Index {
            paths: HashMap::new(),
            identities: HashMap::new(),
            fingerprints: HashMap::new(),
            previous: HashMap::new(),
            previous_dates: HashMap::new(),
            other_titles: HashMap::new(),
            other_games: HashMap::new(),
            historical: Vec::new(),
            previous_images: HashMap::new(),
            other_claims: HashMap::new(),
            associated: HashSet::new(),
        };
        let mut s=self.connection.prepare("SELECT l.path,f.id,f.identity FROM locations l JOIN recordings f ON f.id=l.file_id").map_err(|e|e.to_string())?;
        for row in s
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
        {
            let (path, id, identity) = row.map_err(|e| e.to_string())?;
            if let Some(key) = paths::key(&path) {
                index.paths.entry(key).or_default().insert(id);
            }
            if let Some(identity) = &identity {
                index.identities.insert(identity.clone(), id);
            }
            index.fingerprints.insert(id, identity);
        }
        let mut s=self.connection.prepare("SELECT e.external_id,e.revision,e.created_ms,json_extract(e.metadata_json,'$.contentCreatedMs'),json_extract(e.metadata_json,'$.lastEditMs') FROM provider_entries e JOIN sources s ON s.id=e.source_id WHERE s.kind='medal' AND s.external_key=?1").map_err(|e|e.to_string())?;
        for row in s
            .query_map([source], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    [
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                    ],
                ))
            })
            .map_err(|e| e.to_string())?
        {
            let (id, revision, dates) = row.map_err(|e| e.to_string())?;
            index.previous.insert(id.clone(), revision);
            index.previous_dates.insert(
                id,
                dates.map(|v| v.filter(|n| *n > 0 && *n < super::parse::MAX_TIMESTAMP_MS)),
            );
        }
        let mut s=self.connection.prepare("SELECT l.file_id,e.title,e.game,e.metadata_json FROM provider_links l JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id JOIN sources s ON s.id=e.source_id WHERE s.kind='medal' AND s.external_key<>?1 AND l.relation='primary'").map_err(|e|e.to_string())?;
        for row in s
            .query_map([source], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
        {
            let (id, title, game, json) = row.map_err(|e| e.to_string())?;
            if let Ok(data) = serde_json::from_str(&json) {
                index.other_claims.entry(id).or_default().push(data);
            }
            if let Some(title) = title {
                index.other_titles.entry(id).or_default().insert(title);
            }
            if let Some(game) = game {
                index.other_games.entry(id).or_default().insert(game);
            }
        }
        let mut associated=self.connection.prepare("SELECT DISTINCT l.file_id FROM provider_links l JOIN sources s ON s.id=l.source_id WHERE s.kind='medal' AND l.relation='primary'").map_err(|e|e.to_string())?;
        index.associated = associated
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let mut statement=self.connection.prepare("SELECT l.file_id,e.external_id,e.title,e.game FROM provider_links l JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id JOIN sources s ON s.id=e.source_id WHERE s.kind='medal' AND s.external_key=?1 AND l.relation='primary'").map_err(|e|e.to_string())?;
        index.historical = statement
            .query_map([source], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut statement=self.connection.prepare("SELECT p.external_id,p.relation,p.path,p.revision FROM provider_previews p JOIN sources s ON s.id=p.source_id WHERE s.kind='medal' AND s.external_key=?1").map_err(|e|e.to_string())?;
        for row in statement
            .query_map([source], |r| {
                Ok(((r.get(0)?, r.get(1)?), (r.get(2)?, r.get(3)?)))
            })
            .map_err(|e| e.to_string())?
        {
            let (key, value) = row.map_err(|e| e.to_string())?;
            index.previous_images.insert(key, value);
        }
        Ok(index)
    }
}
pub fn prepare(
    entries: Vec<Entry>,
    index: Index,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Result<(Vec<Prepared>, Counts), String> {
    let incoming: HashSet<_> = entries.iter().map(|entry| entry.id.clone()).collect();
    let complementary = entries.iter().any(|e| e.data.source_format == "sqlite")
        || index
            .other_claims
            .values()
            .flatten()
            .any(|m| m.source_format == "sqlite");
    let mut recovered = HashSet::new();
    let mut prepared = Vec::new();
    let mut counts = Counts::default();
    let mut next = 0;
    let mut files: HashMap<String, Option<files::File>> = HashMap::new();
    let mut images: HashMap<String, Option<(String, String)>> = HashMap::new();
    for mut entry in entries {
        if cancel.load(Ordering::Acquire) {
            return Err("Import cancelled".into());
        }
        if !entry.data.timestamp_warnings.is_empty() {
            counts.timestamp_warnings += 1;
        }
        if let Some(previous) = index.previous_dates.get(&entry.id) {
            for (field, incoming, prior) in [
                ("createdMs", &mut entry.data.created_ms, previous[0]),
                (
                    "contentCreatedMs",
                    &mut entry.data.content_created_ms,
                    previous[1],
                ),
                ("lastEditMs", &mut entry.data.last_edit_ms, previous[2]),
            ] {
                if incoming.is_none() && prior.is_some() {
                    *incoming = prior;
                    if !entry.data.retained_timestamps.iter().any(|v| v == field) {
                        entry.data.retained_timestamps.push(field.into());
                    }
                }
            }
        }
        let mut references = Vec::new();
        let mut image_reference = |relation: &str, path: Option<&str>| {
            path.and_then(|path| {
                let key = paths::key(path).unwrap_or_else(|| path.to_owned());
                images
                    .entry(key)
                    .or_insert_with(|| paths::image(path))
                    .clone()
                    .or_else(|| {
                        index
                            .previous_images
                            .get(&(entry.id.clone(), relation.to_owned()))
                            .filter(|(old, _)| paths::key(old) == paths::key(path))
                            .cloned()
                    })
            })
        };
        let primary_image = image_reference("primary", entry.data.image.as_deref());
        let original_image = image_reference("original", entry.data.original_image.as_deref());
        entry.revision = serde_json::to_string(&(&entry.data, &primary_image, &original_image))
            .map_err(|_| "Could not normalize image revisions")?;
        for (relation, path, _image) in [
            ("primary", &entry.data.file_path, &entry.data.image),
            (
                "original",
                &entry.data.original_file_path,
                &entry.data.original_image,
            ),
        ] {
            if relation == "original" && path.is_none() {
                continue;
            }
            let file = path.as_deref().and_then(|p| {
                let key = paths::key(p).unwrap_or_else(|| p.to_owned());
                files
                    .entry(key)
                    .or_insert_with(|| paths::video(p).ok())
                    .clone()
            });
            let mut candidates = path
                .as_deref()
                .and_then(paths::key)
                .and_then(|key| index.paths.get(&key).cloned())
                .unwrap_or_default();
            if let Some(identity) = file.as_ref().and_then(|f| f.identity.as_ref()) {
                if let Some(id) = index.identities.get(identity) {
                    candidates.insert(*id);
                }
            }
            let mut conflict = relation == "primary" && entry.data.game_conflict;
            let mut identity_conflict = candidates.len() > 1;
            let warning = relation == "primary" && entry.data.game_lookup_ambiguous;
            let mut file_id = if candidates.len() == 1 {
                candidates.iter().next().copied()
            } else {
                None
            };
            if let (Some(id), Some(file)) = (file_id, file.as_ref()) {
                if index.fingerprints.get(&id) != Some(&file.identity) {
                    file_id = None;
                    conflict = true;
                    identity_conflict = true;
                }
            }
            let status = if candidates.len() > 1 {
                "ambiguous"
            } else if conflict && file_id.is_none() && !candidates.is_empty() {
                "ambiguous"
            } else if file_id.is_some() {
                if index.previous.get(&entry.id) == Some(&entry.revision) {
                    "unchanged"
                } else if index.previous.contains_key(&entry.id) {
                    "updated"
                } else {
                    "matched"
                }
            } else if file.is_some() {
                "new"
            } else if path.is_some() {
                "missing"
            } else {
                "unmatched"
            };
            match status {
                "matched" => counts.matched += 1,
                "unchanged" => counts.unchanged += 1,
                "updated" => counts.updated += 1,
                "new" => counts.new_files += 1,
                "missing" => counts.missing += 1,
                "ambiguous" => counts.ambiguous += 1,
                _ => counts.unmatched += 1,
            }
            if conflict {
                counts.conflicts += 1;
            }
            if warning {
                counts.warnings += 1;
            }
            let thumbnail = if relation == "primary" {
                primary_image.clone()
            } else {
                original_image.clone()
            };
            let differences = if relation == "primary" {
                file_id
                    .and_then(|id| index.other_claims.get(&id))
                    .into_iter()
                    .flatten()
                    .flat_map(|prior| super::selection::differences(prior, &entry.data))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if !differences.is_empty() {
                counts.metadata_differences += 1;
            }
            if identity_conflict {
                counts.identity_conflicts += 1;
            }
            if relation == "primary" {
                if let Some(id) = file_id.filter(|id| !index.associated.contains(id)) {
                    recovered.insert(id);
                }
            }
            references.push(Reference {
                row: Row {
                    index: next,
                    clip_id: entry.id.clone(),
                    relation: relation.into(),
                    title: if relation == "primary" {
                        entry.data.title.clone()
                    } else {
                        None
                    },
                    game: entry.data.game.clone(),
                    path: path.clone(),
                    status: status.into(),
                    conflict,
                    warning,
                    identity_conflict,
                    differences,
                    timestamp_warnings: if relation == "primary" {
                        entry.data.timestamp_warnings.clone()
                    } else {
                        Vec::new()
                    },
                },
                file_id,
                file,
                thumbnail,
            });
            next += 1;
        }
        prepared.push(Prepared { entry, references });
        done.fetch_add(1, Ordering::Release);
    }
    let mut titles: HashMap<String, HashSet<String>> = HashMap::new();
    let mut games: HashMap<String, HashSet<String>> = HashMap::new();
    let group = |r: &Reference| {
        r.file_id.map(|id| format!("id:{id}")).or_else(|| {
            r.file.as_ref().map(|f| {
                format!(
                    "new:{}",
                    f.identity.clone().unwrap_or_else(|| f.path.clone())
                )
            })
        })
    };
    for (id, values) in index.other_titles.into_iter().filter(|_| !complementary) {
        titles.insert(format!("id:{id}"), values);
    }
    for (id, values) in index.other_games.into_iter().filter(|_| !complementary) {
        games.insert(format!("id:{id}"), values);
    }
    for (id, external_id, title, game) in index.historical {
        if !incoming.contains(&external_id) {
            if let Some(title) = title {
                titles.entry(format!("id:{id}")).or_default().insert(title);
            }
            if let Some(game) = game {
                games.entry(format!("id:{id}")).or_default().insert(game);
            }
        }
    }
    for p in &prepared {
        for r in &p.references {
            if r.row.relation == "primary" {
                if let (Some(key), Some(game)) = (group(r), &r.row.game) {
                    games.entry(key).or_default().insert(game.clone());
                }
                if let (Some(key), Some(title)) = (group(r), &r.row.title) {
                    titles.entry(key).or_default().insert(title.clone());
                }
            }
        }
    }
    for p in &mut prepared {
        for r in &mut p.references {
            // A related edit's metadata is not applied to its original file.
            if r.row.relation != "primary" {
                continue;
            }
            let title_conflict = group(r)
                .and_then(|key| titles.get(&key))
                .is_some_and(|v| v.len() > 1);
            let game_conflict = group(r)
                .and_then(|key| games.get(&key))
                .is_some_and(|v| v.len() > 1);
            if (title_conflict || game_conflict) && !r.row.conflict {
                r.row.conflict = true;
                counts.conflicts += 1;
            }
        }
    }
    counts.recovered = recovered.len();
    Ok((prepared, counts))
}

fn register(tx: &rusqlite::Transaction<'_>, file: &files::File) -> Result<i64, String> {
    let id: Option<i64> = if let Some(identity) = &file.identity {
        tx.query_row(
            "SELECT id FROM recordings WHERE identity=?1",
            [identity],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
    } else {
        tx.query_row("SELECT f.id FROM locations l JOIN recordings f ON f.id=l.file_id WHERE l.path=?1 AND f.identity IS NULL",[&file.path],|r|r.get(0)).optional().map_err(|e|e.to_string())?
    };
    let id = if let Some(id) = id {
        id
    } else {
        tx.execute(
            "INSERT INTO recordings(identity,size,modified_ms,modified_ns) VALUES(?1,?2,?3,?4)",
            params![file.identity, file.size, file.modified_ms, file.modified_ns],
        )
        .map_err(|e| e.to_string())?;
        tx.last_insert_rowid()
    };
    tx.execute("INSERT INTO locations(file_id,path,name) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET file_id=excluded.file_id,name=excluded.name",params![id,file.path,file.name]).map_err(|e|e.to_string())?;
    Ok(id)
}

fn verify(file: &files::File, checked: &mut HashMap<String, files::File>) -> Result<(), String> {
    let verified = if let Some(verified) = checked.get(&file.path) {
        verified.clone()
    } else {
        let verified = paths::video(&file.path)?;
        checked.insert(file.path.clone(), verified.clone());
        verified
    };
    if verified != *file {
        return Err("A recording changed after preview. Prepare the import again.".into());
    }
    Ok(())
}

pub struct Commit<'a> {
    pub source: &'a str,
    pub path: &'a str,
    pub revision: &'a str,
    pub prepared: &'a [Prepared],
    pub additions: &'a HashSet<usize>,
    pub cancel: &'a AtomicBool,
    pub done: &'a AtomicUsize,
}
impl Db {
    pub fn commit_medal(&mut self, commit: Commit<'_>) -> Result<usize, String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO sources(kind,external_key) VALUES('medal',?1) ON CONFLICT DO NOTHING",
            [commit.source],
        )
        .map_err(|e| e.to_string())?;
        let source: i64 = tx
            .query_row(
                "SELECT id FROM sources WHERE kind='medal' AND external_key=?1",
                [commit.source],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let now = crate::library::now_ms();
        let mut checked_files = HashMap::new();
        let mut checked_images: HashMap<String, Option<(String, String)>> = HashMap::new();
        let mut linked = 0;
        let mut affected = HashSet::new();
        for prepared in commit.prepared {
            if commit.cancel.load(Ordering::Acquire) {
                return Err("Import cancelled; no changes were saved.".into());
            }
            let entry = &prepared.entry;
            let metadata_json =
                serde_json::to_string(&entry.data).map_err(|_| "Could not serialize metadata")?;
            let changed=tx.execute("INSERT INTO provider_entries(source_id,external_id,revision,title,game,created_ms,metadata_json,imported_ms) VALUES(?1,?2,?3,?4,?5,?6,?8,?7) ON CONFLICT(source_id,external_id) DO UPDATE SET revision=excluded.revision,title=excluded.title,game=excluded.game,created_ms=excluded.created_ms,metadata_json=excluded.metadata_json,imported_ms=excluded.imported_ms WHERE provider_entries.revision<>excluded.revision",params![source,entry.id,entry.revision,entry.data.title,entry.data.game,entry.data.created_ms,now,metadata_json]).map_err(|e|e.to_string())?;
            // Only this provider record's associations are refreshed; historical files/media survive.
            if changed > 0
                || prepared
                    .references
                    .iter()
                    .any(|r| r.row.status != "unchanged")
            {
                tx.execute(
                    "DELETE FROM provider_links WHERE source_id=?1 AND external_id=?2",
                    params![source, entry.id],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "DELETE FROM provider_previews WHERE source_id=?1 AND external_id=?2",
                    params![source, entry.id],
                )
                .map_err(|e| e.to_string())?;
            }
            for reference in &prepared.references {
                let id = if let Some(id) = reference.file_id {
                    let expected: Option<String> = tx
                        .query_row("SELECT identity FROM recordings WHERE id=?1", [id], |r| {
                            r.get(0)
                        })
                        .map_err(|_| "Library changed; prepare this import again")?;
                    if let Some(before) = &reference.file {
                        verify(before, &mut checked_files)?;
                        if before.identity != expected {
                            return Err(
                                "A recording changed after preview. Prepare the import again."
                                    .into(),
                            );
                        }
                    }
                    Some(id)
                } else if reference.row.status == "new"
                    && commit.additions.contains(&reference.row.index)
                {
                    let before = reference
                        .file
                        .as_ref()
                        .ok_or("New recording is unavailable")?;
                    verify(before, &mut checked_files)?;
                    Some(register(&tx, before)?)
                } else {
                    None
                };
                if let Some(id) = id {
                    affected.insert(id);
                    tx.execute(
                        "INSERT INTO provider_links(source_id,external_id,file_id,relation,file_size,file_modified_ns) SELECT ?1,?2,id,?4,coalesce(?5,size),CASE WHEN ?5 IS NULL THEN modified_ns ELSE ?6 END FROM recordings WHERE id=?3 ON CONFLICT(source_id,external_id,file_id,relation) DO UPDATE SET file_size=excluded.file_size,file_modified_ns=excluded.file_modified_ns WHERE provider_links.file_size IS NOT excluded.file_size OR provider_links.file_modified_ns IS NOT excluded.file_modified_ns",
                        params![source, entry.id, id, reference.row.relation, reference.file.as_ref().map(|f|f.size),reference.file.as_ref().and_then(|f|f.modified_ns.as_ref())],
                    )
                    .map_err(|e| e.to_string())?;
                    // Existing matches keep their known locations even while temporarily absent.
                    if let Some(file) = &reference.file {
                        tx.execute("UPDATE recordings SET status=CASE WHEN size<>?2 OR modified_ns IS NOT ?3 OR status='error' THEN 'pending' ELSE status END,error=CASE WHEN size<>?2 OR modified_ns IS NOT ?3 OR status='error' THEN NULL ELSE error END,size=?2,modified_ns=?3,modified_ms=?4 WHERE id=?1 AND (size<>?2 OR modified_ns IS NOT ?3 OR modified_ms IS NOT ?4 OR status='error')",params![id,file.size,file.modified_ns,file.modified_ms]).map_err(|e|e.to_string())?;
                        tx.execute("INSERT INTO locations(file_id,path,name) VALUES(?1,?2,?3) ON CONFLICT(path) DO NOTHING",params![id,file.path,file.name]).map_err(|e|e.to_string())?;
                        tx.execute("INSERT INTO provider_access(source_id,location_id) SELECT ?1,id FROM locations WHERE file_id=?2 AND path=?3 ON CONFLICT(source_id,location_id) DO UPDATE SET availability='available' WHERE provider_access.availability<>'available'",params![source,id,file.path]).map_err(|e|e.to_string())?;
                    } else if let Some(key) = reference.row.path.as_deref().and_then(paths::key) {
                        let mut statement = tx
                            .prepare("SELECT id,path FROM locations WHERE file_id=?1")
                            .map_err(|e| e.to_string())?;
                        let locations = statement
                            .query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                            .map_err(|e| e.to_string())?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|e| e.to_string())?;
                        drop(statement);
                        for (location, path) in locations {
                            if paths::key(&path) == Some(key.clone()) {
                                tx.execute("INSERT INTO provider_access(source_id,location_id,availability) VALUES(?1,?2,'missing') ON CONFLICT(source_id,location_id) DO UPDATE SET availability='missing' WHERE provider_access.availability<>'missing'",params![source,location]).map_err(|e|e.to_string())?;
                            }
                        }
                    }
                    if let Some((path, revision)) = &reference.thumbnail {
                        let current = checked_images
                            .entry(path.clone())
                            .or_insert_with(|| paths::image(path));
                        if current
                            .as_ref()
                            .is_some_and(|current| current != &(path.clone(), revision.clone()))
                        {
                            return Err(
                                "A thumbnail changed after preview. Prepare the import again."
                                    .into(),
                            );
                        }
                        tx.execute(
                            "INSERT INTO provider_previews VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source_id,external_id,relation) DO UPDATE SET path=excluded.path,revision=excluded.revision WHERE provider_previews.path<>excluded.path OR provider_previews.revision<>excluded.revision",
                            params![source, entry.id, reference.row.relation, path, revision],
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    linked += 1;
                }
            }
            commit.done.fetch_add(1, Ordering::Release);
        }
        super::selection::reconcile(&tx, &affected, source)?;
        tx.execute("INSERT INTO provider_imports VALUES(?1,?2,?3,?4) ON CONFLICT(source_id) DO UPDATE SET catalog_path=excluded.catalog_path,revision=excluded.revision,imported_ms=excluded.imported_ms",params![source,commit.path,commit.revision,now]).map_err(|e|e.to_string())?;
        if commit.cancel.load(Ordering::Acquire) {
            return Err("Import cancelled; no changes were saved.".into());
        }
        // Validate each distinct input again after the transaction work, before publishing grants.
        for file in checked_files.values() {
            if commit.cancel.load(Ordering::Acquire) {
                return Err("Import cancelled; no changes were saved.".into());
            }
            if paths::video(&file.path)? != *file {
                return Err("A recording changed during import; no changes were saved.".into());
            }
        }
        for (path, before) in checked_images {
            if commit.cancel.load(Ordering::Acquire) {
                return Err("Import cancelled; no changes were saved.".into());
            }
            if paths::image(&path) != before {
                return Err("A thumbnail changed during import; no changes were saved.".into());
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(linked)
    }
}
