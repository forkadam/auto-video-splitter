use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use tempfile::TempDir;
use vsplit_core::config::{load_resolved_config, write_project_config_raw};
use vsplit_core::manifest::new_manifest;
use vsplit_core::schemas::app_config::AppConfig;
use vsplit_core::schemas::config::Config;
use vsplit_core::schemas::cuts::{format_tc, is_valid_timestamp, parse_tc, CutsJson, Scene};
use vsplit_core::schemas::index::Index;
use vsplit_core::schemas::manifest::{Manifest, SourceInfo, StageBlock};

#[test]
fn manifest_enforces_exact_seven_stage_keys_and_deny_unknown_fields() {
    let mut m = new_manifest(
        "clip-a",
        SourceInfo {
            relative_path: "clip-a.mp4".into(),
            size_bytes: 123,
            mtime: Utc::now(),
            partial_hash: "sha256:abc".into(),
        },
    );
    assert!(m.validate().is_ok());

    // Removing a stage key fails validation
    m.stages.remove("verify");
    assert!(m.validate().is_err());

    // Extra stage key fails validation
    m.stages.insert("verify".into(), StageBlock::default());
    m.stages.insert("bogus".into(), StageBlock::default());
    assert!(m.validate().is_err());

    // Unknown top-level field rejected by #[serde(deny_unknown_fields)]
    let bad_json = serde_json::json!({
        "schema_version": 1,
        "id": "clip-a",
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
        "source": {
            "relative_path": "clip-a.mp4",
            "size_bytes": 1,
            "mtime": Utc::now(),
            "partial_hash": "sha256:00"
        },
        "stages": {
            "probe": {}, "detect": {}, "preview": {}, "review": {},
            "plan": {}, "split": {}, "verify": {}
        },
        "extra_field_not_in_v1": true
    });
    assert!(serde_json::from_value::<Manifest>(bad_json).is_err());

    // Unsupported future schema_version rejected
    let mut future = new_manifest(
        "clip-a",
        SourceInfo {
            relative_path: "clip-a.mp4".into(),
            size_bytes: 1,
            mtime: Utc::now(),
            partial_hash: "sha256:00".into(),
        },
    );
    future.schema_version = 2;
    assert!(future.validate().is_err());
}

#[test]
fn config_validates_concurrency_and_snapshot_shape() {
    let mut cfg = Config::default();
    assert!(cfg.validate().is_ok());

    let snap = cfg.snapshot();
    assert_eq!(snap.len(), 3);
    assert!(snap.contains_key("detect"));
    assert!(snap.contains_key("split"));
    assert_eq!(snap["review"], serde_json::json!({ "default_keep": true }));

    // Rejects `review` in concurrency
    cfg.concurrency.insert("review".into(), 1);
    assert!(cfg.validate().is_err());
    cfg.concurrency.remove("review");

    // Rejects 0 concurrency
    cfg.concurrency.insert("split".into(), 0);
    assert!(cfg.validate().is_err());

    // Rejects unknown field in YAML
    let bad_yaml = "detect:\n  unknown_knob: 123\n";
    assert!(serde_yml::from_str::<Config>(bad_yaml).is_err());
}

#[test]
fn cuts_json_validates_timestamps_and_unique_ids() {
    assert!(is_valid_timestamp("00:00:00.000"));
    assert!(is_valid_timestamp("12:34:56.789"));
    assert!(!is_valid_timestamp("0:00:00.000"));
    assert!(!is_valid_timestamp("00:00:00.00"));
    assert!(!is_valid_timestamp("00:00:00,000"));
    assert_eq!(format_tc(3661.25), "01:01:01.250");
    assert!((parse_tc("01:01:01.250").unwrap() - 3661.25).abs() < 1e-6);

    let ok = CutsJson {
        schema_version: 1,
        scenes: vec![
            Scene {
                id: 1,
                start: "00:00:00.000".into(),
                end: "00:00:02.000".into(),
                keep: true,
                label: None,
                merge_with_next: false,
            },
            Scene {
                id: 2,
                start: "00:00:02.000".into(),
                end: "00:00:04.000".into(),
                keep: false,
                label: Some("drop".into()),
                merge_with_next: false,
            },
        ],
    };
    assert!(ok.validate().is_ok());

    let mut dup = ok.clone();
    dup.scenes[1].id = 1;
    let err = dup.validate().unwrap_err().to_string();
    assert!(err.contains("duplicate scene ids"));

    let mut bad_ts = ok.clone();
    bad_ts.scenes[0].start = "0:00:00.0".into();
    assert!(bad_ts.validate().is_err());

    let mut bad_ver = ok;
    bad_ver.schema_version = 99;
    assert!(bad_ver.validate().is_err());
}

#[test]
fn index_rejects_unknown_fields_and_future_version() {
    let bad =
        r#"{"schema_version":1,"working_root":".","workspaces":{},"retired_slugs":[],"extra":1}"#;
    assert!(serde_json::from_str::<Index>(bad).is_err());
    let idx = Index {
        schema_version: 2,
        ..Default::default()
    };
    assert!(idx.validate().is_err());
}

#[test]
fn config_four_layer_deep_merge_and_write_validation() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".vsplit")).unwrap();
    fs::write(
        root.join(".vsplit/config.yaml"),
        "detect:\n  threshold: 19.5\npreview:\n  proxy_height: null\n",
    )
    .unwrap();
    let explicit = root.join("override.yaml");
    fs::write(&explicit, "detect:\n  luma_only: true\n").unwrap();

    let resolved = load_resolved_config(root, Some(&explicit)).unwrap();
    assert!((resolved.detect.threshold - 19.5).abs() < 1e-9);
    assert!(resolved.detect.luma_only);
    assert_eq!(resolved.preview.proxy_height, None);

    // Invalid YAML rejected before touching .vsplit/config.yaml
    assert!(write_project_config_raw(root, "concurrency:\n  review: 2\n").is_err());
    // Valid update persists
    let updated = write_project_config_raw(root, "split:\n  output_container: mkv\n").unwrap();
    assert_eq!(updated.split.output_container, "mkv");
}

#[test]
fn app_config_recent_folders_deduplicates_and_caps_at_ten() {
    let mut app = AppConfig::default();
    for i in 0..15 {
        app.record_recent_folder(PathBuf::from(format!("/folder-{i}")));
    }
    assert_eq!(app.recent_folders.len(), 10);
    assert_eq!(app.recent_folders[0], PathBuf::from("/folder-14"));
    assert_eq!(app.last_folder, Some(PathBuf::from("/folder-14")));

    app.record_recent_folder(PathBuf::from("/folder-10"));
    assert_eq!(app.recent_folders.len(), 10);
    assert_eq!(app.recent_folders[0], PathBuf::from("/folder-10"));
    let _unused: BTreeMap<String, usize> = BTreeMap::new();
}
