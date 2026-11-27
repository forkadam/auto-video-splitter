use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use vsplit_core::atomic::atomic_write_json;
use vsplit_core::paths::workspace_dir;
use vsplit_core::scan::{scan_folder, RelocatePolicy, ScanOptions};
use vsplit_core::schemas::config::Config;
use vsplit_core::schemas::cuts::{CutsJson, Scene};
use vsplit_core::toolchain::{cached_toolchain, MediaToolchain};
use vsplit_server::sse::SseEvent;
use vsplit_server::start_server;

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

async fn http_req(
    addr: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to embedded axum server");
    let body_bytes = body.unwrap_or("").as_bytes();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    req.push_str("\r\n");

    stream.write_all(req.as_bytes()).await.unwrap();
    if !body_bytes.is_empty() {
        stream.write_all(body_bytes).await.unwrap();
    }

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP header separator");
    let hdr_str = String::from_utf8_lossy(&buf[..split]).into_owned();
    let body_out = buf[split + 4..].to_vec();
    let status: u16 = hdr_str
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .expect("HTTP status");
    (status, hdr_str, body_out)
}

fn assert_non_empty_detail(body: &[u8]) {
    let val: Value = serde_json::from_slice(body).expect("JSON error response");
    let detail = val["detail"]
        .as_str()
        .expect("expected string 'detail' field in error JSON");
    assert!(
        !detail.trim().is_empty(),
        "expected non-empty 'detail' field, got {val}"
    );
}

