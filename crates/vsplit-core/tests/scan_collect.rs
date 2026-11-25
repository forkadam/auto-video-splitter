use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use vsplit_core::actions::{ignore_workspaces, unignore_workspaces};
use vsplit_core::collect::collect_outputs;
use vsplit_core::filter::{apply_filters, parse_filter};
use vsplit_core::index::load_index;
use vsplit_core::locks::LockActor;
use vsplit_core::manifest::{load_manifest, save_manifest};
use vsplit_core::paths::workspace_dir;
use vsplit_core::scan::{adopt_or_find, scan_folder, RelocatePolicy, ScanOptions};
use vsplit_core::schemas::config::{CollectMode, Config};
use vsplit_core::schemas::manifest::StageStatus;

fn fixture_clip(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn scan_policies_posix_paths_and_adopt_single_file() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::copy(fixture_clip("short.mp4"), root.join("nested/clip_a.mp4")).unwrap();
    fs::copy(fixture_clip("short.mkv"), root.join("clip_b.mkv")).unwrap();

    let cfg = Config::default();

    // Non-recursive dry-run sees only clip_b.mkv and does not write .vsplit/
    let dry = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(false),
            relocate_policy: RelocatePolicy::Skip,
            dry_run: true,
        },
    )
    .unwrap();
    assert_eq!(dry.adopted.len(), 1);
    assert!(!root.join(".vsplit/index.json").exists());

    // Recursive real scan adopts both with POSIX `/` paths
    let r1 = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(true),
            relocate_policy: RelocatePolicy::Skip,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(r1.adopted.len(), 2);
    let idx = load_index(root).unwrap();
    assert_eq!(
        idx.workspaces["clip-a"].source_relative_path,
        "nested/clip_a.mp4"
    );
    assert!(!idx.workspaces["clip-a"].source_relative_path.contains('\\'));

    // Rename nested/clip_a.mp4 -> nested/renamed_a.mp4
    fs::rename(
        root.join("nested/clip_a.mp4"),
        root.join("nested/renamed_a.mp4"),
    )
    .unwrap();

    // Report policy surfaces pending relocation without writing
    let rep = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(true),
            relocate_policy: RelocatePolicy::Report,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(rep.pending_relocations.len(), 1);
    assert_eq!(rep.relocated.len(), 0);

    // Auto policy updates index + manifest pointers
    let auto = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(true),
            relocate_policy: RelocatePolicy::Auto,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(auto.relocated.len(), 1);
    assert_eq!(
        load_index(root).unwrap().workspaces["clip-a"].source_relative_path,
        "nested/renamed_a.mp4"
    );
    assert_eq!(
        load_manifest(&workspace_dir(root, "clip-a"))
            .unwrap()
            .source
            .relative_path,
        "nested/renamed_a.mp4"
    );

    // Replacing a file at the same path with different content emits warning
    fs::write(root.join("clip_b.mkv"), b"totally different bytes").unwrap();
    let replaced = scan_folder(
        root,
        &cfg,
        &ScanOptions {
            recursive: Some(true),
            relocate_policy: RelocatePolicy::Auto,
            dry_run: false,
        },
    )
    .unwrap();
    assert!(replaced
        .warnings
        .iter()
        .any(|w| w.contains("replaced (hash changed)")));
}

#[test]
fn adopt_or_find_ignores_siblings_and_collect_and_filters_work() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::copy(fixture_clip("short.mp4"), root.join("one.mp4")).unwrap();
    fs::copy(fixture_clip("short.mkv"), root.join("two.mkv")).unwrap();

    let s1 = adopt_or_find(root, &root.join("one.mp4")).unwrap();
    assert_eq!(s1, "one");
    assert_eq!(load_index(root).unwrap().workspaces.len(), 1);

    let s2 = adopt_or_find(root, &root.join("two.mkv")).unwrap();
    assert_eq!(s2, "two");

    // Mark `one` split=done with a dummy output clip; leave `two` at probe=done, detect=pending
    let ws1 = workspace_dir(root, "one");
    let mut m1 = load_manifest(&ws1).unwrap();
    for st in ["probe", "detect", "preview", "review", "plan", "split"] {
        m1.stage_mut(st).status = StageStatus::Done;
    }
    save_manifest(&ws1, &mut m1).unwrap();
    fs::create_dir_all(ws1.join("output")).unwrap();
    fs::write(ws1.join("output/one-highlight-01.mp4"), b"clip-data").unwrap();

    let ws2 = workspace_dir(root, "two");
    let mut m2 = load_manifest(&ws2).unwrap();
    m2.stage_mut("probe").status = StageStatus::Done;
    save_manifest(&ws2, &mut m2).unwrap();

    // Filter validation & evaluation
    assert!(parse_filter("noequals").is_err());
    assert!(parse_filter("bogus=1").is_err());
    assert!(parse_filter("status=bogus").is_err());
    assert!(parse_filter("stage=bogus").is_err());

    let slugs = vec!["one".to_string(), "two".to_string()];
    let p_detect = parse_filter("stage=detect").unwrap();
    assert_eq!(apply_filters(root, &slugs, &[p_detect]), vec!["two"]);
    let p_verify = parse_filter("stage=verify").unwrap();
    assert_eq!(apply_filters(root, &slugs, &[p_verify]), vec!["one"]);

    // Ignore `one` -> collect_all skips ignored
    assert_eq!(
        ignore_workspaces(root, &["one".into()], LockActor::Cli).unwrap(),
        vec!["one"]
    );
    let c_ignored = collect_outputs(root, None, None, CollectMode::Copy, false, false).unwrap();
    assert_eq!(c_ignored.collected, 0);
    assert_eq!(c_ignored.skipped_ignored, 1);
    assert_eq!(c_ignored.skipped_not_done, 1);

    // Unignore `one` -> collect copies clip; second run skips existing unless force=true
    assert_eq!(
        unignore_workspaces(root, &["one".into()], LockActor::Cli).unwrap(),
        vec!["one"]
    );
    let c_copy = collect_outputs(root, None, None, CollectMode::Copy, false, false).unwrap();
    assert_eq!(c_copy.collected, 1);
    assert!(root.join("collected/one-highlight-01.mp4").is_file());

    let c_dup = collect_outputs(root, None, None, CollectMode::Copy, false, false).unwrap();
    assert_eq!(c_dup.collected, 0);
    assert_eq!(c_dup.skipped_existing, 1);

    let c_force = collect_outputs(root, None, None, CollectMode::Copy, true, false).unwrap();
    assert_eq!(c_force.collected, 1);
}
