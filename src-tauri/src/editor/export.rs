use super::{media, Project, Status};
use crate::{library::files, media_process::read_bounded};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
};

#[derive(Clone)]
pub struct Destination {
    pub recovery_directory: Option<PathBuf>,
    pub path: PathBuf,
    parent: PathBuf,
    identity: Option<String>,
}
impl Destination {
    pub fn new(path: &Path) -> Result<Self, String> {
        let parent = files::local_directory(path.parent().ok_or("Invalid output folder")?)?;
        let name = path.file_name().ok_or("Choose a new MP4 filename")?;
        let filename = name.to_str().ok_or("Unsupported output filename")?;
        let stem = filename
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if filename
            .chars()
            .any(|c| c.is_control() || ":<>\"|?*".contains(c))
            || filename.ends_with([' ', '.'])
            || [
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ]
            .contains(&stem.as_str())
        {
            return Err("Choose a normal new MP4 filename, without device names, alternate data streams or reserved characters.".into());
        }
        let path = parent.join(name);
        if path
            .extension()
            .and_then(|v| v.to_str())
            .is_none_or(|v| !v.eq_ignore_ascii_case("mp4"))
        {
            return Err("Choose an .mp4 output filename.".into());
        }
        if path
            .try_exists()
            .map_err(|_| "Could not check the destination")?
            || fs::symlink_metadata(&path).is_ok()
        {
            return Err("The output already exists. Choose a new filename; existing files are never replaced.".into());
        }
        Ok(Self {
            recovery_directory: None,
            path,
            identity: files::identity(&parent),
            parent,
        })
    }
    pub fn verify(&self) -> Result<(), String> {
        if files::local_directory(&self.parent)? != self.parent
            || files::identity(&self.parent) != self.identity
        {
            return Err("The output folder changed during export.".into());
        }
        if fs::symlink_metadata(&self.path).is_ok() {
            return Err("The output filename is now occupied; it was not overwritten.".into());
        }
        Ok(())
    }
}

