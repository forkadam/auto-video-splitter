use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Local};
use serde_json::json;
use tempfile::TempDir;
use vsplit_core::hash::partial_hash;
use vsplit_core::index::load_index;
use vsplit_core::locks::{try_acquire_workspace_lock, LockActor, LockLease};
use vsplit_core::manifest::{load_manifest, save_manifest};
use vsplit_core::paths::workspace_dir;
use vsplit_core::rename::{
    apply_rename_plan, plan_renames, rename_videos, RenameError, RenameOptions, RenameSkipReason,
    DEFAULT_RENAME_FORMAT,
};
use vsplit_core::scan::{adopt_or_find, scan_folder, RelocatePolicy, ScanOptions};
use vsplit_core::schemas::config::Config;
use vsplit_core::schemas::manifest::StageStatus;
use vsplit_core::toolchain::{cached_toolchain, MediaToolchain, ToolchainError};

fn fixture_clip(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn make_clip_with_quicktime_date(tc: &MediaToolchain, dst: &Path, qt_date: &str) {
    let ffmpeg = tc.ffmpeg_path().expect("ffmpeg required for test");
    let src = fixture_clip("short.mp4");
    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-v",
            "error",
            "-i",
            &src.to_string_lossy(),
            "-c",
            "copy",
            "-map_metadata",
            "-1",
            "-movflags",
            "use_metadata_tags",
            "-metadata",
            &format!("com.apple.quicktime.creationdate={qt_date}"),
            &dst.to_string_lossy(),
        ])
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success(), "ffmpeg failed creating {dst:?}");
}

fn make_clip_with_creation_time(tc: &MediaToolchain, dst: &Path, creation_time: &str) {
    let ffmpeg = tc.ffmpeg_path().expect("ffmpeg required for test");
    let src = fixture_clip("short.mp4");
    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-v",
            "error",
            "-i",
            &src.to_string_lossy(),
            "-c",
            "copy",
            "-map_metadata",
            "-1",
            "-metadata",
            &format!("creation_time={creation_time}"),
            &dst.to_string_lossy(),
        ])
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success(), "ffmpeg failed creating {dst:?}");
}

fn make_clip_without_date(tc: &MediaToolchain, dst: &Path) {
    let ffmpeg = tc.ffmpeg_path().expect("ffmpeg required for test");
    let src = fixture_clip("short.mp4");
    let status = Command::new(ffmpeg)
        .args([
            "-y",
            "-v",
            "error",
            "-i",
            &src.to_string_lossy(),
            "-c",
            "copy",
            "-map_metadata",
            "-1",
            "-fflags",
            "+bitexact",
            &dst.to_string_lossy(),
        ])
        .status()
        .expect("spawn ffmpeg");
    assert!(status.success(), "ffmpeg failed creating {dst:?}");
}

