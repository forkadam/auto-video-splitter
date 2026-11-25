use std::fs;
use std::time::Duration;

use chrono::Utc;
use tempfile::TempDir;
use tokio::time::timeout;
use vsplit_core::atomic::{atomic_write_json, atomic_write_text};
use vsplit_core::index::save_index;
use vsplit_core::locks::{try_acquire_workspace_lock, LockActor, LockLease};
use vsplit_core::manifest::{new_manifest, save_manifest};
use vsplit_core::paths::workspace_dir;
use vsplit_core::schemas::cuts::{CutsJson, Scene};
use vsplit_core::schemas::index::{Index, WorkspaceEntry};
use vsplit_core::schemas::manifest::SourceInfo;
use vsplit_server::sse::SseEvent;
use vsplit_server::start_server;

#[tokio::test]
async fn watcher_emits_cuts_config_lock_events_and_broker_signals_sync_lagged() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    // Pre-create workspace + decisions dir before opening project so non-recursive watch covers them
    let ws = workspace_dir(root, "alpha");
    fs::create_dir_all(ws.join("decisions")).unwrap();
    fs::create_dir_all(root.join(".vsplit/.locks")).unwrap();
    let mut m = new_manifest(
        "alpha",
        SourceInfo {
            relative_path: "alpha.mp4".into(),
            size_bytes: 100,
            mtime: Utc::now(),
            partial_hash: "sha256:00".into(),
        },
    );
    save_manifest(&ws, &mut m).unwrap();
    let mut idx = Index::default();
    idx.workspaces.insert(
        "alpha".into(),
        WorkspaceEntry {
            source_relative_path: "alpha.mp4".into(),
            size_bytes: 100,
            partial_hash: "sha256:00".into(),
            adopted_at: Utc::now(),
        },
    );
    save_index(root, &mut idx).unwrap();

    let handle = start_server(Some(root.to_path_buf())).await.unwrap();
    let mut rx = handle.state.broker.subscribe();

    // Give notify watcher a moment to arm OS watches
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 1. External edit to decisions/cuts.json -> emits CutsChanged
    let cuts = CutsJson {
        schema_version: 1,
        scenes: vec![Scene {
            id: 1,
            start: "00:00:00.000".into(),
            end: "00:00:02.000".into(),
            keep: true,
            label: Some("ext".into()),
            merge_with_next: false,
        }],
    };
    atomic_write_json(&ws.join("decisions/cuts.json"), &cuts).unwrap();

    let mut saw_cuts = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = timeout(Duration::from_millis(400), rx.recv()).await {
            if matches!(ev, SseEvent::CutsChanged { ref slug, .. } if slug == "alpha") {
                saw_cuts = true;
                break;
            }
        }
    }
    assert!(
        saw_cuts,
        "expected CutsChanged SSE event after external cuts.json write"
    );

    // 2. External edit to .vsplit/config.yaml -> emits ConfigChanged
    atomic_write_text(
        &root.join(".vsplit/config.yaml"),
        "detect:\n  threshold: 25.0\n",
    )
    .unwrap();
    let mut saw_cfg = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = timeout(Duration::from_millis(400), rx.recv()).await {
            if matches!(ev, SseEvent::ConfigChanged) {
                saw_cfg = true;
                break;
            }
        }
    }
    assert!(
        saw_cfg,
        "expected ConfigChanged SSE event after external config.yaml write"
    );

    // 3. External lock acquire -> emits LockChanged
    let guard = try_acquire_workspace_lock(
        root,
        "alpha",
        LockLease::new(LockActor::Cli, "detect", "r-watch"),
    )
    .unwrap();
    let mut saw_lock = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = timeout(Duration::from_millis(400), rx.recv()).await {
            if matches!(ev, SseEvent::LockChanged { ref slug, .. } if slug == "alpha") {
                saw_lock = true;
                break;
            }
        }
    }
    drop(guard);
    assert!(
        saw_lock,
        "expected LockChanged SSE event on lock file write"
    );

    // 4. Broadcast overflow -> RecvError::Lagged triggers sync_lagged recovery
    let mut slow_rx = handle.state.broker.subscribe();
    for _ in 0..300 {
        handle.state.broker.publish(SseEvent::ConfigChanged);
    }
    assert!(matches!(
        slow_rx.recv().await,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
    ));
}
