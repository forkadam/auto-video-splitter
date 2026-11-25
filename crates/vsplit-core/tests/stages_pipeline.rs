use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use vsplit_core::actions::{approve_workspaces, cuts_get, cuts_set};
use vsplit_core::jobs::JobCoordinator;
use vsplit_core::locks::LockActor;
use vsplit_core::manifest::load_manifest;
use vsplit_core::orchestrator::{dispatch, resolve_stages_to_run, WorkspaceRunOutcome};
use vsplit_core::paths::workspace_dir;
use vsplit_core::scan::adopt_or_find;
use vsplit_core::schemas::config::Config;
use vsplit_core::schemas::manifest::StageStatus;
use vsplit_core::schemas::plan::CutsResolvedJson;
use vsplit_core::toolchain::cached_toolchain;

fn fixture_clip(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[tokio::test]
async fn full_seven_stage_pipeline_with_merge_guard_and_verify_invariants() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let clip_dst = root.join("clip.mp4");
    fs::copy(fixture_clip("short.mp4"), &clip_dst).unwrap();

    let slug = adopt_or_find(root, &clip_dst).unwrap();
    assert_eq!(slug, "clip");

    let cfg = Config::default();
    let tc = cached_toolchain();
    let coord = JobCoordinator::new();

    // Run through review -> stops at awaiting_user
    let stages = resolve_stages_to_run(None, None, None).unwrap();
    let summary1 = dispatch(
        root,
        std::slice::from_ref(&slug),
        &cfg,
        &tc,
        &stages,
        &coord,
        LockActor::Cli,
    )
    .await;
    assert!(summary1.any_awaiting_user);
    assert!(!summary1.any_failed);
    assert!(matches!(
        summary1.workspaces.get(&slug),
        Some(WorkspaceRunOutcome::AwaitingUser)
    ));

    let ws = workspace_dir(root, &slug);
    let m1 = load_manifest(&ws).unwrap();
    assert_eq!(m1.stage("probe").status, StageStatus::Done);
    assert_eq!(m1.stage("detect").status, StageStatus::Done);
    assert_eq!(m1.stage("preview").status, StageStatus::Done);
    assert_eq!(m1.stage("review").status, StageStatus::AwaitingUser);
    assert_eq!(m1.stage("plan").status, StageStatus::Pending);

    // Preview HTML rendered via MiniJinja with pre-formatted fields and proxy video
    let html = fs::read_to_string(ws.join("preview/index.html")).unwrap();
    assert!(html.contains("id=\"scene-001\""));
    assert!(html.contains("source-proxy.mp4"));
    assert!(ws.join("preview/source-proxy.mp4").is_file());

    // Edit cuts: set merge_with_next=true on scene 1, drop scene 2 (keep=false), keep scene 3.
    // Merge gap guard MUST NOT bridge scene 1 and scene 3 across dropped scene 2!
    let mut doc = cuts_get(root, &slug).unwrap();
    assert_eq!(doc.cuts.scenes.len(), 3);
    doc.cuts.scenes[0].merge_with_next = true;
    doc.cuts.scenes[0].label = Some("Opening Shot!".into());
    doc.cuts.scenes[1].keep = false;
    doc.cuts.scenes[2].keep = true;
    // Also test plan.min_segment_length no-op: shorten scene 3 to 0.4s (< 1.0s) and verify it is NOT dropped.
    doc.cuts.scenes[2].start = "00:00:04.200".into();
    doc.cuts.scenes[2].end = "00:00:04.600".into();

    // Bad expected_hash must fail
    assert!(cuts_set(root, &slug, &doc.cuts, Some("0000"), LockActor::Cli).is_err());

    // Good expected_hash succeeds
    let updated = cuts_set(root, &slug, &doc.cuts, Some(&doc.hash), LockActor::Cli).unwrap();
    assert_ne!(updated.hash, doc.hash);

    // Resume pipeline (cuts.json hash changed -> review advances to done -> plan -> split -> verify)
    let summary2 = dispatch(
        root,
        std::slice::from_ref(&slug),
        &cfg,
        &tc,
        &stages,
        &coord,
        LockActor::Cli,
    )
    .await;
    assert!(!summary2.any_failed);
    assert!(!summary2.any_awaiting_user);

    let m2 = load_manifest(&ws).unwrap();
    assert_eq!(m2.stage("review").status, StageStatus::Done);
    assert_eq!(m2.stage("plan").status, StageStatus::Done);
    assert_eq!(m2.stage("split").status, StageStatus::Done);
    assert_eq!(m2.stage("verify").status, StageStatus::Done);

    // Check plan/cuts-resolved.json: 2 separate segments (not merged across gap!), short 0.4s scene kept!
    let plan: CutsResolvedJson =
        serde_json::from_str(&fs::read_to_string(ws.join("plan/cuts-resolved.json")).unwrap())
            .unwrap();
    assert_eq!(plan.scenes.len(), 2);
    assert_eq!(plan.scenes[0].label.as_deref(), Some("opening-shot"));
    assert_eq!(
        plan.scenes[0].output_filename,
        "clip-highlight-01-opening-shot.mp4"
    );
    assert_eq!(plan.scenes[1].id, 3);
    assert!(ws
        .join("output")
        .join(&plan.scenes[0].output_filename)
        .is_file());
    assert!(ws
        .join("output")
        .join(&plan.scenes[1].output_filename)
        .is_file());

    // Verify report has all_ok = true
    assert_eq!(
        m2.stage("verify").artifacts["report"]["all_ok"],
        serde_json::json!(true)
    );

    // Live verify.py invariant: removing an output file and re-running verify returns status=done with all_ok=false!
    fs::remove_file(ws.join("output").join(&plan.scenes[1].output_filename)).unwrap();
    vsplit_core::actions::reset_workspaces(
        root,
        std::slice::from_ref(&slug),
        "verify",
        LockActor::Cli,
    )
    .unwrap();
    let verify_only = resolve_stages_to_run(Some("verify"), None, None).unwrap();
    let summary3 = dispatch(
        root,
        std::slice::from_ref(&slug),
        &cfg,
        &tc,
        &verify_only,
        &coord,
        LockActor::Cli,
    )
    .await;
    assert!(!summary3.any_failed);
    let m3 = load_manifest(&ws).unwrap();
    assert_eq!(m3.stage("verify").status, StageStatus::Done);
    assert_eq!(
        m3.stage("verify").artifacts["report"]["all_ok"],
        serde_json::json!(false)
    );
}

