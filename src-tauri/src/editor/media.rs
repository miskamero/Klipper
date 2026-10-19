//! Editor-specific inspection. Library metadata remains compatible with existing cache payloads.
use crate::media_process::capture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::{process::Command, sync::watch};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub index: u32,
    pub codec: String,
    pub channels: u32,
    pub sample_rate: u32,
    pub title: Option<String>,
    pub language: Option<String>,
    pub start: f64,
    pub duration: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub duration: f64,
    pub origin: f64,
    pub video_index: u32,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub time_base: String,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub color_range: Option<String>,
    pub sample_aspect_ratio: Option<String>,
    pub audio: Vec<Track>,
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
pub fn time_base(value: &str) -> Result<f64, String> {
    let (n, d) = value.split_once('/').ok_or("Invalid video time base")?;
    let n = n.parse::<f64>().map_err(|_| "Invalid video time base")?;
    let d = d.parse::<f64>().map_err(|_| "Invalid video time base")?;
    if n <= 0.0 || d <= 0.0 || !n.is_finite() || !d.is_finite() {
        return Err("Invalid video time base".into());
    }
    Ok(n / d)
}
fn text(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| !s.is_empty() && *s != "unknown")
        .map(str::to_owned)
}
fn index(v: &Value) -> Result<u32, String> {
    v.as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or("Invalid media stream index".into())
}

pub fn parse(bytes: &[u8]) -> Result<Info, String> {
    let data: Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid editor inspection output")?;
    let streams = data["streams"].as_array().ok_or("No media streams found")?;
    let videos: Vec<_> = streams
        .iter()
        .filter(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .collect();
    if videos.len() != 1 {
        return Err("Phase 1 exports require exactly one video stream; additional video streams cannot be silently dropped.".into());
    }
    let v = videos[0];
    if streams
        .iter()
        .any(|s| s["disposition"]["attached_pic"] == 1)
        || data["chapters"].as_array().is_some_and(|v| !v.is_empty())
    {
        return Err(
            "Phase 1 cannot preserve attached artwork or chapters during trimming yet.".into(),
        );
    }
    let codec = text(&v["codec_name"]).ok_or("Unknown video codec")?;
    if !["h264", "hevc"].contains(&codec.as_str()) {
        return Err("The editor currently exports H.264 and HEVC inputs only.".into());
    }
    if v["pix_fmt"] != "yuv420p"
        || ["smpte2084", "arib-std-b67"].contains(&v["color_transfer"].as_str().unwrap_or(""))
        || v["side_data_list"].as_array().is_some_and(|list| {
            list.iter().any(|s| {
                s["side_data_type"].as_str().is_some_and(|t| {
                    t.contains("Mastering") || t.contains("Content light") || t.contains("DOVI")
                })
            })
        })
    {
        return Err("Phase 1 supports 8-bit SDR 4:2:0 video only. HDR, Dolby Vision, 10-bit and other pixel formats need a verified color-preserving export path.".into());
    }
    if v["side_data_list"]
        .as_array()
        .is_some_and(|list| list.iter().any(|s| s["side_data_type"] == "Display Matrix"))
        || v["tags"]["rotate"].as_str().is_some_and(|v| v != "0")
    {
        return Err("Rotated/display-matrix video is not yet supported for export.".into());
    }
    if !["progressive", "unknown", ""].contains(&v["field_order"].as_str().unwrap_or("")) {
        return Err("Interlaced video is not supported for export yet.".into());
    }
    if streams
        .iter()
        .any(|s| !["video", "audio"].contains(&s["codec_type"].as_str().unwrap_or("")))
    {
        return Err("This file contains subtitle/data/attachment streams. Phase 1 cannot preserve them yet.".into());
    }
    let width = index(&v["width"])?;
    let height = index(&v["height"])?;
    if width == 0
        || height == 0
        || width % 2 != 0
        || height % 2 != 0
        || width > 8192
        || height > 8192
    {
        return Err("Unsupported video dimensions for H.264 4:2:0 export.".into());
    }
    let origin = num(&data["format"]["start_time"])
        .or_else(|| num(&v["start_time"]))
        .ok_or("Missing source timeline origin")?;
    let video_start = num(&v["start_time"]).unwrap_or(origin);
    let mut duration = num(&v["duration"])
        .map(|d| video_start + d - origin)
        .or_else(|| num(&data["format"]["duration"]))
        .filter(|d| *d > 0.0 && *d <= 604800.0)
        .ok_or("Missing or unsupported video duration")?;
    let audio: Vec<Track> = streams.iter().filter(|s| s["codec_type"] == "audio").map(|s| {
        let codec = text(&s["codec_name"]).ok_or("Unknown audio codec")?;
        let channels = index(&s["channels"])?;
        if !["aac", "mp3", "opus", "vorbis", "flac", "pcm_s16le", "pcm_s24le", "pcm_f32le"].contains(&codec.as_str()) || !(1..=2).contains(&channels) {
            return Err("Phase 1 can preserve separate mono/stereo AAC, MP3, Opus, Vorbis, FLAC or PCM tracks. This file has an unsupported audio stream; no tracks will be silently removed.".into());
        }
        let rate = s["sample_rate"].as_str().and_then(|v| v.parse::<u32>().ok()).filter(|v| [8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000].contains(v)).ok_or("Unsupported audio sample rate")?;
        Ok(Track { index: index(&s["index"])?, codec, channels, sample_rate: rate,
            title: text(&s["tags"]["title"]), language: text(&s["tags"]["language"]),
            start: num(&s["start_time"]).unwrap_or(origin) - origin, duration: num(&s["duration"]) })
    }).collect::<Result<_, String>>()?;
    if audio.len() > 16 {
        return Err("Too many audio streams for this editor milestone.".into());
    }
    // VFR video stream duration can end before the final audio sample. The recording
    // timeline includes every inspected stream, retaining a final held video frame.
    for track in &audio {
        if let Some(end) = track
            .duration
            .map(|d| track.start + d)
            .filter(|d| d.is_finite() && *d <= 604800.0)
        {
            duration = duration.max(end);
        }
    }
    let time_base = text(&v["time_base"]).ok_or("Missing video time base")?;
    self::time_base(&time_base)?;
    Ok(Info {
        duration,
        origin,
        video_index: index(&v["index"])?,
        codec,
        width,
        height,
        time_base,
        color_space: text(&v["color_space"]),
        color_transfer: text(&v["color_transfer"]),
        color_primaries: text(&v["color_primaries"]),
        color_range: text(&v["color_range"]),
        sample_aspect_ratio: text(&v["sample_aspect_ratio"]),
        audio,
    })
}

pub async fn inspect(
    exe: &Path,
    source: &Path,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Info, String> {
    let mut command = Command::new(exe);
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
            "-format_whitelist",
            "mov,matroska,webm",
            "-show_streams",
            "-show_chapters",
            "-show_format",
            "-of",
            "json",
            "-i",
        ])
        .arg(source);
    let (bytes, status) = capture(
        &mut command,
        cancel,
        Duration::from_secs(30),
        4 * 1024 * 1024,
        "FFprobe",
        false,
    )
    .await?
    .ok_or("Editor operation cancelled")?;
    if !status.success() {
        return Err("FFprobe could not inspect the editor source.".into());
    }
    parse(&bytes)
}