struct Temporary {
    recovery: Option<super::recovery::Recovery>,
    path: PathBuf,
    identity: Option<String>,
    file: Option<File>,
}
impl Temporary {
    fn new(destination: &Destination) -> Result<Self, String> {
        destination.verify()?;
        for _ in 0..10 {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let path = destination.parent.join(format!(
                ".klipper-export-{}-{nonce}.mp4",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).read(true).create_new(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // Keep replacement/deletion blocked while FFmpeg opens and truncates our file.
                options.share_mode(3);
            }
            match options.open(&path) {
                Ok(file) => return Ok(Self { identity: files::identity(&path), path, file: Some(file), recovery: None }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err("Could not create temporary output. Check folder permissions and free disk space.".into()),
            }
        }
        Err("Could not reserve a unique temporary output.".into())
    }
    fn publish(mut self, destination: &Destination) -> Result<(), String> {
        self.file
            .take()
            .ok_or("Temporary output handle unavailable")?
            .sync_all()
            .map_err(|_| "Could not flush the exported video")?;
        destination.verify()?;
        if files::identity(&self.path) != self.identity {
            return Err("Temporary output identity changed.".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let from: Vec<u16> = self.path.as_os_str().encode_wide().chain([0]).collect();
            let to: Vec<u16> = destination
                .path
                .as_os_str()
                .encode_wide()
                .chain([0])
                .collect();
            // No REPLACE_EXISTING: publication fails atomically if the final path exists.
            if unsafe {
                windows_sys::Win32::Storage::FileSystem::MoveFileExW(from.as_ptr(), to.as_ptr(), 0)
            } == 0
            {
                return Err("Could not publish the export. The destination may now exist or be inaccessible.".into());
            }
        }
        #[cfg(not(windows))]
        {
            fs::hard_link(&self.path, &destination.path).map_err(|_| {
                "Could not publish without overwriting. Choose a filesystem supporting hard links."
            })?;
        }
        Ok(())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        self.file.take();
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            && files::identity(&self.path) == self.identity
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn arguments(source: &Path, output: &Path, project: &Project) -> Vec<std::ffi::OsString> {
    let info = &project.info;
    let start = info.origin + project.start;
    let end = info.origin + project.end;
    let quality = project.output.crf.to_string();
    // Boundary resolution and FFmpeg filtering share the source's rational video clock.
    let tick = media::time_base(&info.time_base).expect("validated editor time base");
    let first = (start / tick).round() as i64;
    let last = (end / tick).round() as i64;
    let mut graph = format!(
        "[0:{}]trim=start_pts={first}:end_pts={last},setpts=PTS-{first}[v]",
        info.video_index
    );
    for (n, track) in info.audio.iter().enumerate() {
        // Subtract the SAME origin on every stream, retaining genuine delayed audio.
        graph.push_str(&format!(
            ";[0:{}]atrim=start={start:.6}:end={end:.6},asetpts=PTS-({start:.6})/TB[a{n}]",
            track.index
        ));
    }
    let mut args: Vec<std::ffi::OsString> = [
        "-v",
        "error",
        "-nostdin",
        "-y",
        "-copyts",
        "-noautorotate",
        "-threads",
        "2",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        "mov,matroska,webm",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    // Seek to a preceding keyframe, preserving its original timestamps. Our explicit
    // PTS/sample trim filters perform the accurate cut; do not let FFmpeg also trim
    // preroll using its relative seek clock (notably on nonzero-start files).
    args.extend([
        "-seek_timestamp".into(),
        "1".into(),
        "-ss".into(),
        format!("{start:.9}").into(),
        "-noaccurate_seek".into(),
        "-i".into(),
    ]);
    args.push(source.into());
    let nvenc = matches!(project.output.encoder, super::encoders::Encoder::Nvenc);
    let encoder = if nvenc { "h264_nvenc" } else { "libx264" };
    let preset = if nvenc { "p5" } else { "veryfast" };
    for s in [
        "-filter_complex_threads",
        "1",
        "-filter_complex",
        &graph,
        "-map",
        "[v]",
        "-c:v",
        encoder,
        "-preset",
        preset,
        "-pix_fmt",
        "yuv420p",
        "-fps_mode:v",
        "passthrough",
        "-enc_time_base:v",
        &info.time_base,
        "-threads:v",
        "2",
    ] {
        args.push(s.into());
    }
    if nvenc {
        args.extend(
            [
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                "19",
                "-b:v",
                "0",
                "-spatial-aq",
                "1",
                "-temporal-aq",
                "1",
                "-bf",
                "0",
            ]
            .into_iter()
            .map(Into::into),
        );
    } else {
        args.extend(["-crf".into(), quality.into()]);
    }
    for (n, track) in info.audio.iter().enumerate() {
        args.extend([
            "-map".into(),
            format!("[a{n}]").into(),
            format!("-c:a:{n}").into(),
            "aac".into(),
            format!("-b:a:{n}").into(),
            "192k".into(),
            format!("-ar:a:{n}").into(),
            track.sample_rate.to_string().into(),
        ]);
    }
    args.extend(
        ["-map_metadata", "-1", "-map_chapters", "-1"]
            .into_iter()
            .map(Into::into),
    );
    for (n, track) in info.audio.iter().enumerate() {
        for (key, value) in [("title", &track.title), ("language", &track.language)] {
            if let Some(value) = value
                .as_ref()
                .filter(|v| v.len() <= 256 && !v.contains('\0'))
            {
                args.extend([
                    format!("-metadata:s:a:{n}").into(),
                    format!("{key}={value}").into(),
                ]);
            }
        }
    }
    for (key, value) in [
        ("-colorspace", &info.color_space),
        ("-color_trc", &info.color_transfer),
        ("-color_primaries", &info.color_primaries),
        ("-color_range", &info.color_range),
    ] {
        if let Some(value) = value {
            args.extend([key.into(), value.into()]);
        }
    }
    // x264 may omit an explicitly limited range when no other VUI color fields exist.
    // Preserve the source's explicit flag in the resulting H.264 bitstream as well.
    if let Some(range) = info
        .color_range
        .as_deref()
        .filter(|v| ["tv", "pc"].contains(v))
    {
        args.extend([
            "-bsf:v".into(),
            format!(
                "h264_metadata=video_full_range_flag={}",
                if range == "pc" { 1 } else { 0 }
            )
            .into(),
        ]);
    }
    args.extend(
        [
            "-movflags",
            "+faststart",
            "-avoid_negative_ts",
            "disabled",
            "-progress",
            "pipe:1",
            "-stats_period",
            "0.25",
            "-nostats",
            "-f",
            "mp4",
        ]
        .into_iter()
        .map(Into::into),
    );
    args.push(output.into());
    args
}

pub async fn run(
    executable: &Path,
    probe: &Path,
    source: &files::File,
    destination: Destination,
    project: &Project,
    status: &Arc<Mutex<Status>>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    if *cancel.borrow() {
        return Err("Export cancelled".into());
    }
    if files::describe(Path::new(&source.path))? != *source {
        return Err("The source changed. Reopen the editor.".into());
    }
    for track in &project.info.audio {
        if track.start >= project.end
            || track
                .duration
                .is_some_and(|d| track.start + d <= project.start)
        {
            return Err("An audio stream has no samples in the selected interval. Phase 1 cannot preserve an empty audio stream; choose a wider range.".into());
        }
    }
    let mut temporary = Temporary::new(&destination)?;
    if let Some(directory) = &destination.recovery_directory {
        let identity=temporary.identity.as_deref().ok_or("This filesystem does not provide the stable identity required for safe export recovery")?;
        temporary.recovery = Some(super::recovery::Recovery::create(
            directory,
            &temporary.path,
            identity,
        )?);
    }
    let args = arguments(Path::new(&source.path), &temporary.path, project);
    let mut command = Command::new(executable);
    crate::config::secure_media_command(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000 | 0x00004000);
    let mut child = command
        .spawn()
        .map_err(|_| crate::config::media_start_error("FFmpeg"))?;
    status.lock().unwrap().phase = "encoding".into();
    #[cfg(test)]
    {
        status.lock().unwrap().process_id = child.id();
    }
    let started = Instant::now();
    let result = {
        let stdout = child.stdout.take().ok_or("No export progress pipe")?;
        let stderr = child.stderr.take().ok_or("No export diagnostic pipe")?;
        let work = async {
            tokio::try_join!(
                async {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Some(line) = lines
                        .next_line()
                        .await
                        .map_err(|_| "Could not read export progress".to_string())?
                    {
                        if line.len() > 4096 {
                            return Err("Export progress exceeded its limit".to_string());
                        }
                        if let Some(value) = line
                            .strip_prefix("out_time_us=")
                            .and_then(|v| v.parse::<f64>().ok())
                            .filter(|v| v.is_finite())
                        {
                            let elapsed = started.elapsed().as_secs_f64();
                            let seconds =
                                (value / 1_000_000.0).clamp(0.0, project.end - project.start);
                            let mut progress = status.lock().unwrap();
                            progress.seconds = seconds;
                            progress.eta = if elapsed > 1.0 && seconds > 0.1 {
                                Some((project.end - project.start - seconds) * elapsed / seconds)
                            } else {
                                None
                            };
                        }
                    }
                    Ok(())
                },
                read_bounded(stderr, 256 * 1024),
                async {
                    child
                        .wait()
                        .await
                        .map_err(|_| "Could not wait for FFmpeg".to_string())
                }
            )
        };
        tokio::select! {
            biased;
            _=cancel.wait_for(|v|*v)=>Err("Export cancelled".to_string()),
            result=tokio::time::timeout(Duration::from_secs(12*60*60),work)=>result.map_err(|_|"Export exceeded its 12-hour limit".to_string()).and_then(|v|v).and_then(|(_,_,s)|if s.success(){Ok(())}else{Err(if project.output.encoder == super::encoders::Encoder::Nvenc { "FFmpeg export failed using NVIDIA NVENC. Check free disk space and GPU/driver availability, or choose CPU encoding. No destination file was published." } else { "FFmpeg export failed. Check free disk space and that this FFmpeg build supports libx264/AAC. No destination file was published." }.into())}),
        }
    };
    if result.is_err() {
        let _ = child.start_kill();
        child.wait().await.map_err(|_| "Could not reap FFmpeg")?;
    }
    result?;
    if *cancel.borrow() {
        return Err("Export cancelled".into());
    }
    {
        let mut progress = status.lock().unwrap();
        progress.phase = "verifying".into();
        progress.seconds = project.end - project.start;
    }
    let exported = media::inspect(probe, &temporary.path, cancel).await?;
    let expected = project.end - project.start;
    if exported.codec != "h264"
        || exported.width != project.info.width
        || exported.height != project.info.height
        || project
            .info
            .sample_aspect_ratio
            .as_ref()
            .is_some_and(|v| exported.sample_aspect_ratio.as_ref() != Some(v))
        || project
            .info
            .color_space
            .as_ref()
            .is_some_and(|v| exported.color_space.as_ref() != Some(v))
        || project
            .info
            .color_transfer
            .as_ref()
            .is_some_and(|v| exported.color_transfer.as_ref() != Some(v))
        || project
            .info
            .color_primaries
            .as_ref()
            .is_some_and(|v| exported.color_primaries.as_ref() != Some(v))
        || project
            .info
            .color_range
            .as_ref()
            .is_some_and(|v| exported.color_range.as_ref() != Some(v))
        || exported.audio.len() != project.info.audio.len()
        || (exported.duration - expected).abs() > 0.05
    {
        #[cfg(test)]
        eprintln!(
            "Source technical properties: {:?}\nOutput technical properties: {:?}",
            project.info, exported
        );
        return Err("Export validation failed (timing, dimensions, color, aspect ratio or streams). No destination file was published.".into());
    }
    for (actual, wanted) in exported.audio.iter().zip(&project.info.audio) {
        if actual.channels != wanted.channels
            || actual.sample_rate != wanted.sample_rate
            || actual.codec != "aac"
            || actual.duration.is_none_or(|d| d <= 0.0)
        {
            return Err(
                "Exported audio stream validation failed. No destination file was published."
                    .into(),
            );
        }
    }
    // Decode the complete output before publication; corrupt/truncated packets are not success.
    let mut verify = Command::new(executable);
    verify
        .args([
            "-v",
            "error",
            "-xerror",
            "-nostdin",
            "-threads",
            "2",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(&temporary.path)
        .args(["-map", "0:v", "-map", "0:a?", "-f", "null", "-"]);
    let check = crate::media_process::capture(
        &mut verify,
        cancel,
        Duration::from_secs(12 * 60 * 60),
        1024,
        "FFmpeg",
        true,
    )
    .await?
    .ok_or("Export cancelled")?;
    if !check.1.success() {
        return Err("The exported file failed its decoding check; it was not published.".into());
    }
    if *cancel.borrow() {
        return Err("Export cancelled".into());
    }
    if files::describe(Path::new(&source.path))? != *source {
        return Err("The source changed during export. No output was published.".into());
    }
    temporary.publish(&destination)
}
