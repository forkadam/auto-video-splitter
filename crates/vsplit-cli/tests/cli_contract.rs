use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;
use vsplit_core::locks::{try_acquire_workspace_lock, LockActor, LockLease};
use vsplit_core::toolchain::cached_toolchain;

fn vsplit_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_vsplit"))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

fn fixture_clip(name: &str) -> PathBuf {
    repo_root().join("tests").join("fixtures").join(name)
}

fn run_cli(working_dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(vsplit_bin())
        .arg("-w")
        .arg(working_dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("failed to execute vsplit binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn cli_end_to_end_json_contract_exit_codes_and_0_3_0_rollback_parity() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::copy(fixture_clip("short.mp4"), root.join("sub/sample.mp4")).unwrap();

    // 1. doctor --json -> exit 0
    let (code, stdout, _) = run_cli(root, &["--json", "doctor"]);
    assert_eq!(code, 0, "doctor failed: {stdout}");
    let doc: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["ffmpeg"]["ok"], true);
    assert_eq!(doc["ffprobe"]["ok"], true);

    // 2. scan --recursive --json -> adopts sub/sample.mp4 with POSIX path
    let (code, stdout, _) = run_cli(root, &["--json", "scan", "--recursive"]);
    assert_eq!(code, 0, "scan failed: {stdout}");
    let scan_val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(scan_val["ok"], true);
    assert_eq!(scan_val["adopted"][0]["slug"], "sample");
    assert_eq!(scan_val["adopted"][0]["relative_path"], "sub/sample.mp4");

    // 3. status --json -> compact projection by default, --full for manifests
    let (code, stdout, _) = run_cli(root, &["--json", "status"]);
    assert_eq!(code, 0);
    let st: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(st["ok"], true);
    assert_eq!(st["workspaces"][0]["slug"], "sample");
    assert!(st["workspaces"][0]["stages"]["probe"] == "pending");
    assert!(
        stdout.len() < 800,
        "compact status payload unexpectedly large"
    );

    let (code, full_out, _) = run_cli(root, &["--json", "status", "--full"]);
    assert_eq!(code, 0);
    let full_st: Value = serde_json::from_str(&full_out).unwrap();
    assert!(
        full_st["workspaces"]["sample"]["manifest"]["source"]["relative_path"] == "sub/sample.mp4"
    );

    // 4. Unified --filter stage= regression test: `reset` and `clean` must NOT treat stage= as match-all!
    let (code, stdout, _) = run_cli(
        root,
        &["--json", "clean", "--force", "--filter", "stage=split"],
    );
    assert_eq!(code, 0);
    let clean_miss: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        clean_miss["cleaned"], 0,
        "stage=split must not match workspace at probe"
    );

    // Unknown filter key -> exit 2
    let (code, stdout, _) = run_cli(root, &["--json", "run", "--filter", "bogus=1"]);
    assert_eq!(code, 2);
    let bad_flt: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(bad_flt["ok"], false);
    assert_eq!(bad_flt["code"], "INVALID_FILTER");

    // 5. Non-interactive fail-fast guard: clean without --force exits 2 (never hangs)
    let (code, stdout, _) = run_cli(root, &["--json", "clean", "--all"]);
    assert_eq!(code, 2);
    let non_int: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(non_int["code"], "NON_INTERACTIVE_CONFIRM_REQUIRED");

    // 6. run --through review --json -> exit 10 (AWAITING_USER)
    let (code, stdout, _) = run_cli(root, &["--json", "run", "--through", "review"]);
    assert_eq!(
        code, 10,
        "expected exit 10 AWAITING_USER, got {code}: {stdout}"
    );
    let run_wait: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(run_wait["code"], "AWAITING_USER");

    // 7. cuts get / cuts set with optimistic expected_hash
    let (code, stdout, _) = run_cli(root, &["--json", "cuts", "get", "sample"]);
    assert_eq!(code, 0, "cuts get failed: {stdout}");
    let cuts_doc: Value = serde_json::from_str(&stdout).unwrap();
    let hash = cuts_doc["hash"].as_str().unwrap().to_string();
    let cuts_file = root.join("edited_cuts.json");
    fs::write(
        &cuts_file,
        serde_json::to_string_pretty(&cuts_doc["cuts"]).unwrap(),
    )
    .unwrap();

    // Bad hash -> exit 2 HASH_MISMATCH
    let (code, stdout, _) = run_cli(
        root,
        &[
            "--json",
            "cuts",
            "set",
            "sample",
            "--file",
            cuts_file.to_str().unwrap(),
            "--expected-hash",
            "deadbeef",
        ],
    );
    assert_eq!(code, 2);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["code"],
        "HASH_MISMATCH"
    );

    // Good hash -> exit 0
    let (code, stdout, _) = run_cli(
        root,
        &[
            "--json",
            "cuts",
            "set",
            "sample",
            "--file",
            cuts_file.to_str().unwrap(),
            "--expected-hash",
            &hash,
        ],
    );
    assert_eq!(code, 0, "cuts set failed: {stdout}");

    // 8. approve + run to completion -> exit 0
    let (code, _, _) = run_cli(root, &["--json", "approve"]);
    assert_eq!(code, 0);
    let (code, stdout, _) = run_cli(root, &["--json", "run"]);
    assert_eq!(code, 0, "final run failed: {stdout}");

    // 9. logs <slug> detect --tail 5 --json
    let (code, stdout, _) = run_cli(root, &["--json", "logs", "sample", "detect", "--tail", "5"]);
    assert_eq!(code, 0, "logs failed: {stdout}");
    let log_val: Value = serde_json::from_str(&stdout).unwrap();
    assert!(log_val["content"].as_str().unwrap().contains("decode_fps="));

    // 10. ignore / unignore JSON contract (`{"unignored": 1}`)
    let (code, stdout, _) = run_cli(root, &["--json", "ignore", "sample"]);
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["ignored"],
        1
    );
    let (code, stdout, _) = run_cli(root, &["--json", "unignore", "sample"]);
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["unignored"],
        1
    );

    // 11. collect --json
    let (code, stdout, _) = run_cli(root, &["--json", "collect"]);
    assert_eq!(code, 0, "collect failed: {stdout}");
    let col: Value = serde_json::from_str(&stdout).unwrap();
    assert!(col["collected"].as_u64().unwrap() >= 1);

    // 12. Two-process lock contention: hold OS lock in parent test process, child `vsplit` MUST exit 3!
    let _held = try_acquire_workspace_lock(
        root,
        "sample",
        LockLease::new(LockActor::Desktop, "split", "parent-lock"),
    )
    .unwrap();
    let (code, stdout, _) = run_cli(root, &["--json", "run", "sample"]);
    assert_eq!(
        code, 3,
        "expected exit 3 WORKSPACE_LOCKED, got {code}: {stdout}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["code"],
        "WORKSPACE_LOCKED"
    );
    let (code, stdout, _) = run_cli(root, &["--json", "clean", "--force", "sample"]);
    assert_eq!(code, 3, "locked clean must exit 3, got {code}: {stdout}");
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["code"],
        "WORKSPACE_LOCKED"
    );
    drop(_held);

    // 13. Strict v1 schema round-trip validation (`#[serde(deny_unknown_fields)]`) across all
    // `.vsplit/` artifacts produced by the CLI so workspace state stays strictly v1-compatible.
    {
        use vsplit_core::schemas::cuts::CutsJson;
        use vsplit_core::schemas::index::Index;
        use vsplit_core::schemas::manifest::{Manifest, StageStatus};
        use vsplit_core::schemas::plan::CutsResolvedJson;
        use vsplit_core::schemas::scenes::ScenesJson;

        let idx: Index =
            serde_json::from_str(&fs::read_to_string(root.join(".vsplit/index.json")).unwrap())
                .unwrap();
        idx.validate().unwrap();
        assert!(idx.workspaces.contains_key("sample"));

        let ws = root.join(".vsplit/workspaces/sample");
        let manifest: Manifest =
            serde_json::from_str(&fs::read_to_string(ws.join("manifest.json")).unwrap()).unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.id, "sample");
        assert_eq!(manifest.stage("verify").status, StageStatus::Done);

        let scenes: ScenesJson =
            serde_json::from_str(&fs::read_to_string(ws.join("detect/scenes.json")).unwrap())
                .unwrap();
        assert_eq!(scenes.scenes.len(), 3);

        let cuts: CutsJson =
            serde_json::from_str(&fs::read_to_string(ws.join("decisions/cuts.json")).unwrap())
                .unwrap();
        cuts.validate().unwrap();
        assert_eq!(cuts.scenes.len(), 3);

        let plan: CutsResolvedJson =
            serde_json::from_str(&fs::read_to_string(ws.join("plan/cuts-resolved.json")).unwrap())
                .unwrap();
        assert_eq!(plan.scenes.len(), 3);
    }
}

