use super::*;
use std::{path::Path, process::Command};

fn helper(directory: &Path) -> PathBuf {
    let executable = directory.join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    if !executable.exists() {
        let output = Command::new("rustc")
            .arg("--edition=2021")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/analysis/helper.rs"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    executable
}

fn fixture(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(format!("{name}.mp4"));
    std::fs::write(&path, b"synthetic input, not a video").unwrap();
    path
}

async fn started(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(path.with_extension("started")) {
                if let Ok(pid) = text.parse() {
                    return pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("synthetic child did not start")
}

fn assert_reaped(pid: u32) {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, timeout: u32) -> u32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
            fn GetLastError() -> u32;
        }
        let handle = OpenProcess(0x00100000, 0, pid); // SYNCHRONIZE only
        if !handle.is_null() {
            let status = WaitForSingleObject(handle, 0);
            CloseHandle(handle);
            assert_eq!(status, 0, "cancelled child still running");
        } else {
            assert_eq!(GetLastError(), 87, "could not verify child termination");
        }
    }
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        }
        let mut status = 0;
        // A running child returns 0, an unreaped zombie returns its pid. Reaped returns -1.
        assert_eq!(waitpid(pid as i32, &mut status, 1), -1);
    }
}

async fn response(receiver: oneshot::Receiver<Reply>) -> Reply {
    tokio::time::timeout(Duration::from_secs(10), receiver)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn switching_cancels_reaps_and_runs_only_latest_pending_request() {
    let directory = tempfile::tempdir().unwrap();
    let executable = helper(directory.path());
    let analysis = Arc::new(Analysis::default());
    let a = fixture(directory.path(), "slow-a");
    let b = fixture(directory.path(), "b");
    let c = fixture(directory.path(), "c");
    let first = analysis.request(executable.clone(), a.clone());
    let pid = started(&a).await;
    // Exercise a burst against the worker's separate runtime.
    analysis.supersede();
    let second = analysis.request(executable.clone(), b);
    let third = analysis.request(executable, c);
    assert!(response(first).await.unwrap().is_none());
    assert_reaped(pid);
    // B may finish before C arrives on another thread, but cannot overwrite C or overlap A/C.
    let _ = response(second).await.unwrap();
    let info = response(third).await.unwrap().unwrap();
    assert_eq!(serde_json::to_value(info).unwrap()["container"], "c");
    analysis.shutdown().await;
    assert!(analysis.is_idle());
    assert!(!directory.path().join("overlap").exists());
}

#[tokio::test]
async fn pending_requests_are_replaced_before_worker_starts() {
    // The worker is normally launched on Tauri's runtime. Mark it running to inspect the
    // bounded pending slot deterministically before explicitly driving the worker.
    let analysis = Arc::new(Analysis::default());
    analysis.state.lock().unwrap().running = true;
    let directory = tempfile::tempdir().unwrap();
    let first = analysis.request(helper(directory.path()), fixture(directory.path(), "a"));
    let second = analysis.request(helper(directory.path()), fixture(directory.path(), "b"));
    let third = analysis.request(helper(directory.path()), fixture(directory.path(), "c"));
    assert!(response(first).await.unwrap().is_none());
    assert!(response(second).await.unwrap().is_none());
    analysis.run().await;
    assert!(response(third).await.unwrap().is_some());
    assert!(!directory.path().join("a.started").exists());
    assert!(!directory.path().join("b.started").exists());
}

#[tokio::test]
async fn timeout_failure_and_output_limit_release_the_worker() {
    let directory = tempfile::tempdir().unwrap();
    let executable = helper(directory.path());
    let analysis = Arc::new(Analysis {
        timeout: Duration::from_secs(2),
        ..Analysis::default()
    });
    for (name, message) in [
        ("slow-timeout", "timed out"),
        ("failure", "could not analyze"),
        ("oversized", "safety limit"),
    ] {
        let path = fixture(directory.path(), name);
        let request = analysis.request(executable.clone(), path.clone());
        let pid = started(&path).await;
        assert!(response(request).await.unwrap_err().contains(message));
        assert_reaped(pid);
        let next = analysis.request(executable.clone(), fixture(directory.path(), "recovery"));
        assert!(response(next).await.unwrap().is_some());
    }
    analysis.shutdown().await;
    assert!(!directory.path().join("overlap").exists());
}

#[tokio::test]
async fn shutdown_reaps_active_child_and_discards_pending_and_future_requests() {
    let directory = tempfile::tempdir().unwrap();
    let executable = helper(directory.path());
    let analysis = Arc::new(Analysis::default());
    let path = fixture(directory.path(), "slow-shutdown");
    let request = analysis.request(executable.clone(), path.clone());
    let pid = started(&path).await;
    // Dropping an IPC receiver must not drop the process owner.
    drop(request);
    let pending = analysis.request(
        executable.clone(),
        fixture(directory.path(), "slow-pending"),
    );
    tokio::time::timeout(Duration::from_secs(10), analysis.shutdown())
        .await
        .unwrap();
    assert!(response(pending).await.unwrap().is_none());
    assert_reaped(pid);
    assert!(analysis.is_idle());
    if let Ok(pid) = std::fs::read_to_string(directory.path().join("slow-pending.started")) {
        if let Ok(pid) = pid.parse() {
            assert_reaped(pid);
        }
    }
    let after = analysis.request(executable, fixture(directory.path(), "after-shutdown"));
    assert!(response(after).await.unwrap().is_none());
    assert!(!directory.path().join("after-shutdown.started").exists());
    assert!(!directory.path().join("overlap").exists());
}

#[tokio::test]
async fn already_cancelled_probe_never_spawns() {
    let directory = tempfile::tempdir().unwrap();
    let path = fixture(directory.path(), "cancelled");
    let (_cancel, mut cancelled) = watch::channel(true);
    assert!(media::probe_cancellable(
        &helper(directory.path()),
        &path,
        &mut cancelled,
        Duration::from_secs(2)
    )
    .await
    .unwrap()
    .is_none());
    assert!(!path.with_extension("started").exists());
}

#[tokio::test]
async fn foreground_preempts_background_and_reaps_before_starting() {
    let directory = tempfile::tempdir().unwrap();
    let executable = helper(directory.path());
    let analysis = Arc::new(Analysis::default());
    let background = fixture(directory.path(), "slow-background");
    let reply = analysis
        .try_background(executable.clone(), background.clone())
        .unwrap();
    let pid = started(&background).await;
    assert!(analysis
        .try_background(executable.clone(), background)
        .is_none());
    let foreground = analysis.request(executable.clone(), fixture(directory.path(), "foreground"));
    assert!(response(reply).await.unwrap().is_none());
    assert_reaped(pid);
    assert!(response(foreground).await.unwrap().is_some());
    analysis.shutdown().await;
    assert!(!directory.path().join("overlap").exists());
}

#[tokio::test]
async fn cancelling_library_work_never_cancels_foreground_inspection() {
    let directory = tempfile::tempdir().unwrap();
    let executable = helper(directory.path());
    let analysis = Arc::new(Analysis {
        timeout: Duration::from_millis(300),
        ..Analysis::default()
    });
    let path = fixture(directory.path(), "slow-foreground");
    let reply = analysis.request(executable.clone(), path.clone());
    let pid = started(&path).await;
    analysis.cancel_background();
    assert!(response(reply).await.unwrap_err().contains("timed out"));
    assert_reaped(pid);
    let path = fixture(directory.path(), "slow-background");
    // Response delivery precedes the worker's idle transition by a few instructions.
    let background = loop {
        if let Some(reply) = analysis.try_background(executable.clone(), path.clone()) {
            break reply;
        }
        tokio::task::yield_now().await;
    };
    let pid = started(&path).await;
    analysis.cancel_background();
    assert!(response(background).await.unwrap().is_none());
    assert_reaped(pid);
    let recovery = analysis.request(executable, fixture(directory.path(), "recovery"));
    assert!(response(recovery).await.unwrap().is_some());
    analysis.shutdown().await;
    assert!(!directory.path().join("overlap").exists());
}

#[tokio::test]
async fn thumbnail_cancellation_and_preemption_reap_and_never_overlap_probe() {
    let directory = tempfile::tempdir().unwrap();
    let executable = helper(directory.path());
    let analysis = Arc::new(Analysis::default());
    for foreground in [false, true] {
        let path = fixture(
            directory.path(),
            if foreground {
                "slow-preempt"
            } else {
                "slow-cancel"
            },
        );
        let mut thumbnail_reply = loop {
            if let Some(reply) =
                analysis.try_thumbnail(executable.clone(), path.clone(), 0.0, "thumbnail".into())
            {
                break reply;
            }
            tokio::task::yield_now().await;
        };
        let pid = started(&path).await;
        analysis.cancel_thumbnail("wrong-owner");
        analysis.cancel_background();
        assert!(thumbnail_reply.try_recv().is_err());
        let probe = if foreground {
            Some(analysis.request(executable.clone(), fixture(directory.path(), "recovery")))
        } else {
            analysis.cancel_thumbnail("thumbnail");
            None
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(10), thumbnail_reply)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .is_none()
        );
        assert_reaped(pid);
        if let Some(probe) = probe {
            assert!(response(probe).await.unwrap().is_some());
        }
    }
    let oversized = fixture(directory.path(), "oversized");
    let request = loop {
        if let Some(reply) = analysis.try_thumbnail(
            executable.clone(),
            oversized.clone(),
            0.0,
            "oversized".into(),
        ) {
            break reply;
        }
        tokio::task::yield_now().await;
    };
    let pid = started(&oversized).await;
    assert!(request.await.unwrap().unwrap_err().contains("safety limit"));
    assert_reaped(pid);
    let probe = analysis.request(executable, fixture(directory.path(), "recovery"));
    assert!(response(probe).await.unwrap().is_some());
    analysis.shutdown().await;
    assert!(!directory.path().join("overlap").exists());
}
