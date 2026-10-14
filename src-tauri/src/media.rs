use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    index: u32,
    codec: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    container: Option<String>,
    video_codec: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    frame_rate: Option<f64>,
    duration: Option<f64>,
    file_size: u64,
    bitrate: Option<f64>,
    audio: Vec<AudioStream>,
}

#[derive(Default, Deserialize)]
struct Format {
    format_name: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Default, Deserialize)]
struct Stream {
    #[serde(default)]
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    disposition: BTreeMap<String, i32>,
}

#[derive(Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    format: Format,
    #[serde(default)]
    streams: Vec<Stream>,
}

fn number(value: Option<&str>) -> Option<f64> {
    value?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn frame_rate(value: Option<&str>) -> Option<f64> {
    let value = value?;
    let result = if let Some((top, bottom)) = value.split_once('/') {
        number(Some(top))? / number(Some(bottom))?
    } else {
        number(Some(value))?
    };
    (result.is_finite() && result > 0.0).then_some(result)
}

pub fn parse(bytes: &[u8], file_size: u64) -> Result<MediaInfo, String> {
    let output: ProbeOutput = serde_json::from_slice(bytes)
        .map_err(|_| "FFprobe returned invalid metadata.".to_string())?;
    let video = output.streams.iter().find(|s| {
        s.codec_type.as_deref() == Some("video") && s.disposition.get("attached_pic") != Some(&1)
    });
    let info = MediaInfo {
        container: output.format.format_name,
        video_codec: video.and_then(|s| s.codec_name.clone()),
        width: video.and_then(|s| s.width),
        height: video.and_then(|s| s.height),
        frame_rate: video.and_then(|s| {
            frame_rate(s.avg_frame_rate.as_deref())
                .or_else(|| frame_rate(s.r_frame_rate.as_deref()))
        }),
        duration: number(output.format.duration.as_deref())
            .or_else(|| video.and_then(|s| number(s.duration.as_deref()))),
        file_size,
        bitrate: number(output.format.bit_rate.as_deref()),
        audio: output
            .streams
            .into_iter()
            .filter(|s| s.codec_type.as_deref() == Some("audio"))
            .map(|s| AudioStream {
                index: s.index,
                codec: s.codec_name,
                channels: s.channels,
                channel_layout: s.channel_layout,
                tags: s.tags,
            })
            .collect(),
    };
    Ok(info)
}

async fn read_bounded(reader: impl AsyncRead + Unpin, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Could not read FFprobe output.".to_string())?;
    if bytes.len() as u64 > limit {
        return Err("FFprobe metadata exceeded the safety limit.".into());
    }
    Ok(bytes)
}

pub async fn probe(executable: &Path, path: &Path) -> Result<MediaInfo, String> {
    let file_size = tokio::fs::metadata(path)
        .await
        .map_err(|_| "The selected recording is no longer readable.".to_string())?
        .len();
    let mut command = Command::new(executable);
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
            "-format_whitelist",
            "mov,matroska,webm",
            "-probesize",
            "10000000",
            "-analyzeduration",
            "10000000",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            "-i",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let mut child = command.spawn().map_err(|_| "FFprobe could not start. Install FFmpeg with ffprobe on PATH, then restart Klipper, or provide the portable helper in resources/bin.".to_string())?;
    let stdout = child.stdout.take().ok_or("FFprobe output unavailable.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("FFprobe error output unavailable.")?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::try_join!(
            read_bounded(stdout, 4 * 1024 * 1024),
            read_bounded(stderr, 64 * 1024),
            async {
                child
                    .wait()
                    .await
                    .map_err(|_| "FFprobe process failed.".to_string())
            }
        )
    })
    .await;
    match result {
        Ok(Ok((stdout, _stderr, status))) if status.success() => parse(&stdout, file_size),
        Ok(Ok(_)) => Err("FFprobe could not analyze this recording. The file may be damaged, unsupported or inaccessible. Playback can still be attempted.".into()),
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            match other {
                Ok(Err(error)) => Err(error),
                _ => Err("FFprobe analysis timed out after 30 seconds. Try another recording.".into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_four_audio_streams_without_claiming_mixing() {
        let streams: Vec<_> = (1..=4).map(|index| serde_json::json!({"index": index, "codec_type": "audio", "codec_name": "aac", "channels": 2, "tags": {"language":"eng", "title":format!("Synthetic track {index}")}})).collect();
        let mut data = serde_json::json!({"format":{"format_name":"mov,mp4", "duration":"3.5"},"streams":streams});
        data["streams"].as_array_mut().unwrap().push(serde_json::json!({"codec_type":"video","codec_name":"hevc","width":1920,"height":1080,"avg_frame_rate":"60000/1001"}));
        let info = parse(&serde_json::to_vec(&data).unwrap(), 1024).unwrap();
        assert_eq!(info.audio.len(), 4);
        assert_eq!(info.audio[3].tags["language"], "eng");
        assert_eq!(info.video_codec.as_deref(), Some("hevc"));
        assert!((info.frame_rate.unwrap() - 59.94006).abs() < 0.0001);
    }
    #[test]
    fn tolerates_missing_fields_and_invalid_rates() {
        let info = parse(
            br#"{"format":{"duration":"N/A","bit_rate":"NaN"},"streams":[]}"#,
            0,
        )
        .unwrap();
        assert!(info.duration.is_none());
        assert!(info.bitrate.is_none());
        for value in ["0/0", "1/0", "-1", "NaN", "N/A"] {
            assert!(frame_rate(Some(value)).is_none());
        }
        assert!(parse(b"not json", 0).is_err());
    }
    #[test]
    fn ignores_cover_art_and_falls_back_to_video_duration() {
        let info = parse(br#"{"streams":[{"codec_type":"video","codec_name":"png","disposition":{"attached_pic":1}},{"codec_type":"video","codec_name":"h264","duration":"2","avg_frame_rate":"0/0","r_frame_rate":"30/1"}]}"#, 1).unwrap();
        assert_eq!(info.video_codec.as_deref(), Some("h264"));
        assert_eq!(info.frame_rate, Some(30.0));
        assert_eq!(info.duration, Some(2.0));
    }
    #[tokio::test]
    async fn limits_process_output() {
        assert!(read_bounded(&b"12345"[..], 4).await.is_err());
        assert_eq!(read_bounded(&b"1234"[..], 4).await.unwrap(), b"1234");
    }
    #[tokio::test]
    async fn missing_helper_is_actionable() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("synthetic.mp4");
        std::fs::write(&file, b"fixture").unwrap();
        let error = probe(&directory.path().join("missing-ffprobe"), &file)
            .await
            .unwrap_err();
        assert!(error.contains("PATH"));
    }
}
