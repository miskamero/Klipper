use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
// Bump when the whitelist or interpretation changes, so unchanged sources are normalized again.
pub const NORMALIZATION_VERSION: &str = "medal-clips-v4";
pub(super) const MAX_TIMESTAMP_MS: i64 = 32_503_680_000_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Metadata {
    pub source_format: String,
    pub local_id: Option<String>,
    pub remote_id: Option<String>,
    pub content_has_title: Option<bool>,
    pub remote_trim_start: Option<f64>,
    pub game_field: Option<String>,
    pub timestamp_warnings: Vec<String>,
    pub retained_timestamps: Vec<String>,
    pub title: Option<String>,
    pub title_field: Option<String>,
    pub local_title: Option<String>,
    pub content_title: Option<String>,
    pub title_difference: bool,
    pub description: Option<String>,
    pub game: Option<String>,
    pub game_id: Option<String>,
    pub game_conflict: bool,
    pub game_lookup_ambiguous: bool,
    pub created_ms: Option<i64>,
    pub content_created_ms: Option<i64>,
    pub last_edit_ms: Option<i64>,
    pub duration: Option<f64>,
    pub tags: Vec<String>,
    pub file_path: Option<String>,
    pub image: Option<String>,
    pub original_file_path: Option<String>,
    pub original_image: Option<String>,
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone)]
pub struct Entry {
    pub id: String,
    pub data: Metadata,
    pub revision: String,
}
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub skipped: usize,
    pub invalid_identities: usize,
}
struct Root(Map<String, Value>);
impl<'de> Deserialize<'de> for Root {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Root;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a catalog object with unique clip IDs")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Root, A::Error> {
                let mut entries = Map::new();
                while let Some((key, value)) = access.next_entry::<String, Value>()? {
                    if entries.len() >= 100_000 || entries.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom(
                            "Catalog has duplicate IDs or too many records",
                        ));
                    }
                }
                Ok(Root(entries))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

pub(super) fn text(value: &Value, limit: usize) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(limit).collect())
}
pub(super) fn epoch(value: &Value, millis_per_unit: i64) -> Option<i64> {
    let millis = if let Some(integer) = value.as_i64() {
        integer.checked_mul(millis_per_unit)?
    } else {
        let scaled = value.as_f64()? * millis_per_unit as f64;
        if !scaled.is_finite() || scaled <= 0.0 || scaled >= MAX_TIMESTAMP_MS as f64 {
            return None;
        }
        // The library uses milliseconds. Round fractional source units, never truncate.
        scaled.round() as i64
    };
    (millis > 0 && millis < MAX_TIMESTAMP_MS).then_some(millis)
}
pub(super) fn select(value: &Value, keys: &[&str]) -> Value {
    let mut map = Map::new();
    for key in keys {
        let v = &value[key];
        let v = if v.is_number() || v.is_boolean() {
            Some(v.clone())
        } else {
            text(v, 4096).map(Value::String)
        };
        if let Some(v) = v {
            map.insert((*key).into(), v);
        }
    }
    Value::Object(map)
}
fn event(value: &Value) -> Value {
    let mut result = select(value, &["name"]);
    // No arbitrary event data, player identities, tokens, window titles or account objects.
    let tags = select(
        &value["tags"],
        &["Map", "Weapon", "KillType", "Mode", "Event"],
    );
    if tags.as_object().is_some_and(|m| !m.is_empty()) {
        result["tags"] = tags;
    }
    result
}