// Decode only a bounded neighborhood, including preceding keyframes. Do not infer frame times
// from average FPS. The requested boundary snaps forward to an actual presentation timestamp.
pub async fn boundary(
    exe: &Path,
    source: &Path,
    info: &Info,
    seconds: f64,
    cancel: &mut watch::Receiver<bool>,
) -> Result<f64, String> {
    if (seconds - info.duration).abs() < 0.000001 {
        return Ok(info.duration);
    }
    let mut command = Command::new(exe);
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
            "-format_whitelist",
            "mov,matroska,webm",
            "-select_streams",
        ])
        .arg(info.video_index.to_string())
        .arg("-read_intervals")
        .arg(format!(
            "{:.6}%{:.6}",
            info.origin + (seconds - 1.0).max(0.0),
            info.origin + seconds + 2.0
        ))
        .args([
            "-show_frames",
            "-show_entries",
            "frame=best_effort_timestamp,best_effort_timestamp_time",
            "-of",
            "json",
            "-i",
        ])
        .arg(source);
    let (bytes, status) = capture(
        &mut command,
        cancel,
        Duration::from_secs(15),
        4 * 1024 * 1024,
        "FFprobe",
        true,
    )
    .await?
    .ok_or("Editor operation cancelled")?;
    if !status.success() {
        return Err("Could not resolve the video frame boundary.".into());
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid frame timing output")?;
    let tick = time_base(&info.time_base)?;
    let frames = data["frames"]
        .as_array()
        .ok_or("Missing frame timing output")?;
    let boundary = data["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            f["best_effort_timestamp"]
                .as_i64()
                .map(|pts| pts as f64 * tick)
                .or_else(|| num(&f["best_effort_timestamp_time"]))
        })
        .map(|n| n - info.origin)
        .filter(|n| *n + 0.000001 >= seconds && *n < info.duration)
        .min_by(f64::total_cmp);
    // The terminal boundary is also valid when the bounded read reached EOF beyond
    // the last presentation timestamp (including an audio tail / held video frame).
    boundary
        .or_else(|| (!frames.is_empty() && seconds + 2.0 >= info.duration).then_some(info.duration))
        .ok_or("No frame found near this boundary. Try a nearby position.".into())
}
