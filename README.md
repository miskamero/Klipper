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

- **Node.js** 22.x (22.22.2+), 24.x (24.15.0+), or 26+, with npm (see `package.json` engines).
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
    npm.cmd run native:setup -- "C:\path\to\libmpv-2.dll"
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

To compile an executable without packaging, run `npm run desktop:build`. A release executable requires `libmpv-2.dll` beside it and `bin/ffmpeg.exe` and `bin/ffprobe.exe`; it does not fall back to system PATH tools.

### Windows x64 packages

From an x64 Visual Studio developer terminal, after installing the pinned libmpv as above:

```powershell
npm run dist:prepare
npm run dist:check
cargo fetch --locked --manifest-path src-tauri/Cargo.toml
npm run dist:licenses
npm.cmd run dist:licenses:check -- --internal-test
npm.cmd run dist:windows -- --internal-test
```

`dist:prepare` downloads checksum-pinned FFmpeg/FFprobe and Vulkan runtime archives into ignored `native-deps/` storage and provisions the pinned NSIS toolset in `src-tauri/target/.tauri/`. No existing per-user NSIS cache is needed. The distribution overlay enables Tauri's supported `useLocalToolsDir` option. All tools are authenticated before use; the complete installer tool inventory is checked before and after packaging. It does not install system software. Windows' built-in `tar.exe` extracts the verified archives; media archives use explicit required members. Normal development still uses `npm run desktop` and supports FFmpeg/FFprobe on PATH.

Packaging produces an application folder, ZIP, per-user NSIS installer, application-source snapshot and `SHA256SUMS.txt` under `release/windows-x64/<version>-internal-test-<unique suffix>/`. Only explicitly selected application resources are bundled. Application sources are selected from Git-listed maintained paths, including intentional untracked build inputs, with ignore rules and additional sensitive/generated-file exclusions. Packaging requires a Git checkout without local `.env` production build inputs; symlinks, junctions and aliased source files are rejected. Relevant source contents, file identities and timestamps must remain stable through archiving and compilation; changes abort packaging. A dirty checkout is allowed and its actual files, not HEAD, are archived. All three application version declarations must agree. The release is unsigned, so Windows may show SmartScreen warnings.

Internal-test packages use the separate `org.klipper.desktop.packaging-test` identifier and **Klipper Internal Test** installation name. Their library/settings are separate from normal Klipper. This tests the packaged application without touching a personal library.

Extract the entire ZIP and run `klipper.exe`; Node.js, Rust, Python and developer media tools are not needed. Windows 10/11 x64, WebView2 and supported graphics drivers remain prerequisites. The installer offers WebView2 installation if missing (Internet access required); the ZIP requires an existing WebView2 runtime. Settings, library and cache continue using Windows application-data directories. Uninstalling does not delete source recordings; unfinished exports are cancelled when Klipper closes.

`dist:licenses` collects checksum-verified upstream source archives, locked Rust/frontend sources and verbatim notices into ignored `native-deps/compliance-bundle/`. Run `dist:prepare` first and fetch the locked Cargo sources as shown above; collection downloads any missing pinned source archives. An inventory tracks collector-owned artifacts: obsolete unchanged generated files are removed on refresh, while unknown or modified files stop refresh for review. Both distributions include these materials and Klipper's application-source snapshot. Internal verification checks reject missing/tampered files, changed lockfiles, binary pins, compiler or installer inputs; rerun collection after relevant changes. `dist:licenses:check` without `--internal-test` also requires completed public-release approval.

**Public redistribution remains blocked.** Exact static dependency sources/build inputs for libmpv and Gyan FFmpeg, installer binary/source correspondence and the final combined-work review remain unresolved. Collected top-level sources and generic project links do not satisfy those obligations. Exact MIT plugin lockfiles and byte-identical rebuilds are not legal prerequisites; the manifest distinguishes license obligations, missing evidence and internal verification policies. See [`packaging/compliance.json`](packaging/compliance.json) and [`packaging/THIRD-PARTY-NOTICES.txt`](packaging/THIRD-PARTY-NOTICES.txt). `npm run dist:windows` refuses a public build; `--internal-test` produces private validation artifacts only. Clean-machine installation/runtime testing is also required before a public alpha.

## License

Klipper's application code is licensed under **[GPL-3.0-only](LICENSE)**. Third-party media libraries and tools retain their respective licenses and redistribution requirements.