#[tokio::test]
async fn rename_unadopted_dry_run_glob_duplicates_idempotency_and_fallbacks() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    // Two IMG clips sharing the exact same QuickTime capture second
    make_clip_with_quicktime_date(
        &tc,
        &root.join("IMG_0001.MOV"),
        "2026-05-01T14:23:05+0800",
    );
    make_clip_with_quicktime_date(
        &tc,
        &root.join("IMG_0002.MOV"),
        "2026-05-01T14:23:05+0800",
    );
    // Non-IMG clip with creation_time fallback
    let utc_ts = "2026-06-10T08:30:00Z";
    make_clip_with_creation_time(&tc, &root.join("VID_0003.mp4"), utc_ts);
    // Clip with no creation date metadata
    make_clip_without_date(&tc, &root.join("IMG_NODATE.mp4"));

    // 1) Dry-run with glob "IMG*"
    let dry = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            recursive: Some(false),
            glob: Some("IMG*".to_string()),
            format: None,
            dry_run: true,
        },
        LockActor::Cli,
    )
    .await
    .unwrap();

    assert!(dry.dry_run);
    assert_eq!(dry.format, DEFAULT_RENAME_FORMAT);
    assert_eq!(dry.counts.renamed, 2);
    assert_eq!(dry.counts.workspaces_synced, 0);
    assert_eq!(dry.counts.skipped_no_date, 1);
    assert_eq!(dry.renamed[0].old_path, "IMG_0001.MOV");
    assert_eq!(dry.renamed[0].new_path, "2026-05-01 14-23-05.MOV");
    assert_eq!(
        dry.renamed[0].source_tag,
        "com.apple.quicktime.creationdate"
    );
    assert_eq!(dry.renamed[1].old_path, "IMG_0002.MOV");
    assert_eq!(dry.renamed[1].new_path, "2026-05-01 14-23-05(1).MOV");
    // Dry run must not mutate disk or create .vsplit/
    assert!(root.join("IMG_0001.MOV").is_file());
    assert!(root.join("IMG_0002.MOV").is_file());
    assert!(!root.join(".vsplit").exists());

    // 2) Apply with glob "IMG*"
    let applied = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            recursive: Some(false),
            glob: Some("IMG*".to_string()),
            format: None,
            dry_run: false,
        },
        LockActor::Cli,
    )
    .await
    .unwrap();

    assert!(!applied.dry_run);
    assert_eq!(applied.counts.renamed, 2);
    assert_eq!(applied.counts.skipped_no_date, 1);
    assert!(root.join("2026-05-01 14-23-05.MOV").is_file());
    assert!(root.join("2026-05-01 14-23-05(1).MOV").is_file());
    assert!(root.join("VID_0003.mp4").is_file()); // untouched by IMG* glob
    assert!(root.join("IMG_NODATE.mp4").is_file());
    assert!(!root.join(".vsplit").exists());

    // 3) Second run across all files: IMG files are already_named, VID_0003 renames via creation_time, IMG_NODATE skips
    let second = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            recursive: Some(false),
            glob: None,
            format: None,
            dry_run: false,
        },
        LockActor::Cli,
    )
    .await
    .unwrap();

    let expected_vid_stem = DateTime::parse_from_rfc3339(utc_ts)
        .unwrap()
        .with_timezone(&Local)
        .format(DEFAULT_RENAME_FORMAT)
        .to_string();
    let expected_vid_name = format!("{expected_vid_stem}.mp4");

    assert_eq!(second.counts.renamed, 1);
    assert_eq!(second.counts.skipped_already_named, 2);
    assert_eq!(second.counts.skipped_no_date, 1);
    assert_eq!(second.renamed[0].old_path, "VID_0003.mp4");
    assert_eq!(second.renamed[0].new_path, expected_vid_name);
    assert_eq!(second.renamed[0].source_tag, "creation_time");
    assert!(root.join(&expected_vid_name).is_file());

    // 4) Third run: completely idempotent (0 renamed, 3 already_named, 1 no_date)
    let third = rename_videos(root, &cfg, &tc, &RenameOptions::default(), LockActor::Cli)
        .await
        .unwrap();
    assert_eq!(third.counts.renamed, 0);
    assert_eq!(third.counts.skipped_already_named, 3);
    assert_eq!(third.counts.skipped_no_date, 1);
}

