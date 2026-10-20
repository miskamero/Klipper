//! Shared bounded, cancellable subprocess capture. The caller supplies arguments, never a shell.
use std::{
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::watch,
};

pub(crate) async fn read_bounded(
    reader: impl AsyncRead + Unpin,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Could not read media tool output.")?;
    if bytes.len() as u64 > limit {
        return Err("Media tool output exceeded the safety limit.".into());
    }
    Ok(bytes)
}

pub(crate) async fn capture(
    command: &mut Command,
    cancelled: &mut watch::Receiver<bool>,
    timeout: Duration,
    output_limit: u64,
    tool: &str,
    low_priority: bool,
) -> Result<Option<(Vec<u8>, ExitStatus)>, String> {
    if *cancelled.borrow() {
        return Ok(None);
    }
    crate::config::secure_media_command(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000 | if low_priority { 0x00004000 } else { 0 });
    #[cfg(not(windows))]
    let _ = low_priority;
    let mut child = command
        .spawn()
        .map_err(|_| crate::config::media_start_error(tool))?;
    let result = {
        let output = async {
            let stdout = child
                .stdout
                .take()
                .ok_or("Media tool output unavailable.")?;
            let stderr = child
                .stderr
                .take()
                .ok_or("Media tool error output unavailable.")?;
            tokio::try_join!(
                read_bounded(stdout, output_limit),
                read_bounded(stderr, 64 * 1024),
                async {
                    child
                        .wait()
                        .await
                        .map_err(|_| "Media tool process failed.".to_string())
                }
            )
        };
        tokio::select! {
            biased;
            _=cancelled.wait_for(|value| *value)=>None,
            result=tokio::time::timeout(timeout,output)=>Some(result),
        }
    };
    match result {
        Some(Ok(Ok((stdout, _stderr, status)))) => Ok(Some((stdout, status))),
        other => {
            let _ = child.start_kill();
            child
                .wait()
                .await
                .map_err(|_| format!("Could not reap the {tool} process."))?;
            match other {
                None => Ok(None),
                Some(Ok(Err(error))) => Err(error),
                _ => Err(format!(
                    "{tool} analysis timed out after {} seconds.",
                    timeout.as_secs()
                )),
            }
        }
    }
}
