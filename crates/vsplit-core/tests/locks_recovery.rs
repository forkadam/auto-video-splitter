use std::fs;

use chrono::Utc;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use vsplit_core::actions::clean_workspaces;
use vsplit_core::atomic::atomic_write_json;
use vsplit_core::index::save_index;
use vsplit_core::locks::{
    inspect_workspace_lock, list_active_locks, lock_index, try_acquire_workspace_lock,
    workspace_lease_path, workspace_lock_path, LockActor, LockError, LockLease,
};
use vsplit_core::manifest::{load_manifest, new_manifest, save_manifest};
use vsplit_core::paths::workspace_dir;
use vsplit_core::schemas::config::Config;
use vsplit_core::schemas::cuts::{CutsJson, Scene};
use vsplit_core::schemas::index::{Index, WorkspaceEntry};
use vsplit_core::schemas::manifest::{SourceInfo, StageStatus};
use vsplit_core::schemas::scenes::{SceneEntry, ScenesDetectorConfig, ScenesJson};
use vsplit_core::stages::{recover_stage_kind, StageContext, StageKind, StageLogger};
use vsplit_core::toolchain::{EncoderCapabilities, MediaToolchain};

#[test]
fn workspace_lock_and_readable_sibling_lease_and_stale_cleanup() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    assert!(inspect_workspace_lock(root, "clip-1").is_none());

    let lease = LockLease::new(LockActor::Desktop, "detect", "run-abc");
    let mut guard = try_acquire_workspace_lock(root, "clip-1", lease).unwrap();

    // Sibling .lease.json is readable while .lock is exclusively held (no Windows sharing error).
    let inspected = inspect_workspace_lock(root, "clip-1").expect("expected active lock");
    assert_eq!(inspected.actor, LockActor::Desktop);
    assert_eq!(inspected.stage, "detect");

    // Stage update propagates to .lease.json
    guard.update_stage("preview");
    let inspected2 = inspect_workspace_lock(root, "clip-1").unwrap();
    assert_eq!(inspected2.stage, "preview");

    // Second acquire fails fast with LockError::Held
    let err = try_acquire_workspace_lock(
        root,
        "clip-1",
        LockLease::new(LockActor::Cli, "run", "run-xyz"),
    )
    .err()
    .expect("expected LockError::Held");
    match err {
        LockError::Held { slug, lease } => {
            assert_eq!(slug, "clip-1");
            assert_eq!(lease.actor, LockActor::Desktop);
            assert_eq!(lease.stage, "preview");
        }
        other => panic!("unexpected error: {other}"),
    }

    assert_eq!(list_active_locks(root).len(), 1);

    // Dropping guard removes .lease.json and releases lock, leaving empty sentinel .lock intact
    drop(guard);
    assert!(workspace_lock_path(root, "clip-1").exists());
    assert!(!workspace_lease_path(root, "clip-1").exists());
    assert!(inspect_workspace_lock(root, "clip-1").is_none());

    // Simulate a SIGKILL crash leaving a stale .lease.json while .lock is unlocked:
    atomic_write_json(
        &workspace_lease_path(root, "clip-1"),
        &LockLease::new(LockActor::Cli, "split", "dead-run"),
    )
    .unwrap();
    assert!(
        inspect_workspace_lock(root, "clip-1").is_none(),
        "stale lease must be ignored when .lock sentinel is not held"
    );

    // Index lock acquires and releases cleanly
    let idx_guard = lock_index(root).unwrap();
    drop(idx_guard);
}

