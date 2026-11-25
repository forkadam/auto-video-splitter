use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use vsplit_core::locks::{try_acquire_workspace_lock, LockActor, LockLease};
use vsplit_server::start_server;

fn fixture_clip(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("fixtures")
        .join(name)
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

#[tokio::test]
async fn auth_origin_media_206_cuts_409_and_unignore_contract() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::copy(fixture_clip("short.mp4"), root.join("clip.mp4")).unwrap();

    let handle = start_server(None).await.expect("start_server");
    let tok = handle.bootstrap.token.clone();
    let addr = handle
        .bootstrap
        .base_url
        .strip_prefix("http://")
        .unwrap()
        .to_string();

    // 1. Missing token -> 401
    let (status, _, _) = http_req(&addr, "GET", "/api/health", &[], None).await;
    assert_eq!(status, 401);

    // 2. Foreign Origin even with valid token -> 403 CSRF rejection
    let (status, _, _) = http_req(
        &addr,
        "GET",
        "/api/health",
        &[("X-Vsplit-Token", &tok), ("Origin", "https://evil.example")],
        None,
    )
    .await;
    assert_eq!(status, 403);

    // 3. Allowed Tauri Origin + valid token -> 200
    let (status, _, body) = http_req(
        &addr,
        "GET",
        "/api/health",
        &[
            ("X-Vsplit-Token", &tok),
            ("Origin", "http://tauri.localhost"),
        ],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("\"ok\":true"));

    // 4. Open project folder -> 200
    let open_body = json!({ "path": root.display().to_string() }).to_string();
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/project/open",
        &[("X-Vsplit-Token", &tok)],
        Some(&open_body),
    )
    .await;
    assert_eq!(
        status,
        200,
        "open project failed: {}",
        String::from_utf8_lossy(&body)
    );

    // 5. Scan -> adopts `clip`
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/scan",
        &[("X-Vsplit-Token", &tok)],
        Some("{}"),
    )
    .await;
    assert_eq!(
        status,
        200,
        "scan failed: {}",
        String::from_utf8_lossy(&body)
    );
    let scan_val: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(scan_val["adopted"][0]["slug"], "clip");

    // 6. Ignore / Unignore response key fix (`{"unignored": 1}`)
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/workspaces/clip/ignore",
        &[("X-Vsplit-Token", &tok)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["ignored"],
        1
    );

    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/workspaces/clip/unignore",
        &[("X-Vsplit-Token", &tok)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["unignored"],
        1
    );

    // 7. Run through review (generates preview/source-proxy.mp4 and decisions/cuts.json)
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/workspaces/clip/run",
        &[("X-Vsplit-Token", &tok)],
        Some(r#"{"through":"review"}"#),
    )
    .await;
    assert_eq!(
        status,
        200,
        "run failed: {}",
        String::from_utf8_lossy(&body)
    );

    // 8. HTTP 206 Range request on `/m/{token}/workspaces/clip/preview/source-proxy.mp4`
    let (status, hdrs, body) = http_req(
        &addr,
        "GET",
        &format!("/m/{tok}/workspaces/clip/preview/source-proxy.mp4"),
        &[("Range", "bytes=0-1023")],
        None,
    )
    .await;
    assert_eq!(status, 206, "expected 206, got {status}\n{hdrs}");
    assert_eq!(body.len(), 1024);
    assert!(hdrs
        .to_ascii_lowercase()
        .contains("content-range: bytes 0-1023/"));

    // 9. GET cuts + PUT cuts with bad expected_hash -> 409 Conflict
    let (status, _, body) = http_req(
        &addr,
        "GET",
        "/api/workspaces/clip/cuts",
        &[("X-Vsplit-Token", &tok)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let cuts_res: Value = serde_json::from_slice(&body).unwrap();
    let bad_put = json!({
        "cuts": cuts_res["cuts"],
        "expected_hash": "deadbeef"
    })
    .to_string();
    let (status, _, _) = http_req(
        &addr,
        "PUT",
        "/api/workspaces/clip/cuts",
        &[("X-Vsplit-Token", &tok)],
        Some(&bad_put),
    )
    .await;
    assert_eq!(status, 409);

    // 10. External CLI lock causes HTTP 409 on mutating routes
    let _cli_lock = try_acquire_workspace_lock(
        root,
        "clip",
        LockLease::new(LockActor::Cli, "split", "ext-1"),
    )
    .unwrap();
    let (status, _, _) = http_req(
        &addr,
        "POST",
        "/api/workspaces/clip/clean",
        &[("X-Vsplit-Token", &tok)],
        None,
    )
    .await;
    assert_eq!(status, 409);
    drop(_cli_lock);

    // 11. /api/jobs/cancel + /api/doctor
    let (status, _, body) = http_req(
        &addr,
        "POST",
        "/api/jobs/cancel",
        &[("X-Vsplit-Token", &tok)],
        Some("{}"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["cancelled"],
        0
    );

    let (status, _, body) = http_req(
        &addr,
        "GET",
        "/api/doctor",
        &[("X-Vsplit-Token", &tok)],
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["ok"], true);
}
