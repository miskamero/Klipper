# Klipper

Klipper is a free, open-source desktop application for playing and inspecting local gameplay recordings. It opens your recordings read-only: no account, no uploads, and no telemetry.

The project is still in development. Local folder discovery, a searchable library with video previews, playback, and media inspection are working. Editing tools, compression, and provider metadata imports are not yet implemented.

## Features

### Local library

- Add multiple folders through the native folder picker and discover recordings recursively.
- Search filenames and paths, filter by folder, status or codec, and sort by date, name, duration or size.
- Rescan incrementally, cancel background indexing, or remove folders without deleting recordings.
- Keep library metadata in a local SQLite database in Klipper's platform application-data directory.
- Generate real frame previews only for visible recordings, with cancellable background work.

Previews use FFmpeg and are cached separately in the platform application-cache directory, under thumbnails/previews-v1.sqlite3. JPEGs fit within 320×180 pixels; the cache retains at most 2,000 images and 64 MiB of image data (an 80 MiB database-file ceiling, plus temporary SQLite journal space). Changed recordings invalidate previews. Clearing this cache is safe while Klipper is closed.

Discovery skips symbolic links and Windows reparse points. Unchanged recordings reuse cached inspection results; missing files retain their historical metadata. Filesystem identities help recognize moves within selected folders and hard links where supported; uncertain matches remain separate. Folders are only scanned after you add or explicitly rescan them.

### Video playback

- Native **libmpv playback on Windows**, including HEVC/H.265 recordings.
- Hardware-accelerated decoding when supported by the system.
- Play/pause, seeking, volume and mute, and playback speeds from **0.25× to 3×**.
- Video-only fullscreen mode with mouse-over controls.
- Switch recordings without restarting the application.

### Recording details

- Inspect container, codec, resolution, frame rate, duration, and other available video properties using FFprobe.
- View metadata for individual audio streams, including recordings with multiple tracks.
- Use a resizable, collapsible inspector beside the player.

**Formats:** MP4 is the primary format. M4V, MOV, MKV, and WebM can also be selected, although actual playback depends on the codecs and playback backend. HEVC playback with D3D11VA hardware decoding has been verified on Windows with an NVIDIA RTX 3080; other hardware configurations remain unverified.

> **Current limitation:** Audio tracks can be inspected, but independent track selection and mixing are not implemented. Klipper is not yet a clip editor.

## Getting started

Klipper currently targets **Windows 10 and 11 (x64)** and is run from source. A ready-to-download portable release is not available yet. Linux support has not been validated and currently uses HTML video rather than libmpv.

### Prerequisites

- **Node.js:** 22.22.2+ (Node 22) or 24.15+ (Node 24), with npm.
- **Rust:** current stable Windows MSVC toolchain; the declared minimum is Rust 1.90.
- **Visual Studio 2022 C++ Build Tools:** _Desktop development with C++_ and the Windows SDK.
- **Microsoft Edge WebView2 Runtime.**
- **FFmpeg and FFprobe** available on `PATH` for thumbnail extraction and recording inspection.
- **libmpv for Windows x64**, installed locally as described below.

See [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows) for the development environment.

### Run from source

1. Clone the repository and open a terminal in its root directory.
2. Install JavaScript dependencies:

    ```powershell
    npm ci
    ```

3. Download and extract the Windows x64 libmpv archive identified in [`scripts/libmpv.json`](scripts/libmpv.json). Verify the archive against the SHA-256 listed there.
4. Register the extracted DLL with Klipper:

    ```powershell
    npm run native:setup -- "C:\path\to\extracted\libmpv-2.dll"
    npm run native:check
    ```

5. Launch Klipper:

    ```powershell
    npm run desktop
    ```

The setup command verifies the DLL checksum and copies it into the ignored `native-deps/windows-x64/` directory, where development and debug builds load it. It refuses to overwrite an existing DLL with a different checksum. It does not install anything system-wide or download binaries automatically. The pinned libmpv build also requires a Vulkan loader (`vulkan-1.dll`), available with the tested NVIDIA driver.

If Rust linking fails because Windows SDK or MSVC libraries cannot be found, use the **x64 Developer PowerShell for Visual Studio**.

## Using Klipper

1. In **Library**, choose **Add folder** to index a recording folder, then open a recording from the list. Alternatively, use **Open recording** to play a single file.
2. Watch the recording in the player and use the transport controls to seek, pause, adjust volume, or change playback speed.
3. Expand the inspector to examine video and audio-stream information.
4. Use the player's fullscreen button for video-only fullscreen. Move the mouse to reveal the controls; press **Escape** to return.

Use **Library** to return to your current search, filters and browsing position. Expand **Folders** to rescan or remove a source; removal affects only library associations, never source files. The database uses the platform application-data location for org.klipper.desktop (on Windows, %APPDATA%\org.klipper.desktop\library.sqlite3).

Klipper never scans drives or recording folders without selection. Opening an indexed recording revalidates it within a selected root and grants the same single-file playback access as the native file picker.

## Development

Klipper uses **Tauri v2, Rust, React, TypeScript, and SCSS Modules**. The native Windows player is implemented in `src-tauri/src/native_player/`, with its React components in `src/components/native/`.

`npm run dev` serves a browser preview; native file selection and window controls require `npm run desktop`. Frontend formatting uses Prettier with actual tabs at width four; Rust uses rustfmt.

Useful checks:

```powershell
npm run format:check
npm run lint
npm run typecheck
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

To compile the desktop executable without creating an installer:

```powershell
npm run desktop:build
```

The executable is written to `src-tauri/target/release/klipper.exe`. Copy the verified `native-deps/windows-x64/libmpv-2.dll` beside that executable before running it. The build does not copy the DLL automatically. Recording inspection still needs FFprobe on `PATH`; WebView2 remains an OS prerequisite.

Thumbnail extraction also needs FFmpeg. Portable packaging should include vetted FFmpeg and FFprobe helpers under resources/bin, with any DLLs required by the chosen builds and their corresponding license/source materials. The development build falls back to PATH; no binaries are downloaded at runtime.

**A successful build is not yet a complete portable distribution**: dependency bundling, clean-machine validation, and the necessary third-party license and source materials are still pending. End users of a future packaged release will not need Node.js, Rust, or Python.

## License

Klipper's application code is licensed under **[GPL-3.0-only](LICENSE)**. Ignored legacy utilities and private data are excluded from that application license scope. Native media libraries have their own licenses and redistribution requirements; their binaries are not included in this repository.
