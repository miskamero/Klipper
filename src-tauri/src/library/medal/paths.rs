use super::super::files;
use std::path::{Component, Path, PathBuf};

// Pure comparison of absolute paths; no basename, timestamps or edit-suffix guessing.
pub fn key(path: &str) -> Option<String> {
    let value = path.trim();
    if cfg!(windows) || value.as_bytes().get(1) == Some(&b':') {
        let value = value.replace('/', "\\");
        let value = value.strip_prefix(r"\\?\").unwrap_or(&value);
        if value.len() < 3
            || value.as_bytes()[1] != b':'
            || value.as_bytes()[2] != b'\\'
            || !value.as_bytes()[0].is_ascii_alphabetic()
        {
            return None;
        }
        let mut parts = Vec::new();
        for part in value.split('\\') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.len() <= 1 {
                        return None;
                    }
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        return Some(parts.join("\\").to_lowercase());
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return None;
    }
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            p => result.push(p.as_os_str()),
        }
    }
    result.to_str().map(str::to_owned)
}

// Catalog references confer no folder grant. Reject links at every ancestor and all network paths.
pub fn local_file(path: &str) -> Result<PathBuf, String> {
    key(path).ok_or("Not an absolute local path")?;
    let path = Path::new(path);
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta =
            std::fs::symlink_metadata(ancestor).map_err(|_| "Referenced file is unavailable")?;
        if meta.file_type().is_symlink() {
            return Err("Referenced path contains a symbolic link".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err("Referenced path contains a reparse point".into());
            }
        }
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "Referenced file is unavailable")?;
    if key(&canonical.to_string_lossy()) != key(&path.to_string_lossy()) {
        return Err("Referenced path changed location".into());
    }
    if !canonical.is_file() {
        return Err("Referenced path is not a file".into());
    }
    Ok(canonical)
}
pub fn video(path: &str) -> Result<files::File, String> {
    files::describe(&local_file(path)?)
}
pub fn image(path: &str) -> Option<(String, String)> {
    let path = local_file(path).ok()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !["jpg", "jpeg", "png", "webp"].contains(&ext.as_str()) {
        return None;
    }
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() == 0 || meta.len() > 16 * 1024 * 1024 {
        return None;
    }
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .to_string();
    let path = path.to_str()?.to_owned();
    let revision = serde_json::to_string(&(
        "medal-image-v1",
        &path,
        files::identity(Path::new(&path)),
        meta.len(),
        modified,
    ))
    .ok()?;
    Some((path, revision))
}