#[tokio::test]
async fn rename_adopted_workspace_sync_preserves_artifacts_and_respects_locks() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    make_clip_with_quicktime_date(
        &tc,
        &root.join("IMG_1001.MOV"),
        "2026-07-04T19:45:12+0800",
    );
    make_clip_with_quicktime_date(
        &tc,
        &root.join("IMG_1002.MOV"),
        "2026-07-04T20:00:00+0800",
    );

    let scan = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(false),
            relocate_policy: RelocatePolicy::Skip,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(scan.adopted.len(), 2);

    // Add decisions/cuts.json and mark `probe` stage Done in img-1001 workspace
    let ws1_old = workspace_dir(root, "img-1001");
    fs::create_dir_all(ws1_old.join("decisions")).unwrap();
    let cuts_payload = r#"{"schema_version":1,"scenes":[]}"#;
    fs::write(ws1_old.join("decisions/cuts.json"), cuts_payload).unwrap();
    let mut m_before = load_manifest(&ws1_old).unwrap();
    m_before.stages.get_mut("probe").unwrap().status = StageStatus::Done;
    save_manifest(&ws1_old, &mut m_before).unwrap();

    // Hold a live workspace lock on sibling img-1002
    let _lock_guard = try_acquire_workspace_lock(
        root,
        "img-1002",
        LockLease::new(LockActor::Desktop, "split", "test-run-1"),
    )
    .unwrap();

    // Verify plan_renames + affected_slugs + apply_rename_plan directly
    let plan = plan_renames(root, &cfg, &tc, &RenameOptions::default())
        .await
        .unwrap();
    assert_eq!(plan.affected_slugs(), vec!["img-1001".to_string()]);

    let report = apply_rename_plan(root, plan, LockActor::Cli).unwrap();

    assert_eq!(report.counts.renamed, 1);
    assert_eq!(report.counts.workspaces_synced, 1);
    assert_eq!(report.counts.skipped_locked, 1);
    assert!(report.warnings.is_empty());

    let item = &report.renamed[0];
    assert_eq!(item.old_path, "IMG_1001.MOV");
    assert_eq!(item.new_path, "2026-07-04 19-45-12.MOV");
    assert_eq!(item.old_slug.as_deref(), Some("img-1001"));
    assert_eq!(item.new_slug.as_deref(), Some("2026-07-04-19-45-12"));

    // Source renamed & old path gone
    assert!(root.join("2026-07-04 19-45-12.MOV").is_file());
    assert!(!root.join("IMG_1001.MOV").exists());

    // Workspace moved & decisions/cuts.json preserved
    let ws1_new = workspace_dir(root, "2026-07-04-19-45-12");
    assert!(!ws1_old.exists());
    assert!(ws1_new.is_dir());
    assert_eq!(
        fs::read_to_string(ws1_new.join("decisions/cuts.json")).unwrap(),
        cuts_payload
    );

    // Manifest updated and stage state preserved
    let manifest = load_manifest(&ws1_new).unwrap();
    assert_eq!(manifest.id, "2026-07-04-19-45-12");
    assert_eq!(manifest.source.relative_path, "2026-07-04 19-45-12.MOV");
    assert_eq!(manifest.stages["probe"].status, StageStatus::Done);

    // Index updated (old slug removed, NOT retired; new slug present)
    let idx = load_index(root).unwrap();
    assert!(!idx.workspaces.contains_key("img-1001"));
    assert!(!idx.retired_slugs.contains(&"img-1001".to_string()));
    assert_eq!(
        idx.workspaces["2026-07-04-19-45-12"].source_relative_path,
        "2026-07-04 19-45-12.MOV"
    );

    // Locked sibling completely untouched
    assert!(root.join("IMG_1002.MOV").is_file());
    assert!(workspace_dir(root, "img-1002").is_dir());
    assert_eq!(
        idx.workspaces["img-1002"].source_relative_path,
        "IMG_1002.MOV"
    );
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path, "IMG_1002.MOV");
    assert_eq!(report.skipped[0].reason, RenameSkipReason::WorkspaceLocked);
}

#[tokio::test]
async fn rename_recursive_subfolders_keeps_clips_in_their_subfolders() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    fs::create_dir_all(root.join("day1")).unwrap();
    fs::create_dir_all(root.join("day2/camA")).unwrap();

    // Same timestamp in two different subfolders should not conflict with each other
    make_clip_with_quicktime_date(
        &tc,
        &root.join("day1/CLIP_A.MOV"),
        "2026-08-15T10:11:12+0800",
    );
    make_clip_with_quicktime_date(
        &tc,
        &root.join("day2/camA/CLIP_B.MOV"),
        "2026-08-15T10:11:12+0800",
    );

    let report = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            recursive: Some(true),
            glob: None,
            format: None,
            dry_run: false,
        },
        LockActor::Cli,
    )
    .await
    .unwrap();

    assert_eq!(report.counts.renamed, 2);
    assert_eq!(report.renamed[0].old_path, "day1/CLIP_A.MOV");
    assert_eq!(report.renamed[0].new_path, "day1/2026-08-15 10-11-12.MOV");
    assert_eq!(report.renamed[1].old_path, "day2/camA/CLIP_B.MOV");
    assert_eq!(
        report.renamed[1].new_path,
        "day2/camA/2026-08-15 10-11-12.MOV"
    );
    assert!(root.join("day1/2026-08-15 10-11-12.MOV").is_file());
    assert!(root.join("day2/camA/2026-08-15 10-11-12.MOV").is_file());
}

