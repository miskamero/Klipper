use std::path::PathBuf;
use tauri::Manager;

pub fn secure_runtime() {
    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{
            SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_APPLICATION_DIR,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        };
        // Also protect optional libraries loaded internally by libmpv (for example GPU APIs).
        // Their search must never include the launcher's working directory or PATH.
        assert_ne!(
            SetDefaultDllDirectories(
                LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
            ),
            0,
            "Could not configure safe Windows DLL loading"
        );
    }
}

pub(crate) fn secure_media_command(command: &mut tokio::process::Command) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        let executable = std::path::PathBuf::from(command.as_std().get_program());
        if executable.is_absolute() {
            if let Some(directory) = executable.parent() {
                command.current_dir(directory).env_remove("PATH");
            }
        }
    }
    #[cfg(not(all(windows, not(debug_assertions))))]
    let _ = command;
}

pub(crate) fn media_start_error(tool: &str) -> String {
    if cfg!(debug_assertions) {
        format!("{tool} could not start. Install FFmpeg with {tool} on PATH, or provide it in resources/bin, then restart Klipper.")
    } else {
        format!("Bundled {tool} could not start. Repair or extract the complete Klipper distribution and check file permissions. System PATH tools are not used in release builds.")
    }
}

// Packaged releases use only their own bin directory. PATH is a development convenience,
// never a substitute for a missing or damaged distribution dependency.
pub fn ffprobe(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    helper(app, "ffprobe")
}

pub fn ffmpeg(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    helper(app, "ffmpeg")
}

fn helper(app: &tauri::AppHandle, tool: &str) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.into()
    };
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("bin")
        .join(&name);
    resolve_helper(bundled, &name, cfg!(debug_assertions))
}

fn resolve_helper(bundled: PathBuf, name: &str, development: bool) -> Result<PathBuf, String> {
    if bundled.is_file() {
        return Ok(bundled);
    }
    if development {
        return Ok(PathBuf::from(name));
    }
    Err(format!(
        "Bundled {name} is missing. Repair or extract the complete Klipper distribution; system PATH tools are not used in release builds."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_requires_the_bundled_tool_even_if_path_has_another_copy() {
        let directory = tempfile::tempdir().unwrap();
        let bundled = directory.path().join("ffmpeg.exe");
        assert!(resolve_helper(bundled.clone(), "ffmpeg.exe", false)
            .unwrap_err()
            .contains("system PATH tools are not used"));
        assert_eq!(
            resolve_helper(bundled.clone(), "ffmpeg.exe", true).unwrap(),
            PathBuf::from("ffmpeg.exe")
        );
        std::fs::write(&bundled, b"synthetic tool placeholder").unwrap();
        assert_eq!(
            resolve_helper(bundled.clone(), "ffmpeg.exe", false).unwrap(),
            bundled
        );
    }

    #[cfg(all(windows, not(debug_assertions)))]
    #[test]
    fn bundled_subprocesses_do_not_inherit_a_launchers_dll_search_directories() {
        let mut command = tokio::process::Command::new(r"C:\Klipper\bin\ffmpeg.exe");
        secure_media_command(&mut command);
        assert_eq!(
            command.as_std().get_current_dir(),
            Some(std::path::Path::new(r"C:\Klipper\bin"))
        );
        assert!(command
            .as_std()
            .get_envs()
            .any(|(name, value)| name == "PATH" && value.is_none()));
    }
}
