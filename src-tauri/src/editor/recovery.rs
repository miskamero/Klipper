//! One owned-output receipt; never a source-file replacement journal.
use crate::library::files;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, PartialEq)]
struct Receipt {
    path: PathBuf,
    identity: String,
    parent: Option<String>,
}
pub struct Recovery {
    file: PathBuf,
    receipt: Receipt,
}
impl Recovery {
    pub fn create(directory: &Path, path: &Path, identity: &str) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|_| "Could not create export recovery storage")?;
        let file = directory.join("pending-output.json");
        let receipt = Receipt {
            path: path.into(),
            identity: identity.into(),
            parent: path.parent().and_then(files::identity),
        };
        let bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
        let mut handle = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .map_err(|_| "An interrupted export receipt needs cleanup; reopen the editor")?;
        handle
            .write_all(&bytes)
            .and_then(|_| handle.sync_all())
            .map_err(|_| "Could not save export recovery receipt")?;
        Ok(Self { file, receipt })
    }
}
impl Drop for Recovery {
    fn drop(&mut self) {
        if !self.receipt.path.try_exists().unwrap_or(true)
            && fs::read(&self.file)
                .ok()
                .and_then(|b| serde_json::from_slice::<Receipt>(&b).ok())
                .as_ref()
                == Some(&self.receipt)
        {
            let _ = fs::remove_file(&self.file);
        }
    }
}
pub fn recover(directory: &Path) -> Result<(), String> {
    let file = directory.join("pending-output.json");
    if !file
        .try_exists()
        .map_err(|_| "Could not inspect export recovery storage")?
    {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(&file).map_err(|_| "Could not inspect export receipt")?;
    if metadata.len() > 16 * 1024 || metadata.file_type().is_symlink() {
        return Err("Invalid export recovery receipt. It was left untouched.".into());
    }
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(&file).map_err(|_| "Could not read export receipt")?)
            .map_err(|_| "Invalid export recovery receipt; it was left untouched")?;
    if let Ok(meta) = fs::symlink_metadata(&receipt.path) {
        let name = receipt
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let parent = receipt.path.parent().ok_or("Invalid recovery path")?;
        if !name.starts_with(".klipper-export-")
            || !name.ends_with(".mp4")
            || !meta.is_file()
            || meta.file_type().is_symlink()
            || files::identity(&receipt.path).as_deref() != Some(&receipt.identity)
            || files::local_directory(parent)? != parent
            || files::identity(parent) != receipt.parent
        {
            return Err("Interrupted output changed identity; it was left untouched. Review Klipper's export recovery receipt before continuing.".into());
        }
        fs::remove_file(&receipt.path).map_err(|_|"An interrupted temporary export could not be cleaned up. Close any process using it and reopen the editor.")?;
    } else if receipt
        .path
        .try_exists()
        .map_err(|_| "Interrupted output folder is inaccessible; cleanup was deferred")?
    {
        return Err("Interrupted export cleanup was deferred".into());
    }
    fs::remove_file(file)
        .map_err(|_| "Could not remove the completed export recovery receipt".into())
}