#[tokio::test]
async fn rename_cached_probe_ffprobe_json_shortcut_avoids_spawning_ffprobe() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    // Create a clip with NO embedded creation date metadata
    let clip_path = root.join("NO_META_CLIP.mp4");
    make_clip_without_date(&tc, &clip_path);

    let scan = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(false),
            relocate_policy: RelocatePolicy::Skip,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(scan.adopted.len(), 1);
    let slug = &scan.adopted[0].slug;

    // Write synthetic probe/ffprobe.json with a QuickTime creationdate tag
    let probe_dir = workspace_dir(root, slug).join("probe");
    fs::create_dir_all(&probe_dir).unwrap();
    let synthetic_probe = json!({
        "format": {
            "tags": {
                "com.apple.quicktime.creationdate": "2026-09-20T21:30:45+0800"
            }
        }
    });
    fs::write(
        probe_dir.join("ffprobe.json"),
        serde_json::to_string_pretty(&synthetic_probe).unwrap(),
    )
    .unwrap();

    // Pass an empty toolchain (no ffprobe!) to prove cached ffprobe.json is used without spawning ffprobe
    let empty_tc = MediaToolchain {
        ffmpeg: None,
        ffprobe: None,
        encoders: Default::default(),
    };

    let report = rename_videos(
        root,
        &cfg,
        &empty_tc,
        &RenameOptions::default(),
        LockActor::Cli,
    )
    .await
    .unwrap();

    assert_eq!(report.counts.renamed, 1);
    assert_eq!(report.counts.workspaces_synced, 1);
    assert_eq!(report.renamed[0].old_path, "NO_META_CLIP.mp4");
    assert_eq!(report.renamed[0].new_path, "2026-09-20 21-30-45.mp4");
    assert_eq!(
        report.renamed[0].source_tag,
        "com.apple.quicktime.creationdate"
    );
    assert!(root.join("2026-09-20 21-30-45.mp4").is_file());
    assert!(workspace_dir(root, "2026-09-20-21-30-45").is_dir());

    // Sanity-check invalid format & missing ffprobe fast-fail error paths
    let bad_fmt = rename_videos(
        root,
        &cfg,
        &empty_tc,
        &RenameOptions {
            format: Some("%Y:%m:%d".to_string()),
            ..RenameOptions::default()
        },
        LockActor::Cli,
    )
    .await;
    assert!(matches!(bad_fmt, Err(RenameError::InvalidFormat { .. })));

    // Uncached clip with missing ffprobe fails fast with RenameError::Toolchain(MissingTool("ffprobe"))
    let uncached_dir = TempDir::new().unwrap();
    make_clip_with_quicktime_date(
        &tc,
        &uncached_dir.path().join("UNCACHED.MOV"),
        "2026-09-21T10:00:00+0800",
    );
    let missing_tool = rename_videos(
        uncached_dir.path(),
        &cfg,
        &empty_tc,
        &RenameOptions::default(),
        LockActor::Cli,
    )
    .await;
    assert!(matches!(
        missing_tool,
        Err(RenameError::Toolchain(ToolchainError::MissingTool(
            "ffprobe"
        )))
    ));
}

