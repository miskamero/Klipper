use super::super::tests::make_source;
use super::super::{media, Output};
use super::*;

async fn spec(source: &Path, output: &Path, start: f64, end: f64) -> Spec {
    let (_send, mut cancel) = watch::channel(false);
    let info = media::inspect(Path::new("ffprobe"), source, &mut cancel)
        .await
        .unwrap();
    let start = media::boundary(Path::new("ffprobe"), source, &info, start, &mut cancel)
        .await
        .unwrap();
    let end = media::boundary(Path::new("ffprobe"), source, &info, end, &mut cancel)
        .await
        .unwrap();
    Spec {
        source: files::describe(source).unwrap(),
        project: Project {
            output: Output {
                encoder: Encoder::Cpu,
                ..Output::default()
            },
            version: 1,
            token: 99,
            recording_id: 999,
            revision: "fixture".into(),
            start,
            end,
            info,
        },
        destination: export::Destination::new(output).unwrap(),
        executable: "ffmpeg".into(),
        probe: "ffprobe".into(),
    }
}
async fn wait_idle(queue: &Queue) {
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut idle = queue.idle.subscribe();
        let _ = idle.wait_for(|v| *v).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn queue_bounds_history_and_pending_work_and_never_starts_cancelled_entries() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("fixture.mp4");
    std::fs::write(&source, b"not executed").unwrap();
    let info=media::parse(&serde_json::to_vec(&serde_json::json!({"format":{"start_time":"0","duration":"4"},"streams":[{"index":0,"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p","width":160,"height":96,"time_base":"1/30","start_time":"0","duration":"4"}]})).unwrap()).unwrap();
    let make = |index| Spec {
        source: files::describe(&source).unwrap(),
        project: Project {
            output: Output::default(),
            version: 1,
            token: 999,
            recording_id: 999,
            revision: "not needed after admission".into(),
            start: 0.0,
            end: 1.0,
            info: info.clone(),
        },
        destination: export::Destination::new(
            &directory.path().join(format!("output-{index}.mp4")),
        )
        .unwrap(),
        executable: "must-not-start".into(),
        probe: "must-not-start".into(),
    };
    let queue = held_queue();
    for batch in 0..4 {
        let mut ids = Vec::new();
        for index in 0..16 {
            ids.push(queue.enqueue(make(batch * 16 + index)).unwrap());
        }
        assert!(queue.enqueue(make(999)).is_err());
        assert!(queue.dismiss(ids[0]).is_err());
        for id in ids {
            queue.cancel(id).unwrap();
        }
    }
    assert_eq!(queue.snapshot().jobs.len(), 64);
    assert!(queue.enqueue(make(1000)).is_err());
    queue.dismiss(1).unwrap();
    queue.enqueue(make(1000)).unwrap();
    queue.stop();
    queue.clone().run().await;
    assert!(queue.is_idle());
    assert!(queue.support.get().is_none());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
fn held_queue() -> Arc<Queue> {
    let queue = ExportState::default().0;
    queue.inner.lock().unwrap().worker = true;
    queue.idle.send_replace(false);
    queue
}

#[tokio::test]
#[ignore = "generates synthetic H.264/HEVC fixtures; requires FFmpeg and FFprobe"]
async fn fifo_jobs_are_independent_cancel_pending_reserve_names_and_continue_after_failure() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.mp4");
    let b = directory.path().join("b.mp4");
    make_source(&a, false, false, false);
    make_source(&b, true, true, true);
    let before_a = std::fs::read(&a).unwrap();
    let before_b = std::fs::read(&b).unwrap();
    let queue = held_queue();
    let first = spec(
        &a,
        &directory.path().join("first.mp4"),
        7.0 / 30.0,
        36.0 / 30.0,
    )
    .await;
    let reserved = first.destination.path.clone();
    let first_id = queue.enqueue(first).unwrap();
    assert!(queue
        .enqueue(spec(&a, &reserved, 0.0, 1.0).await)
        .unwrap_err()
        .contains("reserves"));
    let mut failed = spec(&a, &directory.path().join("failed.mp4"), 0.0, 1.0).await;
    failed.executable = directory.path().join("absent-encoder");
    let failed_id = queue.enqueue(failed).unwrap();
    let cancelled_path = directory.path().join("never-started.mp4");
    let cancelled_id = queue
        .enqueue(spec(&a, &cancelled_path, 0.0, 1.0).await)
        .unwrap();
    queue.cancel(cancelled_id).unwrap();
    let second = spec(&b, &directory.path().join("second.mp4"), 0.3, 2.9).await;
    let second_id = queue.enqueue(second).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let copy = events.clone();
    *queue.notify.lock().unwrap() = Some(Arc::new(move |snapshot| {
        copy.lock().unwrap().push(snapshot);
    }));
    queue.clone().run().await;
    wait_idle(&queue).await;
    let jobs = queue.snapshot().jobs;
    assert_eq!(
        jobs.iter()
            .map(|j| j.status.phase.as_str())
            .collect::<Vec<_>>(),
        ["completed", "error", "cancelled", "completed"]
    );
    for event in events.lock().unwrap().iter() {
        assert!(
            event
                .jobs
                .iter()
                .filter(|j| matches!(
                    j.status.phase.as_str(),
                    "preparing" | "encoding" | "verifying"
                ))
                .count()
                <= 1
        );
        if event
            .jobs
            .iter()
            .any(|j| j.id == second_id && j.status.phase == "preparing")
        {
            assert_eq!(
                event
                    .jobs
                    .iter()
                    .find(|j| j.id == first_id)
                    .unwrap()
                    .status
                    .phase,
                "completed"
            );
        }
    }
    assert!(!cancelled_path.exists());
    assert!(jobs
        .iter()
        .find(|j| j.id == cancelled_id)
        .unwrap()
        .status
        .process_id
        .is_none());
    let (_send, mut cancel) = watch::channel(false);
    let first_info = media::inspect(Path::new("ffprobe"), &reserved, &mut cancel)
        .await
        .unwrap();
    let second_info = media::inspect(
        Path::new("ffprobe"),
        &directory.path().join("second.mp4"),
        &mut cancel,
    )
    .await
    .unwrap();
    assert!((first_info.duration - jobs[0].status.duration).abs() < 0.04);
    assert!((second_info.duration - jobs[3].status.duration).abs() < 0.04);
    assert_eq!(first_info.audio.len(), 4);
    assert_eq!(second_info.audio.len(), 4);
    assert_eq!(std::fs::read(&a).unwrap(), before_a);
    assert_eq!(std::fs::read(&b).unwrap(), before_b);
    queue.dismiss(failed_id).unwrap();
    assert_eq!(queue.snapshot().jobs.len(), 3);
}

