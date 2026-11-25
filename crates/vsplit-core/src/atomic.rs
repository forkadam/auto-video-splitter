use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tempfile::NamedTempFile;

const RETRY_INITIAL_MS: u64 = 50;
const RETRY_MAX_ATTEMPTS: usize = 6;

fn is_transient_windows_lock(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied || matches!(err.raw_os_error(), Some(5 | 32 | 33))
}

fn persist_with_retry(mut tmp: NamedTempFile, dst: &Path) -> io::Result<()> {
    let mut delay = Duration::from_millis(RETRY_INITIAL_MS);
    for attempt in 0..RETRY_MAX_ATTEMPTS {
        match tmp.persist(dst) {
            Ok(_) => return Ok(()),
            Err(pe) => {
                if attempt == RETRY_MAX_ATTEMPTS - 1 || !is_transient_windows_lock(&pe.error) {
                    tracing::warn!(
                        path = %dst.display(),
                        attempts = attempt + 1,
                        error = %pe.error,
                        "atomic replace failed after retries"
                    );
                    return Err(pe.error);
                }
                tmp = pe.file;
                thread::sleep(delay);
                delay *= 2;
            }
        }
    }
    unreachable!()
}

pub fn atomic_write_bytes(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all().ok();
    persist_with_retry(tmp, path)
}

/// Write UTF-8 text atomically using LF line endings.
pub fn atomic_write_text(path: &Path, content: &str) -> io::Result<()> {
    let lf = if content.contains("\r\n") {
        content.replace("\r\n", "\n")
    } else {
        content.to_string()
    };
    atomic_write_bytes(path, lf.as_bytes())
}

/// Serialize `data` to pretty JSON (2-space indent + trailing LF) and write atomically.
pub fn atomic_write_json<T: Serialize>(path: &Path, data: &T) -> io::Result<()> {
    let mut s = serde_json::to_string_pretty(data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    s.push('\n');
    atomic_write_text(path, &s)
}

/// Remove a directory tree with exponential backoff for transient Windows file locks.
/// Returns `true` if the directory is gone, `false` if it still exists on disk.
pub fn robust_remove_dir_all(path: &Path) -> bool {
    let mut delay = Duration::from_millis(RETRY_INITIAL_MS);
    for attempt in 0..RETRY_MAX_ATTEMPTS {
        match fs::remove_dir_all(path) {
            Ok(()) => return true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return true,
            Err(e) => {
                if attempt == RETRY_MAX_ATTEMPTS - 1 {
                    tracing::warn!(
                        path = %path.display(),
                        attempts = attempt + 1,
                        error = %e,
                        "robust_remove_dir_all failed after retries"
                    );
                    break;
                }
                thread::sleep(delay);
                delay *= 2;
            }
        }
    }
    !path.exists()
}