#[tokio::test]
async fn rename_api_auth_validation_and_windows_safe_watcher_e2e() {
    // Isolate app.toml so integration tests never touch developer config.
    let cfg_dir = TempDir::new().unwrap();
    std::env::set_var("VSPLIT_CONFIG_DIR", cfg_dir.path());

    // 0. POST /api/rename with no open project -> 400 Bad Request
    let empty_handle = start_server(None).await.expect("start_server(None) failed");
    let empty_addr = empty_handle
        .bootstrap
        .base_url
        .strip_prefix("http://")
        .unwrap()
        .to_string();
    let (status, _, body) = http_req(
        &empty_addr,
        "POST",
        "/api/rename",
        &[("X-Vsplit-Token", &empty_handle.bootstrap.token)],
        Some("{}"),
    )
    .await;
    assert_eq!(
        status,
        400,
        "expected 400 when no project is open, got {status}: {}",
        String::from_utf8_lossy(&body)
    );
    assert_non_empty_detail(&body);

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let tc = cached_toolchain();

    // Prepare IMG_0055.MOV with QuickTime creationdate + other.mp4 to verify glob filtering
    make_clip_with_quicktime_date(
        &tc,
        &root.join("IMG_0055.MOV"),
        "2025-10-05T16:40:12+0800",
    );
    make_clip_with_quicktime_date(&tc, &root.join("other.mp4"), "2025-10-06T09:15:00+0800");

    // Adopt both clips before starting server so WorkspaceWatcher actively watches their workspace dirs
    let scan = scan_folder(
        root,
        &Config::default(),
        &ScanOptions {
            recursive: Some(false),
            relocate_policy: RelocatePolicy::Skip,
            dry_run: false,
        },
    )
    .expect("scan_folder failed");
    assert_eq!(scan.adopted.len(), 2);

    // Ensure decisions/ also exists so OS watcher holds handles on both workspace and decisions/
    fs::create_dir_all(workspace_dir(root, "img-0055").join("decisions")).unwrap();

    let handle = start_server(Some(root.to_path_buf()))
        .await
        .expect("start_server failed");
    let tok = handle.bootstrap.token.clone();
    let addr = handle
        .bootstrap
        .base_url
        .strip_prefix("http://")
        .unwrap()
        .to_string();

    // Give notify watcher a moment to arm OS watches (matching watcher_sse.rs)
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 1. Unauthenticated POST /api/rename -> 401
    let (status, _, _) = http_req(&addr, "POST", "/api/rename", &[], Some("{}")).await;
    assert_eq!(status, 401);

    // 2. Invalid format and empty format -> 422 with non-empty detail
    let bad_fmt = json!({ "format": "%Y:%m", "dry_run": true }).to_string();
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/rename",
        &[("X-Vsplit-Token", &tok)],
        Some(&bad_fmt),
    )
    .await;
    assert_eq!(
        status,
        422,
        "expected 422 for invalid format, got {status}: {}",
        String::from_utf8_lossy(&body)
    );
    assert_non_empty_detail(&body);

    let empty_fmt = json!({ "format": "", "dry_run": true }).to_string();
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/rename",
        &[("X-Vsplit-Token", &tok)],
        Some(&empty_fmt),
    )
    .await;
    assert_eq!(
        status,
        422,
        "expected 422 for empty format, got {status}: {}",
        String::from_utf8_lossy(&body)
    );
    assert_non_empty_detail(&body);

    // 3. Invalid glob -> 422 with non-empty detail
    let bad_glob = json!({ "glob": "[unclosed", "dry_run": true }).to_string();
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/rename",
        &[("X-Vsplit-Token", &tok)],
        Some(&bad_glob),
    )
    .await;
    assert_eq!(
        status,
        422,
        "expected 422 for invalid glob, got {status}: {}",
        String::from_utf8_lossy(&body)
    );
    assert_non_empty_detail(&body);

    // 4a. Dry-run with glob "IMG*" -> 200, counts.renamed == 1, file untouched
    let dry_body = json!({ "glob": "IMG*", "dry_run": true }).to_string();
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/rename",
        &[("X-Vsplit-Token", &tok)],
        Some(&dry_body),
    )
    .await;
    assert_eq!(
        status,
        200,
        "dry-run failed: {}",
        String::from_utf8_lossy(&body)
    );
    let dry_val: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(dry_val["dry_run"], true);
    assert_eq!(dry_val["counts"]["renamed"], 1);
    assert!(root.join("IMG_0055.MOV").is_file());
    assert!(!root.join("2025-10-05 16-40-12.MOV").exists());

    // 4b. Apply with glob "IMG*" -> 200, workspace synced with ZERO warnings despite active watcher
    let apply_body = json!({ "glob": "IMG*", "dry_run": false }).to_string();
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/rename",
        &[("X-Vsplit-Token", &tok)],
        Some(&apply_body),
    )
    .await;
    assert_eq!(
        status,
        200,
        "apply failed: {}",
        String::from_utf8_lossy(&body)
    );
    let apply_val: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(apply_val["dry_run"], false);
    assert_eq!(apply_val["counts"]["renamed"], 1);
    assert_eq!(apply_val["counts"]["workspaces_synced"], 1);
    assert_eq!(
        apply_val["warnings"].as_array().unwrap(),
        &Vec::<Value>::new(),
        "expected empty warnings (WorkspaceWatchGuard must release directory handles), got: {:?}",
        apply_val["warnings"]
    );
    assert!(root.join("2025-10-05 16-40-12.MOV").is_file());
    assert!(!root.join("IMG_0055.MOV").exists());
    assert!(root.join("other.mp4").is_file());

    // 4c. GET /api/workspaces returns 2025-10-05-16-40-12 (and no longer img-0055)
    let (status, _, body) = http_req(
        &addr,
        "GET",
        "/api/workspaces",
        &[("X-Vsplit-Token", &tok)],
        None,
    )
    .await;
    assert_eq!(
        status,
        200,
        "GET /api/workspaces failed: {}",
        String::from_utf8_lossy(&body)
    );
    let ws_val: Value = serde_json::from_slice(&body).unwrap();
    let slugs: Vec<&str> = ws_val["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["slug"].as_str())
        .collect();
    assert!(
        slugs.contains(&"2025-10-05-16-40-12"),
        "expected 2025-10-05-16-40-12 in workspaces, got {slugs:?}"
    );
    assert!(
        !slugs.contains(&"img-0055"),
        "did not expect img-0055 in workspaces, got {slugs:?}"
    );
    assert!(
        slugs.contains(&"other"),
        "expected untouched 'other' workspace, got {slugs:?}"
    );

    // 4d. Verify WorkspaceWatchGuard::drop re-armed the watcher on the newly renamed slug
    let mut rx = handle.state.broker.subscribe();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let new_ws = workspace_dir(root, "2025-10-05-16-40-12");
    let cuts = CutsJson {
        schema_version: 1,
        scenes: vec![Scene {
            id: 1,
            start: "00:00:00.000".into(),
            end: "00:00:01.000".into(),
            keep: true,
            label: Some("rearmed".into()),
            merge_with_next: false,
        }],
    };
    atomic_write_json(&new_ws.join("decisions/cuts.json"), &cuts).unwrap();

    let mut saw_cuts = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) = timeout(Duration::from_millis(400), rx.recv()).await {
            if matches!(ev, SseEvent::CutsChanged { ref slug, .. } if slug == "2025-10-05-16-40-12")
            {
                saw_cuts = true;
                break;
            }
        }
    }
    assert!(
        saw_cuts,
        "expected CutsChanged SSE event on renamed slug 2025-10-05-16-40-12 after WorkspaceWatchGuard drop"
    );
}
