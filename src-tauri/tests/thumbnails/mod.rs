use super::*;
use tempfile::tempdir;
mod generated;
mod lifecycle;

fn jpeg() -> Vec<u8> {
    vec![255, 216, 255, 217]
}
#[test]
fn cache_persists_and_evicts_least_recently_used_with_byte_and_count_limits() {
    for (byte_limit, count_limit) in [(8, 100), (100, 2)] {
        let directory = tempdir().unwrap();
        let mut cache =
            cache::Cache::with_limits(directory.path(), byte_limit, count_limit).unwrap();
        cache.put("a", &jpeg()).unwrap();
        cache.put("b", &jpeg()).unwrap();
        assert!(cache.get("a").unwrap().is_some());
        cache.put("c", &jpeg()).unwrap();
        assert!(cache.get("b").unwrap().is_none());
        drop(cache);
        let mut cache = cache::Cache::open(directory.path()).unwrap();
        assert!(cache.get("a").unwrap().is_some());
        assert!(cache.get("c").unwrap().is_some());
        assert!(cache.put("bad", b"not an image").is_err());
        assert!(cache.put("huge", &vec![0; cache::MAX_IMAGE + 1]).is_err());
    }
}

#[test]
fn cache_fingerprints_change_for_content_fingerprints_and_reuse_physical_moves() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("synthetic.mp4");
    std::fs::write(&path, b"synthetic").unwrap();
    let mut file = files::describe(&path).unwrap();
    let key = fingerprint(1, &file).unwrap();
    file.size += 1;
    assert_ne!(key, fingerprint(1, &file).unwrap());
    file.size -= 1;
    file.modified_ns = Some("changed".into());
    assert_ne!(key, fingerprint(1, &file).unwrap());
    file.modified_ns = files::describe(&path).unwrap().modified_ns;
    if file.identity.is_some() {
        file.path = "another selected location".into();
        assert_eq!(key, fingerprint(1, &file).unwrap());
    }
}

#[tokio::test]
async fn admission_is_single_owner_and_cancellation_is_scoped() {
    let directory = tempdir().unwrap();
    let manager = Thumbnails::new(directory.path());
    let analysis = Arc::new(Analysis::default());
    let mut ticket = manager
        .begin("first".into(), analysis.clone())
        .unwrap()
        .unwrap();
    assert!(manager
        .begin("second".into(), analysis.clone())
        .unwrap()
        .is_none());
    manager.cancel("wrong", &analysis);
    assert!(!*ticket.cancel.borrow());
    manager.cancel("first", &analysis);
    ticket.cancel.wait_for(|v| *v).await.unwrap();
    drop(ticket);
    assert!(manager.is_idle());
    let ticket = manager
        .begin("next".into(), analysis.clone())
        .unwrap()
        .unwrap();
    manager.stop(&analysis);
    assert!(*ticket.cancel.borrow());
    drop(ticket);
    manager.shutdown().await;
    assert!(manager.begin("late".into(), analysis).unwrap().is_none());
}
