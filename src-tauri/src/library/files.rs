use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::UNIX_EPOCH,
};
use tokio::sync::mpsc::Sender;

#[derive(Clone, Debug)]
pub struct File {
    pub path: String,
    pub name: String,
    pub identity: Option<String>,
    pub size: i64,
    pub modified_ms: Option<i64>,
    pub modified_ns: Option<String>,
}

pub fn supported(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["mp4", "m4v", "mov", "mkv", "webm"].contains(&s.to_ascii_lowercase().as_str())
    })
}

fn link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions and other reparse points, not only Rust's symlink type.
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

pub fn local_directory(path: &Path) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || link(&meta) {
        return Err("Choose a local directory, not a symbolic link or reparse point.".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let text = path
        .to_str()
        .ok_or("Folder path cannot be represented as text")?;
    if text.starts_with(r"\\?\UNC\") || (text.starts_with(r"\\") && !text.starts_with(r"\\?\")) {
        return Err("Choose a folder on a local disk, not a network share.".into());
    }
    Ok(path)
}

#[cfg(windows)]
pub fn identity(path: &Path) -> Option<String> {
    use std::{
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::*,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        // Attribute access only. No media content read, exclusive lock or write access.
        let handle = CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info = std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>();
        let ok = GetFileInformationByHandle(handle, &mut info) != 0;
        CloseHandle(handle);
        // A filesystem that cannot supply a usable file index must use conservative path matching.
        (ok && (info.nFileIndexHigh != 0 || info.nFileIndexLow != 0)).then(|| {
            format!(
                "{}:{}:{}:{}:{}",
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
                info.ftCreationTime.dwHighDateTime,
                info.ftCreationTime.dwLowDateTime
            )
        })
    }
}

#[cfg(unix)]
pub fn identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).ok()?;
    // Without a birth time, inode reuse cannot be distinguished confidently. Fall back to paths.
    let birth = meta
        .created()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}:{}:{}", meta.dev(), meta.ino(), birth))
}

#[cfg(not(any(windows, unix)))]
pub fn identity(_: &Path) -> Option<String> {
    None
}

pub fn describe(path: &Path) -> Result<File, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || link(&meta) || !supported(path) {
        return Err("Not a supported regular recording file".into());
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    Ok(File {
        path: path
            .to_str()
            .ok_or("Filename cannot be represented as text")?
            .into(),
        name: path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid filename")?
            .into(),
        identity: identity(path),
        size: i64::try_from(meta.len()).map_err(|_| "File is too large")?,
        modified_ms: modified.and_then(|t| i64::try_from(t.as_millis()).ok()),
        modified_ns: modified.map(|t| t.as_nanos().to_string()),
    })
}

// Recheck every ancestor on use: an indexed path can later be replaced by a junction/symlink.
pub fn within(
    path: &Path,
    root: &Path,
    expected_identity: Option<&str>,
) -> Result<PathBuf, String> {
    let current_root = local_directory(root)?;
    if current_root != root
        || expected_identity.is_some_and(|id| identity(root).as_deref() != Some(id))
    {
        return Err(
            "The indexed folder has changed identity. Remove it and select it again.".into(),
        );
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Recording is outside its selected folder")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Invalid indexed path".into());
        }
        current.push(component);
        let meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
        if link(&meta) {
            return Err("Indexed path now contains a link or reparse point".into());
        }
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(root) {
        return Err("Recording escaped its selected folder".into());
    }
    Ok(canonical)
}

pub enum Entry {
    File(File),
    Issue(String),
    Link,
}

// A bounded channel applies backpressure. At most one directory handle per nesting level is held.
pub fn walk(root: &Path, cancel: Arc<AtomicBool>, output: Sender<Entry>) {
    fn send(output: &Sender<Entry>, entry: Entry) -> bool {
        output.blocking_send(entry).is_ok()
    }
    let initial = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            let _ = send(&output, Entry::Issue(error.to_string()));
            return;
        }
    };
    let root_identity = identity(root);
    let mut stack = vec![initial];
    while !cancel.load(Ordering::Acquire) {
        let Some(entries) = stack.last_mut() else {
            break;
        };
        let Some(item) = entries.next() else {
            stack.pop();
            continue;
        };
        let result = (|| -> Result<(), io::Error> {
            let item = item?;
            let path = item.path();
            let meta = fs::symlink_metadata(&path)?;
            if link(&meta) {
                if !send(&output, Entry::Link) {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
            } else if meta.is_dir() {
                // Recheck canonical containment before enumerating; never follow links intentionally.
                if stack.len() >= 128 || within(&path, root, root_identity.as_deref()).is_err() {
                    return Err(io::Error::other(
                        "Directory is too deep or outside the selected root",
                    ));
                }
                stack.push(fs::read_dir(path)?);
            } else if meta.is_file() && supported(&path) {
                let entry = within(&path, root, root_identity.as_deref())
                    .and_then(|path| describe(&path))
                    .map(Entry::File)
                    .unwrap_or_else(Entry::Issue);
                if !send(&output, entry) {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            if error.kind() == io::ErrorKind::BrokenPipe {
                break;
            }
            if !send(&output, Entry::Issue(error.to_string())) {
                break;
            }
        }
    }
}