#[tokio::test]
#[ignore = "generates synthetic media; requires FFmpeg/FFprobe"]
async fn changed_missing_sources_and_destination_collisions_fail_without_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    make_source(&source, false, false, false);
    let queue = held_queue();
    let path = directory.path().join("occupied.mp4");
    queue.enqueue(spec(&source, &path, 0.0, 1.0).await).unwrap();
    std::fs::write(&path, b"must remain").unwrap();
    let changed = directory.path().join("changed.mp4");
    std::fs::copy(&source, &changed).unwrap();
    let changed_output = directory.path().join("changed-output.mp4");
    queue
        .enqueue(spec(&changed, &changed_output, 0.0, 1.0).await)
        .unwrap();
    std::fs::write(&changed, b"synthetic replacement").unwrap();
    let missing = directory.path().join("missing.mp4");
    std::fs::copy(&source, &missing).unwrap();
    let missing_output = directory.path().join("missing-output.mp4");
    queue
        .enqueue(spec(&missing, &missing_output, 0.0, 1.0).await)
        .unwrap();
    std::fs::rename(&missing, directory.path().join("moved-fixture.mp4")).unwrap();
    queue.clone().run().await;
    assert!(queue
        .snapshot()
        .jobs
        .iter()
        .all(|j| j.status.phase == "error"));
    assert_eq!(std::fs::read(path).unwrap(), b"must remain");
    assert!(!changed_output.exists());
    assert!(!missing_output.exists());
}

