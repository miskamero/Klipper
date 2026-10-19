# Klipper

A free, open-source desktop application built with Tauri, React, and Rust for organizing, playing, and editing mainly gameplay recordings.

Klipper brings your local clips into one searchable library, recovers titles and other metadata from Medal, and provides native video playback and a simple trim-and-export editor—all without uploading your recordings or modifying the originals.

**Find your clips. Watch your moments. Make the cut.**


## Features

### Clip Library

- **Collections:** Browse all recordings, imported sources, or individual folders in a visual thumbnail gallery or compact list.
- **Folder discovery:** Add folders, index recordings recursively, and rescan without touching the original files.
- **Search and filters:** Find clips by title, game, or filename; filter and sort recordings using their available metadata.
- **Medal integration:** Import clip titles, games, timestamps, tags, bookmarks, and thumbnails from `clips.json` or supported Medal SQLite databases. Review matches before importing.
- **Local storage:** Keep library metadata in SQLite and reuse cached thumbnails for fast browsing. Resetting the library never deletes your recordings.

### Video Player

- Native **libmpv playback on Windows**, including HEVC/H.265 and supported hardware-accelerated decoding.
- Play, pause, seek, adjust volume, change playback speed (**0.25×–3×**), and use video-only fullscreen.
- Inspect video properties and individual audio streams in a collapsible details panel.
- Open clips from your library or play any supported local file in the standalone Player, without adding it to the library.

### Video Editor

- Trim recordings with a visual, frame-based timeline, draggable in/out points, precise time inputs, and frame stepping.
- Preview a selected segment before exporting.
- Export a **new H.264 MP4** using CPU encoding or **NVIDIA NVENC** when available.
- Preserve supported audio streams as separate AAC tracks.
- Queue exports in the background, monitor progress, cancel jobs, and continue browsing or editing other clips.
- Validate completed exports before saving them. **Original recordings are never overwritten.**

**Current limitations:** The editor supports a single continuous trim and 8-bit SDR video. Independent audio-track mixing, HEVC export, replacing originals with backups, and saved editing projects are not yet implemented. The export queue lasts for the current application session.

## Installation & Setup

Klipper currently runs from source on **Windows 10/11 (x64)**. Linux support has not been fully validated.

### Prerequisites

- **Node.js** 22.22.2+ or 24.15+, with npm.
- **Rust** stable MSVC toolchain (declared minimum: 1.90).
- **Visual Studio 2022 C++ Build Tools** and Windows SDK.
- **Microsoft Edge WebView2 Runtime**.
- **FFmpeg and FFprobe** available on `PATH`.
- The Windows x64 **libmpv** build pinned in [`scripts/libmpv.json`](scripts/libmpv.json).

See [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows) for help setting up the development environment.

### Run from Source

1. Clone the repository and install its dependencies:

   ```powershell
   git clone https://github.com/miskamero/Klipper.git
   cd Klipper
   npm ci
   ```

2. Download and extract the libmpv archive identified in [`scripts/libmpv.json`](scripts/libmpv.json). Verify the archive's SHA-256 checksum against the manifest, then register the DLL:

   ```powershell
   npm run native:setup -- "C:\path\to\libmpv-2.dll"
   npm run native:check
   ```

3. Start Klipper:

   ```powershell
   npm run desktop
   ```

The setup script checks the DLL and copies it into the ignored `native-deps/windows-x64/` directory. It does not install libmpv system-wide. The pinned build also needs `vulkan-1.dll`, normally provided by a compatible graphics driver.

If Rust linking fails, try the **x64 Developer PowerShell for Visual Studio**.

## How to Use

1. **Build your library:** Select **Add folder** to index local recordings, or use **Open recording** to play a file just once.
2. **Bring in Medal data:** Choose **Import from Medal**, select a supported catalog or database, review the matches, and confirm. Klipper does not automatically add unindexed videos without approval.
3. **Find and watch clips:** Browse Collections, search or filter your recordings, and open a clip. Returning from playback preserves your place in the library.
4. **Trim and export:** Select **Edit video**, adjust the timeline, and export a new video. The background queue lets you continue using Klipper while it finishes.

Klipper runs locally without accounts, telemetry, or cloud uploads. It only indexes folders you explicitly choose. **Manage library** lets you rescan, remove folders, or reset Klipper's library without deleting the source files.

## Development

Klipper uses **Tauri v2, Rust, React, TypeScript, and SCSS Modules**. Native playback is implemented in `src-tauri/src/native_player/`, with its frontend components in `src/components/native/`.

```powershell
npm run format:check
npm run lint
npm run typecheck
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

To compile a desktop executable, run `npm run desktop:build`. The resulting executable is **not yet a portable distribution**: libmpv and the FFmpeg/FFprobe tools still need to be provided, along with the applicable third-party licensing materials. The release executable requires `libmpv-2.dll` beside it.

## License

Klipper's application code is licensed under **[GPL-3.0-only](LICENSE)**. Third-party media libraries and tools retain their respective licenses and redistribution requirements.