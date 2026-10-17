use super::{db::Db, medal::paths};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub root_id: i64,
    pub name: String,
    pub path: String,
    pub recordings: usize,
}
#[derive(Serialize)]
pub struct Source {
    pub kind: String,
    pub recordings: i64,
}
#[derive(Default, Serialize)]
pub struct Navigation {
    pub total: i64,
    pub sources: Vec<Source>,
    pub folders: Vec<Folder>,
}

// These collections describe known SQLite associations, never a fresh filesystem traversal.
impl Db {
    pub fn navigation(&self) -> Result<Navigation, String> {
        let total = self
            .connection
            .query_row(
                "SELECT count(DISTINCT file_id) FROM library_locations",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let mut s=self.connection.prepare("SELECT s.kind,count(DISTINCT p.file_id) FROM provider_links p JOIN sources s ON s.id=p.source_id WHERE EXISTS(SELECT 1 FROM library_locations l WHERE l.file_id=p.file_id AND l.availability='available') GROUP BY s.kind ORDER BY s.kind").map_err(|e|e.to_string())?;
        let sources = s
            .query_map([], |r| {
                Ok(Source {
                    kind: r.get(0)?,
                    recordings: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut folders = Vec::new();
        for root in self.roots()? {
            let mut nodes: BTreeMap<String, (String, String, HashSet<i64>)> = BTreeMap::new();
            let root_token = format!("root:{}", root.id);
            let mut s=self.connection.prepare("SELECT l.id,l.file_id,l.path FROM locations l JOIN root_entries e ON e.location_id=l.id WHERE e.root_id=?1 ORDER BY l.id").map_err(|e|e.to_string())?;
            for row in s
                .query_map([root.id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
            {
                let (location, file, path) = row.map_err(|e| e.to_string())?;
                let Some(parent) = Path::new(&path).parent() else {
                    continue;
                };
                let Ok(relative) = parent.strip_prefix(&root.path) else {
                    continue;
                };
                let mut directory = std::path::PathBuf::from(&root.path);
                for (depth, component) in relative.components().enumerate() {
                    directory.push(component);
                    let text = directory.to_string_lossy().into_owned();
                    let Some(key) = paths::key(&text) else {
                        continue;
                    };
                    let node = nodes.entry(key).or_insert_with(|| {
                        (
                            format!("{}:{location}:{}", root.id, depth + 1),
                            text,
                            HashSet::new(),
                        )
                    });
                    node.2.insert(file);
                }
            }
            folders.push(Folder {
                id: root_token.clone(),
                parent_id: None,
                root_id: root.id,
                name: Path::new(&root.path)
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.path.clone()),
                path: root.path.clone(),
                recordings: root.recordings as usize,
            });
            for (token, path, files) in nodes.values() {
                let parent_key = Path::new(path)
                    .parent()
                    .and_then(|p| paths::key(&p.to_string_lossy()));
                let parent_id = parent_key
                    .and_then(|key| nodes.get(&key).map(|n| n.0.clone()))
                    .unwrap_or_else(|| root_token.clone());
                folders.push(Folder {
                    id: token.clone(),
                    parent_id: Some(parent_id),
                    root_id: root.id,
                    name: Path::new(path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: path.clone(),
                    recordings: files.len(),
                });
            }
        }
        Ok(Navigation {
            total,
            sources,
            folders,
        })
    }

    // A folder token contains only a known root, location and ancestor depth. No IPC path is used.
    pub fn folder_scope(&self, token: &str) -> Result<(i64, String), String> {
        let parts = token
            .split(':')
            .map(str::parse::<i64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Invalid library folder")?;
        if parts.len() != 3 || parts[2] < 1 || parts[2] > 256 {
            return Err("Invalid library folder".into());
        }
        let row:Option<(String,String)>=self.connection.query_row("SELECT r.path,l.path FROM roots r JOIN root_entries e ON e.root_id=r.id JOIN locations l ON l.id=e.location_id WHERE r.id=?1 AND l.id=?2",[parts[0],parts[1]],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        let (root, path) = row.ok_or("This library folder is no longer available")?;
        let parent = Path::new(&path).parent().ok_or("Invalid folder location")?;
        let relative = parent
            .strip_prefix(&root)
            .map_err(|_| "Folder is outside its root")?;
        let components: Vec<_> = relative.components().collect();
        if parts[2] as usize > components.len() {
            return Err("Invalid library folder depth".into());
        }
        let mut folder = std::path::PathBuf::from(root);
        for c in &components[..parts[2] as usize] {
            folder.push(c);
        }
        let mut prefix = folder.to_string_lossy().replace('\\', "/");
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        Ok((parts[0], prefix))
    }
}
