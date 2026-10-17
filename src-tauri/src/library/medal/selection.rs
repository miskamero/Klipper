use super::parse::Metadata;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    pub field: String,
    pub existing: Value,
    pub incoming: Value,
    pub existing_format: String,
}
pub fn claims(data: &Metadata) -> BTreeMap<&'static str, Value> {
    let mut values = BTreeMap::from([
        ("title", json!(data.title)),
        ("game", json!(data.game)),
        ("description", json!(data.description)),
        ("localCreatedMs", json!(data.created_ms)),
        ("remoteCreatedMs", json!(data.content_created_ms)),
        ("hasTitle", json!(data.content_has_title)),
        ("remoteTrimStart", json!(data.remote_trim_start)),
        ("tags", json!(data.tags)),
    ]);
    values.retain(|_, v| {
        !v.is_null()
            && v.as_str().is_none_or(|s| !s.trim().is_empty())
            && v.as_array().is_none_or(|a| !a.is_empty())
    });
    values
}
pub fn differences(existing: &Metadata, incoming: &Metadata) -> Vec<Difference> {
    let prior = claims(existing);
    let source_format = if existing.source_format.is_empty() {
        "json"
    } else {
        &existing.source_format
    };
    claims(incoming)
        .into_iter()
        .filter_map(|(field, incoming)| {
            let existing = prior.get(field)?;
            (existing != &incoming).then(|| Difference {
                field: field.into(),
                existing: existing.clone(),
                incoming,
                existing_format: source_format.into(),
            })
        })
        .collect()
}

struct Claim {
    source: i64,
    external: String,
    value: String,
    revision: String,
}

// Only a selected owner's unambiguous update may change an existing nonempty selection.
// A second source never wins because it happened to finish later.
pub fn reconcile(
    tx: &Transaction<'_>,
    affected: &HashSet<i64>,
    importing: i64,
) -> Result<(), String> {
    for &file in affected {
        for field in ["title", "game", "created_ms", "preview"] {
            let mut statement=tx.prepare(if field=="preview" {
                "SELECT p.source_id,p.external_id,json_array(p.path,p.revision,l.file_size,l.file_modified_ns),e.revision FROM provider_previews p JOIN provider_links l ON l.source_id=p.source_id AND l.external_id=p.external_id AND l.relation=p.relation JOIN recordings f ON f.id=l.file_id JOIN provider_entries e ON e.source_id=p.source_id AND e.external_id=p.external_id WHERE l.file_id=?1 AND l.file_size=f.size AND l.file_modified_ns IS f.modified_ns ORDER BY p.source_id,p.external_id"
            } else {
                match field {
                    "title"=>"SELECT e.source_id,e.external_id,json_quote(e.title),e.revision FROM provider_entries e JOIN provider_links l ON l.source_id=e.source_id AND l.external_id=e.external_id WHERE l.file_id=?1 AND l.relation='primary' AND nullif(trim(e.title),'') IS NOT NULL ORDER BY e.source_id,e.external_id",
                    "game"=>"SELECT e.source_id,e.external_id,json_quote(e.game),e.revision FROM provider_entries e JOIN provider_links l ON l.source_id=e.source_id AND l.external_id=e.external_id WHERE l.file_id=?1 AND l.relation='primary' AND nullif(trim(e.game),'') IS NOT NULL ORDER BY e.source_id,e.external_id",
                    _=>"SELECT e.source_id,e.external_id,json_quote(e.created_ms),e.revision FROM provider_entries e JOIN provider_links l ON l.source_id=e.source_id AND l.external_id=e.external_id WHERE l.file_id=?1 AND l.relation='primary' AND e.created_ms IS NOT NULL ORDER BY e.source_id,e.external_id",
                }
            }).map_err(|e|e.to_string())?;
            let candidates = statement
                .query_map([file], |r| {
                    Ok(Claim {
                        source: r.get(0)?,
                        external: r.get(1)?,
                        value: r.get(2)?,
                        revision: r.get(3)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let mut old: Option<(i64,String,String)>=tx.query_row("SELECT source_id,external_id,value_json FROM provider_choices WHERE file_id=?1 AND field=?2",params![file,field],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|e|e.to_string())?;
            if field == "preview" {
                if let Some((_, _, selected)) = &old {
                    let value: Value =
                        serde_json::from_str(selected).map_err(|_| "Invalid selected preview")?;
                    let current: (i64, Option<String>) = tx
                        .query_row(
                            "SELECT size,modified_ns FROM recordings WHERE id=?1",
                            [file],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .map_err(|e| e.to_string())?;
                    if value[2] != json!(current.0) || value[3] != json!(current.1) {
                        tx.execute(
                            "DELETE FROM provider_choices WHERE file_id=?1 AND field='preview'",
                            [file],
                        )
                        .map_err(|e| e.to_string())?;
                        old = None;
                    }
                }
            }
            let values: HashSet<_> = candidates.iter().map(|c| &c.value).collect();
            if values.len() != 1 {
                continue;
            }
            let candidate = if let Some((source, external, _)) = &old {
                if *source != importing {
                    continue;
                }
                candidates
                    .iter()
                    .find(|c| c.source == *source && c.external == *external)
            } else {
                candidates.first()
            };
            if let Some(c) = candidate {
                tx.execute("INSERT INTO provider_choices VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(file_id,field) DO UPDATE SET source_id=excluded.source_id,external_id=excluded.external_id,value_json=excluded.value_json,revision=excluded.revision WHERE provider_choices.value_json<>excluded.value_json",params![file,field,c.source,c.external,c.value,c.revision]).map_err(|e|e.to_string())?;
            }
        }
    }
    Ok(())
}