#[tokio::test]
async fn approve_unlocks_unchanged_cuts_and_cancellation_aborts_cleanly() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let clip_dst = root.join("clip2.mp4");
    fs::copy(fixture_clip("short.mp4"), &clip_dst).unwrap();
    let slug = adopt_or_find(root, &clip_dst).unwrap();

    let mut cfg = Config::default();
    cfg.preview.proxy_height = None; // skip proxy for fast test
    let tc = cached_toolchain();
    let coord = JobCoordinator::new();

    let through_rev = resolve_stages_to_run(None, None, Some("review")).unwrap();
    let s1 = dispatch(
        root,
        std::slice::from_ref(&slug),
        &cfg,
        &tc,
        &through_rev,
        &coord,
        LockActor::Cli,
    )
    .await;
    assert!(s1.any_awaiting_user);

    // Approve without editing cuts.json
    let approved = approve_workspaces(root, std::slice::from_ref(&slug), LockActor::Cli).unwrap();
    assert_eq!(approved, vec![slug.clone()]);

    // Pre-cancelled coordinator must not advance split to done
    let cancel_coord = JobCoordinator::new();
    cancel_coord.cancel_all();
    let all = resolve_stages_to_run(None, None, None).unwrap();
    let s_cancel = dispatch(
        root,
        std::slice::from_ref(&slug),
        &cfg,
        &tc,
        &all,
        &cancel_coord,
        LockActor::Cli,
    )
    .await;
    assert!(s_cancel.cancelled);
    let m = load_manifest(&workspace_dir(root, &slug)).unwrap();
    assert_ne!(m.stage("split").status, StageStatus::Done);
}