#[tokio::test]
async fn rename_cycle_swap_and_chain_tail_locked_between_plan_and_apply() {
    let cfg = Config::default();
    let tc = cached_toolchain();

    // Part A: Two-step cycle A -> B, B -> A
    {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let path_a = root.join("2026-05-01 10-00-00.MOV");
        let path_b = root.join("2026-05-01 11-00-00.MOV");
        make_clip_with_quicktime_date(&tc, &path_a, "2026-05-01T11:00:00+0800");
        make_clip_with_quicktime_date(&tc, &path_b, "2026-05-01T10:00:00+0800");

        let hash_a_orig = partial_hash(&path_a).unwrap();
        let hash_b_orig = partial_hash(&path_b).unwrap();
        assert_ne!(hash_a_orig, hash_b_orig);

        let report = rename_videos(root, &cfg, &tc, &RenameOptions::default(), LockActor::Cli)
            .await
            .unwrap();
        assert_eq!(report.counts.renamed, 2);
        assert_eq!(report.counts.skipped_error, 0);
        assert!(report.warnings.is_empty());

        // Bytes must have swapped cleanly with zero data loss
        assert_eq!(partial_hash(&path_b).unwrap(), hash_a_orig);
        assert_eq!(partial_hash(&path_a).unwrap(), hash_b_orig);
    }

    // Part B: Chain A -> B, B -> C where B becomes locked between plan_renames and apply_rename_plan
    {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let path_a = root.join("2026-05-02 10-00-00.MOV");
        let path_b = root.join("2026-05-02 11-00-00.MOV");
        let path_c = root.join("2026-05-02 12-00-00.MOV");
        make_clip_with_quicktime_date(&tc, &path_a, "2026-05-02T11:00:00+0800");
        make_clip_with_quicktime_date(&tc, &path_b, "2026-05-02T12:00:00+0800");

        let hash_a_orig = partial_hash(&path_a).unwrap();
        let hash_b_orig = partial_hash(&path_b).unwrap();

        let scan = scan_folder(root, &cfg, &ScanOptions::default()).unwrap();
        assert_eq!(scan.adopted.len(), 2);

        let plan = plan_renames(root, &cfg, &tc, &RenameOptions::default())
            .await
            .unwrap();
        assert_eq!(plan.affected_slugs().len(), 2);

        // Lock tail B after planning, before applying
        let _tail_lock = try_acquire_workspace_lock(
            root,
            "2026-05-02-11-00-00",
            LockLease::new(LockActor::Desktop, "split", "lock-tail"),
        )
        .unwrap();

        let report = apply_rename_plan(root, plan, LockActor::Cli).unwrap();
        assert_eq!(report.counts.renamed, 0);
        assert_eq!(report.counts.skipped_locked, 1);
        assert_eq!(report.counts.skipped_error, 1);

        // Both original files remain intact at their original paths; C was not created; no leftover temp files
        assert!(path_a.is_file());
        assert!(path_b.is_file());
        assert!(!path_c.exists());
        assert_eq!(partial_hash(&path_a).unwrap(), hash_a_orig);
        assert_eq!(partial_hash(&path_b).unwrap(), hash_b_orig);
        for entry in fs::read_dir(root).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.starts_with(".vsplit-rename-tmp-"), "leftover temp file: {name}");
        }
    }
}

#[tokio::test]
async fn rename_isolates_corrupt_file_ffprobe_errors_from_valid_siblings() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    fs::write(root.join("CORRUPT.mp4"), b"definitely not a valid mp4 stream").unwrap();
    make_clip_with_quicktime_date(&tc, &root.join("VALID.MOV"), "2026-10-01T08:15:30+0800");

    let report = rename_videos(root, &cfg, &tc, &RenameOptions::default(), LockActor::Cli)
        .await
        .unwrap();

    assert_eq!(report.counts.renamed, 1);
    assert_eq!(report.counts.skipped_error, 1);
    assert_eq!(report.renamed[0].old_path, "VALID.MOV");
    assert_eq!(report.renamed[0].new_path, "2026-10-01 08-15-30.MOV");
    assert!(root.join("2026-10-01 08-15-30.MOV").is_file());

    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path, "CORRUPT.mp4");
    assert_eq!(report.skipped[0].reason, RenameSkipReason::Error);
    assert!(
        report.skipped[0]
            .detail
            .as_deref()
            .is_some_and(|d| !d.trim().is_empty()),
        "expected non-empty error detail for corrupt file"
    );
    assert!(root.join("CORRUPT.mp4").is_file());
}