#[tokio::test]
#[ignore = "generates synthetic media; requires FFmpeg/FFprobe"]
async fn shutdown_and_active_cancellation_reap_before_starting_the_next_job() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    make_source(&source, false, false, false);
    let queue = held_queue();
    let id = queue
        .enqueue(spec(&source, &directory.path().join("cancel.mp4"), 0.0, 3.9).await)
        .unwrap();
    queue
        .enqueue(spec(&source, &directory.path().join("pending.mp4"), 0.0, 1.0).await)
        .unwrap();
    let worker = tokio::spawn(queue.clone().run());
    let pid = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let jobs = queue.snapshot().jobs;
            if let Some(pid) = jobs[0].status.process_id {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    queue.cancel(id).unwrap();
    queue.shutdown().await;
    worker.await.unwrap();
    assert!(queue
        .snapshot()
        .jobs
        .iter()
        .all(|j| j.status.phase == "cancelled"));
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, ms: u32) -> u32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let handle = OpenProcess(0x00100000, 0, pid);
        if !handle.is_null() {
            assert_eq!(WaitForSingleObject(handle, 0), 0);
            CloseHandle(handle);
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
    assert!(!directory.path().join("cancel.mp4").exists());
    assert!(!directory.path().join("pending.mp4").exists());
}

#[tokio::test]
#[ignore = "generates synthetic media; requires FFmpeg/FFprobe"]
async fn automatic_encoding_falls_back_to_cpu_when_nvenc_is_unavailable() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    make_source(&source, false, false, false);
    let queue = ExportState::default().0;
    queue
        .support
        .set(Support {
            nvenc: false,
            detail: "synthetic unavailable driver".into(),
        })
        .ok()
        .unwrap();
    let mut job = spec(&source, &directory.path().join("cpu.mp4"), 0.0, 1.0).await;
    job.project.output.encoder = Encoder::Auto;
    queue.enqueue(job).unwrap();
    wait_idle(&queue).await;
    let jobs = queue.snapshot().jobs;
    assert_eq!(jobs[0].status.encoder, Encoder::Cpu);
    assert_eq!(jobs[0].status.phase, "completed");
}

#[tokio::test]
async fn shutdown_waits_for_a_capability_probe_to_release_its_process_barrier() {
    let queue = ExportState::default().0;
    let guard = queue.probe.lock().await;
    assert!(!queue.is_idle());
    let copy = queue.clone();
    let shutdown = tokio::spawn(async move {
        copy.shutdown().await;
    });
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    drop(guard);
    shutdown.await.unwrap();
    assert!(queue.is_idle());
}

#[tokio::test]
#[ignore = "generates a small synthetic input rejected by NVENC; requires FFmpeg/FFprobe"]
async fn automatic_encoding_recovers_from_a_runtime_nvenc_failure() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("tiny.mp4");
    let result = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=30:duration=2",
            "-c:v",
            "libx264",
            "-threads",
            "2",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(result.status.success());
    let queue = ExportState::default().0;
    queue
        .support
        .set(Support {
            nvenc: true,
            detail: "synthetic capability success".into(),
        })
        .ok()
        .unwrap();
    let mut job = spec(&source, &directory.path().join("fallback.mp4"), 0.0, 1.0).await;
    job.project.output.encoder = Encoder::Auto;
    queue.enqueue(job).unwrap();
    wait_idle(&queue).await;
    let jobs = queue.snapshot().jobs;
    assert_eq!(jobs[0].status.phase, "completed");
    assert_eq!(jobs[0].status.encoder, Encoder::Cpu);
    assert!(jobs[0].status.notice.contains("retrying"));
}
