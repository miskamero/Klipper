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
- The private corrected **KlinoffBuilds FFmpeg 9.0.2 encoder and libmpv 0.41.0 RC1 packaging2** binary and matching source ZIPs.
- The Windows x64 **libmpv** build pinned in [`scripts/libmpv.json`](scripts/libmpv.json).

See [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows) for help setting up the development environment.

### Run from Source

1. Clone the repository and install its dependencies:

    ```powershell
    git clone https://github.com/miskamero/Klipper.git
    cd Klipper
    npm ci
    ```

2. Register the directory containing both corrected binary ZIPs and their matching `-source.zip` files:

    ```powershell
    npm.cmd run native:setup -- "F:\ffmpeg-windows\work\release-packages-rc1-corrected"
    npm run native:check
    ```

3. Start Klipper:

    ```powershell
    npm run desktop
    ```

Setup verifies archive and runtime SHA-256 pins, extracts only required executables/DLLs into ignored `native-deps/windows-x64-klinoff-rc1/`, and retains the source ZIPs for packaging. The previous `native-deps/windows-x64/` files are preserved. FFmpeg uses its own x264/x265 and GCC runtime DLLs; libmpv uses its five companion DLLs and the pinned app-local Vulkan loader. Setup may download that loader if uncached; it installs no system software. The separate FFmpeg playback SDK is unnecessary because libmpv includes its playback libraries statically. Windows development uses these pinned paths with no PATH media-tool fallback.

If Rust linking fails, try the **x64 Developer PowerShell for Visual Studio**.

The encoder build omits PNG decoding and font rendering. Imported PNG previews use a bounded Rust decoder and an automatically removed temporary PPM; FFmpeg still scales and encodes the cached JPEG. Synthetic export tests use a changing pixel pattern without system fonts.

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

To compile an executable without packaging, run `npm run desktop:build`. A release executable requires libmpv, its companion DLLs and the Vulkan loader beside it, plus FFmpeg/FFprobe and their companion DLLs in `bin/`. Use the packaging workflow to assemble this complete layout. Release builds do not fall back to system PATH tools.

### Windows x64 packages

From an x64 Visual Studio developer terminal, after registering the corrected packages as above:

```powershell
npm run dist:prepare
npm run dist:check
cargo fetch --locked --manifest-path src-tauri/Cargo.toml
npm run dist:licenses
npm.cmd run dist:licenses:check -- --internal-test
npm.cmd run dist:windows -- --internal-test
```

`dist:prepare` reuses the locally registered KlinoffBuilds archives, prepares the pinned Vulkan loader and provisions the pinned NSIS toolset in `src-tauri/target/.tauri/`. It also accepts `-- <package-directory>` to register packages directly. No existing per-user NSIS cache is needed. The distribution overlay enables Tauri's `useLocalToolsDir` option. Tools are authenticated before use; every runtime DLL's imports must resolve within its packaged directory or Windows system libraries. The installer inventory is checked before and after packaging. Windows' built-in `tar.exe` extracts explicit runtime members. No system software is installed.

Packaging produces an application folder, ZIP, per-user NSIS installer, application-source snapshot and `SHA256SUMS.txt` under `release/windows-x64/<version>-internal-test-<unique suffix>/`. Only explicitly selected application resources are bundled. Application sources are selected from Git-listed maintained paths, including intentional untracked build inputs, with ignore rules and additional sensitive/generated-file exclusions. Packaging requires a Git checkout without local `.env` production build inputs; symlinks, junctions and aliased source files are rejected. Relevant source contents, file identities and timestamps must remain stable through archiving and compilation; changes abort packaging. A dirty checkout is allowed and its actual files, not HEAD, are archived. All three application version declarations must agree. The release is unsigned, so Windows may show SmartScreen warnings.

Internal-test packages use the separate `org.klipper.desktop.packaging-test` identifier and **Klipper Internal Test** installation name. Their library/settings are separate from normal Klipper. This tests the packaged application without touching a personal library.

Extract the entire ZIP and run `klipper.exe`; Node.js, Rust, Python and developer media tools are not needed. Windows 10/11 x64, WebView2 and supported graphics drivers remain prerequisites. The installer offers WebView2 installation if missing (Internet access required); the ZIP requires an existing WebView2 runtime. Settings, library and cache continue using Windows application-data directories. Uninstalling does not delete source recordings; unfinished exports are cancelled when Klipper closes.

`dist:licenses` retains both corrected KlinoffBuilds binary/source archive pairs, extracts all their component notices, and collects checksum-verified upstream source archives, locked Rust/frontend sources and verbatim notices into ignored `native-deps/compliance-bundle-klinoff-rc1/`. Run `dist:prepare` first and fetch the locked Cargo sources as shown above; collection downloads any missing pinned source archives. An inventory tracks collector-owned artifacts: obsolete unchanged generated files are removed on refresh, while unknown or modified files stop refresh for review. Both distributions include these materials and Klipper's application-source snapshot. Internal verification checks reject missing/tampered files, changed lockfiles, binary pins, compiler or installer inputs; rerun collection after relevant changes. `dist:licenses:check` without `--internal-test` also requires completed public-release approval.

**Public redistribution remains blocked.** The corrected KlinoffBuilds corresponding-source bundles and notices are retained with their binaries; final combined-work review and installer binary/source correspondence remain unresolved. Repositories and assets remain private. See [`packaging/compliance.json`](packaging/compliance.json) and [`packaging/THIRD-PARTY-NOTICES.txt`](packaging/THIRD-PARTY-NOTICES.txt). `npm run dist:windows` refuses a public build; `--internal-test` produces private validation artifacts only. Clean-machine installation/runtime testing is required before any public release.

## License

Klipper's application code is licensed under **[GPL-3.0-only](LICENSE)**. Third-party media libraries and tools retain their respective licenses and redistribution requirements.
