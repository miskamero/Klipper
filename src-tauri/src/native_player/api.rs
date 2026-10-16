// Minimal libmpv C API binding for the native playback worker.
// The context is created, used, and destroyed exclusively on the player worker.
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{Foundation::*, System::LibraryLoader::*};
type Context = *mut c_void;
type Create = unsafe extern "C" fn() -> Context;
type Initialize = unsafe extern "C" fn(Context) -> i32;
type Destroy = unsafe extern "C" fn(Context);
type SetOption = unsafe extern "C" fn(Context, *const c_char, *const c_char) -> i32;
type Command = unsafe extern "C" fn(Context, u64, *const *const c_char) -> i32;
type Property = unsafe extern "C" fn(Context, *const c_char) -> *mut c_char;
type Free = unsafe extern "C" fn(*mut c_void);
type WaitEvent = unsafe extern "C" fn(Context, f64) -> *const Event;

#[repr(C)]
pub(super) struct Event {
    pub(super) id: i32,
    pub(super) error: i32,
    reply: u64,
    pub(super) data: *mut c_void,
}

pub(super) struct Player {
    library: HMODULE,
    pub(super) context: Context,
    destroy: Destroy,
    command: Command,
    property: Property,
    free: Free,
    pub(super) wait_event: WaitEvent,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

impl Player {
    pub(super) unsafe fn new(stage: HWND) -> Result<Self, String> {
        Self::from_dll(stage, &library_path()?)
    }

    unsafe fn from_dll(stage: HWND, dll: &std::path::Path) -> Result<Self, String> {
        if !dll.is_file() {
            return Err(missing_dll_message());
        }
        let library = LoadLibraryExW(
            wide(dll.to_str().ok_or("Invalid DLL path")?).as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if library.is_null() {
            return Err(format!(
                "Could not load libmpv from {} (Windows error {}). Check that the approved x64 DLL and its dependencies, including the Vulkan loader required by this build, are available. No system codec extension is needed.",
                dll.display(), GetLastError()
            ));
        }
        macro_rules! symbol {
            ($name:literal, $kind:ty) => {{
                let address = GetProcAddress(library, concat!($name, "\0").as_ptr());
                let Some(address) = address else {
                    FreeLibrary(library);
                    return Err(format!("libmpv export missing: {}", $name));
                };
                std::mem::transmute::<unsafe extern "system" fn() -> isize, $kind>(address)
            }};
        }
        let create = symbol!("mpv_create", Create);
        let initialize = symbol!("mpv_initialize", Initialize);
        let option = symbol!("mpv_set_option_string", SetOption);
        let mut player = Self {
            library,
            context: null_mut(),
            destroy: symbol!("mpv_terminate_destroy", Destroy),
            command: symbol!("mpv_command_async", Command),
            property: symbol!("mpv_get_property_string", Property),
            free: symbol!("mpv_free", Free),
            wait_event: symbol!("mpv_wait_event", WaitEvent),
        };
        player.context = create();
        if player.context.is_null() {
            return Err("mpv_create failed".into());
        }
        for (name, value) in [
            ("config", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            ("autoload-files", "no"),
            ("access-references", "no"),
            ("audio-file-auto", "no"),
            ("sub-auto", "no"),
            ("cover-art-auto", "no"),
            ("save-position-on-quit", "no"),
            ("resume-playback", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("input-cursor", "no"),
            ("input-media-keys", "no"),
            ("osc", "no"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("vo", "gpu-next"),
            ("gpu-api", "d3d11"),
            ("gpu-context", "d3d11"),
            ("hwdec", "auto"),
            ("demuxer-lavf-o", "protocol_whitelist=file"),
        ] {
            if option(
                player.context,
                CString::new(name).unwrap().as_ptr(),
                CString::new(value).unwrap().as_ptr(),
            ) < 0
            {
                return Err(format!("libmpv rejected option {name}={value}"));
            }
        }
        let window = CString::new((stage as usize).to_string()).unwrap();
        if option(player.context, c"wid".as_ptr(), window.as_ptr()) < 0
            || initialize(player.context) < 0
        {
            return Err("libmpv initialization failed".into());
        }
        Ok(player)
    }

    pub(super) unsafe fn send(&self, arguments: &[&str]) -> Result<(), String> {
        let arguments: Vec<_> = arguments
            .iter()
            .map(|value| CString::new(*value))
            .collect::<Result<_, _>>()
            .map_err(|_| "Command contained a null character")?;
        let mut pointers: Vec<_> = arguments.iter().map(|value| value.as_ptr()).collect();
        pointers.push(null());
        let result = (self.command)(self.context, 0, pointers.as_ptr());
        if result < 0 {
            Err(format!("mpv command rejected: {result}"))
        } else {
            Ok(())
        }
    }

    pub(super) unsafe fn get(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        let value = (self.property)(self.context, name.as_ptr());
        if value.is_null() {
            return None;
        }
        let text = CStr::from_ptr(value).to_string_lossy().into_owned();
        (self.free)(value.cast());
        Some(text)
    }
}

pub(super) fn library_path() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../native-deps/windows-x64/libmpv-2.dll"))
    }
    #[cfg(not(debug_assertions))]
    {
        packaged_library(&std::env::current_exe().map_err(|e| e.to_string())?)
    }
}

#[cfg(any(test, not(debug_assertions)))]
fn packaged_library(executable: &std::path::Path) -> Result<PathBuf, String> {
    Ok(executable
        .parent()
        .ok_or("Application directory unavailable")?
        .join("libmpv-2.dll"))
}

pub(super) fn missing_dll_message() -> String {
    #[cfg(debug_assertions)]
    {
        "Native playback is unavailable. Run npm run native:setup -- <extracted-libmpv-2.dll> to install the pinned Windows x64 development DLL in native-deps/windows-x64/. It is not downloaded automatically. HEVC requires the native player in this environment.".into()
    }
    #[cfg(not(debug_assertions))]
    {
        "Native playback is unavailable. The Windows x64 libmpv-2.dll must be beside the Klipper executable, with its required dependencies. Repair the application distribution and retry. It is not downloaded automatically.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_returns_an_actionable_error_without_initializing_mpv() {
        let directory = tempfile::tempdir().unwrap();
        let result = unsafe { Player::from_dll(null_mut(), &directory.path().join("absent.dll")) };
        let error = result.err().expect("missing DLL must fail");
        #[cfg(debug_assertions)]
        assert!(error.contains("native-deps/windows-x64/"));
        #[cfg(not(debug_assertions))]
        assert!(error.contains("beside the Klipper executable"));
        assert!(error.contains("not downloaded automatically"));
    }
    #[test]
    fn packaged_player_uses_only_the_executable_directory() {
        let executable = PathBuf::from(r"C:\Portable\Klipper\klipper.exe");
        assert_eq!(
            packaged_library(&executable).unwrap(),
            executable.parent().unwrap().join("libmpv-2.dll")
        );
        #[cfg(debug_assertions)]
        assert!(library_path()
            .unwrap()
            .ends_with("native-deps/windows-x64/libmpv-2.dll"));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_null() {
                (self.destroy)(self.context);
            }
            FreeLibrary(self.library);
        }
    }
}
