use std::path::{Path, PathBuf};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use vsplit_server::start_spike_server;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
        .join("fixtures")
}

async fn send_http(addr: &str, request: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("failed to connect to embedded axum server");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("failed to write request");
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .expect("failed to read response");

    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("missing HTTP header terminator");
    let header_str = String::from_utf8_lossy(&buf[..split]).into_owned();
    let body = buf[split + 4..].to_vec();
    let status: u16 = header_str
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("missing HTTP status line");
    (status, header_str, body)
}

#[tokio::test]
async fn gate_b_loopback_auth_and_http_206_range() {
    let fixtures = fixtures_dir();
    assert!(
        fixtures.join("short.mp4").exists(),
        "missing tests/fixtures/short.mp4"
    );

    let boot = start_spike_server(fixtures)
        .await
        .expect("start_spike_server failed");
    assert_eq!(boot.token.len(), 64, "token must be 256-bit hex (64 chars)");
    let addr = boot
        .base_url
        .strip_prefix("http://")
        .expect("expected http:// base_url");

    // 1. /api/health without token -> 401
    let (status, _, _) = send_http(
        addr,
        &format!("GET /api/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert_eq!(status, 401);

    // 2. /api/health with X-Vsplit-Token -> 200 {"ok":true}
    let (status, _, body) = send_http(
        addr,
        &format!(
            "GET /api/health HTTP/1.1\r\nHost: {addr}\r\nX-Vsplit-Token: {}\r\nConnection: close\r\n\r\n",
            boot.token
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        String::from_utf8_lossy(&body).contains("\"ok\":true"),
        "unexpected health body: {}",
        String::from_utf8_lossy(&body)
    );

    // 3. Wrong token on media route -> 401
    let (status, _, _) = send_http(
        addr,
        &format!(
            "GET /m/wrongtoken/media/short.mp4 HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        ),
    )
    .await;
    assert_eq!(status, 401);

    // 4. Path traversal attempt -> 400 or 404
    let (status, _, _) = send_http(
        addr,
        &format!(
            "GET /m/{}/media/..%2F..%2FCargo.toml HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n",
            boot.token
        ),
    )
    .await;
    assert!(
        status == 400 || status == 404,
        "expected 400/404 for traversal, got {status}"
    );

    // 5. HTTP 206 Partial Content Range request on short.mp4
    let (status, headers, body) = send_http(
        addr,
        &format!(
            "GET /m/{}/media/short.mp4 HTTP/1.1\r\nHost: {addr}\r\nRange: bytes=0-1023\r\nConnection: close\r\n\r\n",
            boot.token
        ),
    )
    .await;
    assert_eq!(
        status, 206,
        "expected 206 Partial Content, headers:\n{headers}"
    );
    assert_eq!(body.len(), 1024, "expected exact 1024-byte range slice");
    let headers_lower = headers.to_ascii_lowercase();
    assert!(
        headers_lower.contains("content-range: bytes 0-1023/"),
        "missing Content-Range header:\n{headers}"
    );
    assert!(
        headers_lower.contains("accept-ranges: bytes"),
        "missing Accept-Ranges header:\n{headers}"
    );
}