#[test]
fn clean_fails_fast_when_workspace_is_locked() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let ws = workspace_dir(root, "busy");
    fs::create_dir_all(&ws).unwrap();
    let mut m = new_manifest(
        "busy",
        SourceInfo {
            relative_path: "busy.mp4".into(),
            size_bytes: 10,
            mtime: Utc::now(),
            partial_hash: "sha256:00".into(),
        },
    );
    save_manifest(&ws, &mut m).unwrap();
    let mut idx = Index::default();
    idx.workspaces.insert(
        "busy".into(),
        WorkspaceEntry {
            source_relative_path: "busy.mp4".into(),
            size_bytes: 10,
            partial_hash: "sha256:00".into(),
            adopted_at: Utc::now(),
        },
    );
    save_index(root, &mut idx).unwrap();

    let _held = try_acquire_workspace_lock(
        root,
        "busy",
        LockLease::new(LockActor::Desktop, "split", "r1"),
    )
    .unwrap();

    let res = clean_workspaces(root, &["busy".into()], LockActor::Cli);
    assert!(res.is_err());
    assert!(
        ws.exists(),
        "locked workspace directory must not be deleted"
    );
}

#[tokio::test]
async fn review_recovery_is_non_destructive_when_cuts_json_is_valid() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let ws = workspace_dir(root, "rev-test");
    fs::create_dir_all(ws.join("detect")).unwrap();
    fs::create_dir_all(ws.join("decisions")).unwrap();

    atomic_write_json(
        &ws.join("detect/scenes.json"),
        &ScenesJson {
            detector: "content".into(),
            config: ScenesDetectorConfig { threshold: 27.0 },
            scenes: vec![SceneEntry {
                id: 1,
                start_time: 0.0,
                end_time: 2.0,
                start_frame: 0,
                end_frame: 48,
            }],
        },
    )
    .unwrap();

    let user_cuts = CutsJson {
        schema_version: 1,
        scenes: vec![Scene {
            id: 1,
            start: "00:00:00.000".into(),
            end: "00:00:02.000".into(),
            keep: true,
            label: Some("my-precious-label".into()),
            merge_with_next: false,
        }],
    };
    atomic_write_json(&ws.join("decisions/cuts.json"), &user_cuts).unwrap();

    let mut m = new_manifest(
        "rev-test",
        SourceInfo {
            relative_path: "rev.mp4".into(),
            size_bytes: 10,
            mtime: Utc::now(),
            partial_hash: "sha256:00".into(),
        },
    );
    m.stage_mut("review").status = StageStatus::Running;
    save_manifest(&ws, &mut m).unwrap();

    let cfg = Config::default();
    let tc = MediaToolchain {
        ffmpeg: None,
        ffprobe: None,
        encoders: EncoderCapabilities::default(),
    };
    let logger = StageLogger::new(&ws, "review", "recover-test");
    let mut ctx = StageContext {
        workspace_dir: ws.clone(),
        working_root: root.to_path_buf(),
        source_path: root.join("rev.mp4"),
        manifest: &mut m,
        config: &cfg,
        toolchain: &tc,
        logger: &logger,
        cancel: CancellationToken::new(),
    };

    recover_stage_kind(StageKind::Review, &mut ctx).await;
    save_manifest(&ws, &mut m).unwrap();

    let reloaded = load_manifest(&ws).unwrap();
    assert_eq!(reloaded.stage("review").status, StageStatus::AwaitingUser);
    let preserved = fs::read_to_string(ws.join("decisions/cuts.json")).unwrap();
    assert!(
        preserved.contains("my-precious-label"),
        "valid user cuts.json must be preserved on review recovery"
    );

    // Corrupt cuts.json -> recovery wipes decisions/ and resets to pending
    fs::write(ws.join("decisions/cuts.json"), "{broken json").unwrap();
    m.stage_mut("review").status = StageStatus::Running;
    let mut ctx2 = StageContext {
        workspace_dir: ws.clone(),
        working_root: root.to_path_buf(),
        source_path: root.join("rev.mp4"),
        manifest: &mut m,
        config: &cfg,
        toolchain: &tc,
        logger: &logger,
        cancel: CancellationToken::new(),
    };
    recover_stage_kind(StageKind::Review, &mut ctx2).await;
    assert_eq!(m.stage("review").status, StageStatus::Pending);
    assert!(!ws.join("decisions/cuts.json").exists());
}