#[test]
fn cli_run_exits_one_on_stage_failure() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let clip = root.join("broken.mp4");
    fs::write(&clip, b"not a valid video file").unwrap();

    let (code, _, _) = run_cli(root, &["--json", "scan"]);
    assert_eq!(code, 0);

    let (code, stdout, _) = run_cli(root, &["--json", "run"]);
    assert_eq!(
        code, 1,
        "expected exit 1 on failed stage, got {code}: {stdout}"
    );
    let val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], false);
    assert_eq!(val["code"], "STAGE_FAILED");
}

#[test]
fn rename_invalid_format_exits_usage_error_json() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let (code, stdout, _) = run_cli(root, &["--json", "rename", "--format", "%Y:%m"]);
    assert_eq!(
        code, 2,
        "expected exit 2 INVALID_ARGUMENT, got {code}: {stdout}"
    );
    let val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], false);
    assert_eq!(val["code"], "INVALID_ARGUMENT");
}

#[test]
fn rename_empty_dir_dry_run_json_ok() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let (code, stdout, _) = run_cli(root, &["--json", "rename", "--dry-run"]);
    assert_eq!(code, 0, "rename --dry-run failed: {stdout}");
    let val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["ok"], true);
    assert_eq!(val["dry_run"], true);
    assert_eq!(val["counts"]["renamed"], 0);
    assert_eq!(val["counts"]["workspaces_synced"], 0);
    assert_eq!(val["counts"]["skipped_already_named"], 0);
    assert_eq!(val["counts"]["skipped_no_date"], 0);
    assert_eq!(val["counts"]["skipped_locked"], 0);
    assert_eq!(val["counts"]["skipped_error"], 0);
    assert!(val["renamed"].as_array().unwrap().is_empty());
    assert!(val["skipped"].as_array().unwrap().is_empty());
    assert!(val["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn rename_cli_dry_run_and_apply_with_glob_and_workspace_sync() {
    let tc = cached_toolchain();
    let Ok(ffmpeg) = tc.ffmpeg_path() else {
        return;
    };
    if tc.ffprobe_path().is_err() {
        return;
    }

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let src_clip = root.join("IMG_0042.MOV");
    let expected_clip = root.join("2025-08-20 11-22-33.MOV");

    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-v",
            "error",
            "-i",
            &fixture_clip("short.mp4").to_string_lossy(),
            "-c",
            "copy",
            "-map_metadata",
            "-1",
            "-movflags",
            "use_metadata_tags",
            "-metadata",
            "com.apple.quicktime.creationdate=2025-08-20T11:22:33+0800",
            &src_clip.to_string_lossy(),
        ])
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success(), "ffmpeg failed creating {src_clip:?}");

    // Scan to adopt IMG_0042.MOV into a workspace
    let (code, stdout, _) = run_cli(root, &["--json", "scan"]);
    assert_eq!(code, 0, "scan failed: {stdout}");
    let scan_val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(scan_val["ok"], true);
    assert_eq!(scan_val["adopted"].as_array().unwrap().len(), 1);

    // Dry-run rename with --glob "IMG*"
    let (code, stdout, _) = run_cli(root, &["--json", "rename", "--glob", "IMG*", "--dry-run"]);
    assert_eq!(code, 0, "rename --dry-run failed: {stdout}");
    let dry_val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(dry_val["ok"], true);
    assert_eq!(dry_val["dry_run"], true);
    assert_eq!(dry_val["counts"]["renamed"], 1);
    assert_eq!(dry_val["renamed"][0]["old_path"], "IMG_0042.MOV");
    assert_eq!(dry_val["renamed"][0]["new_path"], "2025-08-20 11-22-33.MOV");
    assert!(src_clip.exists(), "dry-run must not rename source file");
    assert!(
        !expected_clip.exists(),
        "dry-run must not create target file"
    );

    // Apply rename with --glob "IMG*"
    let (code, stdout, _) = run_cli(root, &["--json", "rename", "--glob", "IMG*"]);
    assert_eq!(code, 0, "rename apply failed: {stdout}");
    let apply_val: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(apply_val["ok"], true);
    assert_eq!(apply_val["dry_run"], false);
    assert_eq!(apply_val["counts"]["renamed"], 1);
    assert_eq!(apply_val["counts"]["workspaces_synced"], 1);
    assert_eq!(apply_val["renamed"][0]["new_slug"], "2025-08-20-11-22-33");
    assert!(
        expected_clip.exists(),
        "renamed clip should exist at {expected_clip:?}"
    );
    assert!(
        !src_clip.exists(),
        "original clip should have been moved away from {src_clip:?}"
    );

    // Status reflects synced workspace slug
    let (code, stdout, _) = run_cli(root, &["--json", "status"]);
    assert_eq!(code, 0, "status failed: {stdout}");
    let st: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(st["ok"], true);
    assert_eq!(st["workspaces"][0]["slug"], "2025-08-20-11-22-33");
}