pub fn parse(bytes: &[u8]) -> Result<Catalog, String> {
    let bytes = bytes.strip_prefix(&[239, 187, 191]).unwrap_or(bytes);
    let Root(object) = serde_json::from_slice(bytes)
        .map_err(|_| "The selected catalog is not valid Medal JSON.")?;
    if object.len() > 100_000 {
        return Err("Catalog exceeds 100,000 entries.".into());
    }
    if !object.is_empty()
        && !object
            .values()
            .any(|v| v.is_object() && (v.get("GameTitle").is_some() || v.get("FilePath").is_some()))
    {
        return Err("This JSON does not contain supported Medal clip records.".into());
    }
    // Category names can be present on one clip but absent on another. Never guess a conflicting map.
    let mut games: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for clip in object.values() {
        for (id, name) in [
            (
                &clip["Content"]["category"]["categoryId"],
                &clip["Content"]["category"]["categoryName"],
            ),
            (
                &clip["metadata"]["attributedCategoryId"],
                &clip["metadata"]["attributedCategoryName"],
            ),
        ] {
            if let (Some(id), Some(name)) = (text(id, 128), text(name, 512)) {
                games.entry(id).or_default().insert(name);
            }
        }
    }
    let mut entries = Vec::new();
    let mut skipped = 0;
    for (id, clip) in &object {
        if !clip.is_object() || id.is_empty() || id.len() > 256 {
            skipped += 1;
            continue;
        }
        let content = &clip["Content"];
        let metadata = &clip["metadata"];
        let local_title = text(&clip["GameTitle"], 512);
        let content_title = text(&content["contentTitle"], 512);
        let custom_content = content["hasTitle"].as_bool() == Some(true);
        let (title, title_field) = if custom_content && content_title.is_some() {
            (content_title.clone(), Some("Content.contentTitle".into()))
        } else if local_title.is_some() {
            (local_title.clone(), Some("GameTitle".into()))
        } else {
            (None, None)
        };
        let attributed_name = text(&metadata["attributedCategoryName"], 512);
        let category_name = text(&content["category"]["categoryName"], 512);
        let fallback_id =
            text(&clip["GameCategory"], 128).or_else(|| text(&content["categoryId"], 128));
        let game_id = if attributed_name.is_some() {
            text(&metadata["attributedCategoryId"], 128).or(fallback_id)
        } else if category_name.is_some() {
            text(&content["category"]["categoryId"], 128).or(fallback_id)
        } else {
            fallback_id
        };
        // These are separate title fields with explicit precedence, not competing claims.
        // Likewise, an ambiguous catalog-wide lookup must not discredit a direct game name.
        let game_conflict = attributed_name.is_some()
            && category_name.is_some()
            && attributed_name != category_name;
        let direct_game = attributed_name.or(category_name);
        let lookup = game_id.as_ref().and_then(|id| games.get(id));
        let game_lookup_ambiguous =
            direct_game.is_none() && lookup.is_some_and(|names| names.len() > 1);
        let game = if game_conflict {
            None
        } else {
            direct_game.or_else(|| {
                lookup
                    .filter(|names| names.len() == 1)
                    .and_then(|names| names.first().cloned())
            })
        };
        let mut tags = BTreeSet::new();
        for list in [&clip["tags"], &content["tags"]] {
            if let Some(list) = list.as_array() {
                for tag in list.iter().take(256) {
                    if let Some(tag) = text(tag, 256) {
                        tags.insert(tag);
                    }
                }
            }
        }
        let mut extra = BTreeMap::new();
        if game_conflict {
            extra.insert("gameNames".into(), serde_json::json!({
                "attributedCategoryName": metadata["attributedCategoryName"].as_str().map(|s| s.chars().take(512).collect::<String>()),
                "categoryName": content["category"]["categoryName"].as_str().map(|s| s.chars().take(512).collect::<String>())
            }));
        }
        let properties = select(
            clip,
            &[
                "uuid",
                "clipID",
                "clipType",
                "contentType",
                "SaveType",
                "Status",
                "fileSource",
                "isEditedWithMedal",
                "editType",
                "editStatus",
                "Deleted",
                "isProject",
                "isRender",
                "isRestoredOriginal",
                "trimStartTime",
                "trimEndTime",
            ],
        );
        extra.insert("properties".into(), properties);
        for (name, v) in [
            (
                "capture",
                select(
                    metadata,
                    &["triggerType", "clipDurationSec", "isWebcamCaptured"],
                ),
            ),
            (
                "captureSettings",
                select(&metadata["quality"], &["resolution", "fps", "bitrate"]),
            ),
            ("event", event(&metadata["eventobj"])),
            (
                "trim",
                select(
                    &clip["edit"]["trim"],
                    &["trimStartTimeInSeconds", "trimDurationInSeconds"],
                ),
            ),
        ] {
            if v.as_object().is_some_and(|m| !m.is_empty()) {
                extra.insert(name.into(), v);
            }
        }
        if let Some(history) = clip["editHistory"].as_array() {
            extra.insert(
                "trimHistory".into(),
                Value::Array(
                    history
                        .iter()
                        .take(256)
                        .map(|v| {
                            select(
                                &v["trim"],
                                &["trimStartTimeInSeconds", "trimDurationInSeconds"],
                            )
                        })
                        .filter(|v| v.as_object().is_some_and(|m| !m.is_empty()))
                        .collect(),
                ),
            );
        }
        if let Some(bookmarks) = clip["bookmarks"].as_array() {
            extra.insert(
                "bookmarks".into(),
                Value::Array(
                    bookmarks
                        .iter()
                        .take(256)
                        .map(|v| {
                            let mut item = select(v, &["time", "label", "duration"]);
                            let ev = event(&v["metadata"]["eventobj"]);
                            if ev.as_object().is_some_and(|m| !m.is_empty()) {
                                item["event"] = ev;
                            }
                            item
                        })
                        .collect(),
                ),
            );
        }
        if let Some(audio) = clip["editLayer"]["audioStreams"].as_array() {
            extra.insert(
                "editAudioStreams".into(),
                Value::Array(
                    audio
                        .iter()
                        .take(64)
                        .map(|v| {
                            select(
                                v,
                                &[
                                    "index",
                                    "title",
                                    "displayLabel",
                                    "isMuted",
                                    "isIncludeInMix",
                                ],
                            )
                        })
                        .collect(),
                ),
            );
        }
        let data = Metadata {
            source_format: "json".into(),
            local_id: Some(id.clone()),
            remote_id: text(&content["contentId"], 256),
            content_has_title: content["hasTitle"].as_bool(),
            remote_trim_start: content["trimStartTime"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0),
            game_field: game
                .as_ref()
                .map(|_| "JSON category name or verified catalog lookup".into()),
            timestamp_warnings: Vec::new(),
            retained_timestamps: Vec::new(),
            title,
            title_field,
            title_difference: local_title.is_some()
                && content_title.is_some()
                && local_title != content_title,
            local_title,
            content_title,
            description: text(&content["contentDescription"], 4096),
            game,
            game_id,
            game_conflict,
            game_lookup_ambiguous,
            created_ms: epoch(&clip["TimeCreated"], 1000),
            content_created_ms: epoch(&content["created"], 1),
            last_edit_ms: epoch(&clip["lastEdit"], 1),
            duration: clip["duration"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0),
            tags: tags.into_iter().collect(),
            file_path: text(&clip["FilePath"], 32768),
            image: text(&clip["Image"], 32768),
            original_file_path: text(&clip["originalFilePath"], 32768),
            original_image: text(&clip["originalThumbnailPath"], 32768),
            extra,
        };
        let revision = serde_json::to_string(&data).map_err(|_| "Could not normalize catalog.")?;
        if revision.len() > 256 * 1024 {
            skipped += 1;
            continue;
        }
        entries.push(Entry {
            id: id.clone(),
            data,
            revision,
        });
    }
    if entries.is_empty() && !object.is_empty() {
        return Err("No supported clip records were found.".into());
    }
    Ok(Catalog {
        entries,
        skipped,
        invalid_identities: 0,
    })
}