#[tokio::test]
async fn rename_glob_does_not_steal_workspace_of_byte_identical_live_sibling() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    let live_path = root.join("LIVE_A.MOV");
    make_clip_with_quicktime_date(&tc, &live_path, "2026-11-05T16:40:00+0800");
    let slug = adopt_or_find(root, &live_path).unwrap();
    assert_eq!(slug, "live-a");

    // Create a byte-identical unadopted copy matching the glob filter
    let copy_path = root.join("IMG_COPY.MOV");
    fs::copy(&live_path, &copy_path).unwrap();

    let report = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            recursive: Some(false),
            glob: Some("IMG*".to_string()),
            format: None,
            dry_run: false,
        },
        LockActor::Cli,
    )
    .await
    .unwrap();

    assert_eq!(report.counts.renamed, 1);
    assert_eq!(report.counts.workspaces_synced, 0);
    assert_eq!(report.renamed[0].old_path, "IMG_COPY.MOV");
    assert_eq!(report.renamed[0].new_path, "2026-11-05 16-40-00.MOV");
    assert_eq!(report.renamed[0].old_slug, None);
    assert_eq!(report.renamed[0].new_slug, None);

    // Live sibling and its workspace must be completely untouched
    assert!(live_path.is_file());
    assert!(workspace_dir(root, "live-a").is_dir());
    let idx = load_index(root).unwrap();
    assert_eq!(idx.workspaces["live-a"].source_relative_path, "LIVE_A.MOV");
    assert!(!idx.workspaces.contains_key("2026-11-05-16-40-00"));
}

#[tokio::test]
async fn rename_cycle_with_corrupt_manifest_leaves_no_temp_files_and_syncs_healthy_sibling() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let cfg = Config::default();
    let tc = cached_toolchain();

    // Two adopted clips forming a 2-clip swap cycle: A <-> B
    let path_a = root.join("2026-12-01 10-00-00.MOV");
    let path_b = root.join("2026-12-01 11-00-00.MOV");
    make_clip_with_quicktime_date(&tc, &path_a, "2026-12-01T11:00:00+0800");
    make_clip_with_quicktime_date(&tc, &path_b, "2026-12-01T10:00:00+0800");

    let hash_a_orig = partial_hash(&path_a).unwrap();
    let hash_b_orig = partial_hash(&path_b).unwrap();
    assert_ne!(hash_a_orig, hash_b_orig);

    let scan = scan_folder(root, &cfg, &ScanOptions::default()).unwrap();
    assert_eq!(scan.adopted.len(), 2);

    // Corrupt workspace A's manifest.json before renaming
    let ws_a = workspace_dir(root, "2026-12-01-10-00-00");
    let ws_b = workspace_dir(root, "2026-12-01-11-00-00");
    fs::write(ws_a.join("manifest.json"), "{corrupt json").unwrap();

    let report = rename_videos(root, &cfg, &tc, &RenameOptions::default(), LockActor::Cli)
        .await
        .unwrap();

    // Both source video files must have swapped cleanly with zero stranded temp files
    assert_eq!(report.counts.renamed, 2);
    assert_eq!(report.counts.workspaces_synced, 1);
    assert_eq!(partial_hash(&path_b).unwrap(), hash_a_orig);
    assert_eq!(partial_hash(&path_a).unwrap(), hash_b_orig);
    for entry in fs::read_dir(root).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !name.starts_with(".vsplit-rename-tmp-"),
            "leftover temp file: {name}"
        );
    }

    // Warning emitted for corrupt manifest, and corrupt workspace directory was not moved
    assert!(!report.warnings.is_empty());
    assert!(ws_a.is_dir());

    // Healthy sibling B synced cleanly to its new slug (`2026-12-01-10-00-00-2` since `10-00-00` is reserved)
    let b_item = report
        .renamed
        .iter()
        .find(|r| r.old_path == "2026-12-01 11-00-00.MOV")
        .expect("healthy sibling B in renamed list");
    let b_new_slug = b_item.new_slug.as_deref().expect("B has new slug");
    assert_ne!(b_new_slug, "2026-12-01-11-00-00");
    assert!(!ws_b.exists());
    let ws_b_new = workspace_dir(root, b_new_slug);
    assert!(ws_b_new.is_dir());
    let m_b = load_manifest(&ws_b_new).unwrap();
    assert_eq!(m_b.id, b_new_slug);
    assert_eq!(m_b.source.relative_path, "2026-12-01 10-00-00.MOV");

    let idx = load_index(root).unwrap();
    assert_eq!(
        idx.workspaces[b_new_slug].source_relative_path,
        "2026-12-01 10-00-00.MOV"
    );
    assert_eq!(
        idx.workspaces["2026-12-01-10-00-00"].source_relative_path,
        "2026-12-01 11-00-00.MOV"
    );
}
