use axum::http::{header, HeaderMap, StatusCode};
use rand::RngCore;

/// Generate a 256-bit (64 hex chars) per-launch session token, or honor `VSPLIT_DEV_TOKEN`
/// in debug builds for local Vite HMR development.
pub fn generate_session_token() -> String {
    #[cfg(debug_assertions)]
    if let Ok(dev) = std::env::var("VSPLIT_DEV_TOKEN") {
        let trimmed = dev.trim();
        if trimmed.len() >= 16 {
            return trimmed.to_string();
        }
    }

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Allowed browser/webview `Origin` values (Tauri webview + local Vite dev server).
pub fn is_allowed_origin(origin: &str) -> bool {
    let lower = origin.to_ascii_lowercase();
    lower == "http://tauri.localhost"
        || lower == "https://tauri.localhost"
        || lower == "tauri://localhost"
        || lower == "http://localhost"
        || lower == "http://127.0.0.1"
        || lower.starts_with("http://localhost:")
        || lower.starts_with("http://127.0.0.1:")
}

/// Verify both `X-Vsplit-Token` (401 on mismatch) and `Origin` header (403 on foreign origin).
pub fn verify_api_request(headers: &HeaderMap, expected_token: &str) -> Result<(), StatusCode> {
    if let Some(origin_val) = headers.get(header::ORIGIN) {
        let origin_str = origin_val.to_str().unwrap_or("");
        if !origin_str.is_empty() && !is_allowed_origin(origin_str) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    let provided = headers
        .get("x-vsplit-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq(provided.as_bytes(), expected_token.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

pub fn verify_path_token(provided: &str, expected_token: &str) -> Result<(), StatusCode> {
    if !constant_time_eq(provided.as_bytes(), expected_token.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Redact any `/m/<token>/` path segment before logging.
pub fn redact_token_in_path(path: &str, token: &str) -> String {
    if token.is_empty() {
        return path.to_string();
    }
    path.replace(token, "<redacted>")
}
