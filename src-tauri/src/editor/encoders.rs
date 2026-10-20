use crate::media_process::capture;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use tokio::{process::Command, sync::watch};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Encoder {
    #[default]
    Auto,
    Cpu,
    Nvenc,
}

#[derive(Clone, Serialize)]
pub struct Support {
    pub nvenc: bool,
    pub detail: String,
}

pub async fn detect(executable: &Path, cancel: &mut watch::Receiver<bool>) -> Support {
    // An encoder listing does not prove the driver/device can open an encoding session.
    // Use the same quality options as export, with a tiny generated input and no file output.
    let mut command = Command::new(executable);
    command.args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=30",
        "-frames:v",
        "6",
        "-an",
        "-c:v",
        "h264_nvenc",
        "-preset",
        "p5",
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
        "-f",
        "null",
        "-",
    ]);
    let nvenc = capture(
        &mut command,
        cancel,
        Duration::from_secs(10),
        1024,
        "FFmpeg",
        true,
    )
    .await
    .ok()
    .flatten()
    .is_some_and(|(_, status)| status.success());
    Support { nvenc, detail: if nvenc { "NVIDIA H.264 encoding verified with a synthetic encode." } else { "NVIDIA encoding unavailable; CPU H.264 remains available. Check the FFmpeg build and NVIDIA driver if expected." }.into() }
}
