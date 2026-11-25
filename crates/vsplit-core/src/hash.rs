use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use sha2::{Digest, Sha256};

const CHUNK: usize = 1024 * 1024; // 1 MB

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// `sha256(first 1MB + last 1MB + size_bytes_big_endian_8)`. Matches Python `partial_hash`.
pub fn partial_hash(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];

    if size <= (2 * CHUNK) as u64 {
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    } else {
        f.read_exact(&mut buf)?;
        hasher.update(&buf);
        f.seek(SeekFrom::End(-(CHUNK as i64)))?;
        f.read_exact(&mut buf)?;
        hasher.update(&buf);
    }
    hasher.update(size.to_be_bytes());
    Ok(format!("sha256:{}", hex_encode(&hasher.finalize())))
}

/// Full SHA-256 of raw file bytes (`sha256:<hex>`). Matches Python `content_hash`.
pub fn content_hash(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{}", hex_encode(&hasher.finalize())))
}

/// Raw 64-char SHA-256 hex digest of a byte slice (used for `cuts.json` `expected_hash`).
pub fn bytes_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Strip optional `sha256:` prefix and lowercase for hash comparisons.
pub fn normalize_hash_hex(h: &str) -> String {
    h.trim()
        .strip_prefix("sha256:")
        .unwrap_or(h.trim())
        .to_ascii_lowercase()
}
