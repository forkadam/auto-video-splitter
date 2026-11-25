use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::atomic::atomic_write_json;
use crate::paths::{locks_dir, validate_slug, PathError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockActor {
    Desktop,
    Cli,
}

impl LockActor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Cli => "cli",
        }
    }
}

/// Sibling metadata written atomically to `.vsplit/.locks/<slug>.lease.json`.
/// Never locked directly so Windows peer readers never hit `ERROR_LOCK_VIOLATION`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockLease {
    pub pid: u32,
    pub actor: LockActor,
    pub stage: String,
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub version: String,
}

impl LockLease {
    pub fn new(actor: LockActor, stage: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            pid: std::process::id(),
            actor,
            stage: stage.into(),
            run_id: run_id.into(),
            started_at: Utc::now(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[derive(Debug, Error)]
pub enum LockError {
    #[error(transparent)]
    InvalidSlug(#[from] PathError),
    #[error("workspace {slug:?} is locked by {} (pid {}, stage {})", lease.actor.as_str(), lease.pid, lease.stage)]
    Held { slug: String, lease: LockLease },
    #[error("lock I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn normalize_lock_key(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
    match path.file_name() {
        Some(name) => parent.join(name),
        None => parent,
    }
}

fn in_process_workspace_locks() -> &'static Mutex<HashMap<PathBuf, LockLease>> {
    static MAP: OnceLock<Mutex<HashMap<PathBuf, LockLease>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn in_process_index_held() -> &'static Mutex<HashSet<PathBuf>> {
    static SET: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

fn in_process_index_condvar() -> &'static Condvar {
    static CV: OnceLock<Condvar> = OnceLock::new();
    CV.get_or_init(Condvar::new)
}

pub fn index_lock_path(working_root: &Path) -> PathBuf {
    locks_dir(working_root).join("index.lock")
}

pub fn workspace_lock_path(working_root: &Path, slug: &str) -> PathBuf {
    locks_dir(working_root).join(format!("{slug}.lock"))
}

pub fn workspace_lease_path(working_root: &Path, slug: &str) -> PathBuf {
    locks_dir(working_root).join(format!("{slug}.lease.json"))
}

fn open_sentinel(path: &Path) -> Result<File, LockError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| LockError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| LockError::Io {
            path: path.to_path_buf(),
            source: e,
        })
}

fn read_lease_file(path: &Path) -> Option<LockLease> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn fallback_lease() -> LockLease {
    LockLease {
        pid: 0,
        actor: LockActor::Cli,
        stage: "unknown".to_string(),
        run_id: String::new(),
        started_at: Utc::now(),
        version: String::new(),
    }
}

/// Clean owned index lock guard (no unsafe, no leaks).
pub struct OwnedIndexLock {
    file: File,
    key: PathBuf,
}

impl Drop for OwnedIndexLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        if let Ok(mut set) = in_process_index_held().lock() {
            set.remove(&self.key);
        }
        in_process_index_condvar().notify_all();
    }
}

/// Acquire blocking exclusive OS + in-process lock on `.vsplit/.locks/index.lock`.
/// Canonical lock order: ALWAYS acquire `lock_index` BEFORE any `try_acquire_workspace_lock`.
pub fn lock_index(working_root: &Path) -> Result<OwnedIndexLock, LockError> {
    let path = index_lock_path(working_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| LockError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let key = normalize_lock_key(&path);

    {
        let mut held = in_process_index_held().lock().unwrap();
        while held.contains(&key) {
            held = in_process_index_condvar().wait(held).unwrap();
        }
        held.insert(key.clone());
    }

    let file = match open_sentinel(&path) {
        Ok(f) => f,
        Err(e) => {
            in_process_index_held().lock().unwrap().remove(&key);
            in_process_index_condvar().notify_all();
            return Err(e);
        }
    };

    if let Err(e) = file.lock_exclusive() {
        in_process_index_held().lock().unwrap().remove(&key);
        in_process_index_condvar().notify_all();
        return Err(LockError::Io { path, source: e });
    }

    Ok(OwnedIndexLock { file, key })
}

/// Exclusive non-blocking guard on `.vsplit/.locks/<slug>.lock` + atomic `.lease.json` sidecar.
/// Never unlinks `<slug>.lock` on drop (prevents POSIX inode split-brain).
pub struct WorkspaceLockGuard {
    key: PathBuf,
    file: File,
    lease_path: PathBuf,
    lease: LockLease,
}

impl WorkspaceLockGuard {
    pub fn lease(&self) -> &LockLease {
        &self.lease
    }

    pub fn update_stage(&mut self, stage: impl Into<String>) {
        self.lease.stage = stage.into();
        let _ = atomic_write_json(&self.lease_path, &self.lease);
        if let Ok(mut map) = in_process_workspace_locks().lock() {
            map.insert(self.key.clone(), self.lease.clone());
        }
    }
}

impl Drop for WorkspaceLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lease_path);
        let _ = self.file.unlock();
        if let Ok(mut map) = in_process_workspace_locks().lock() {
            map.remove(&self.key);
        }
    }
}

/// Non-blocking exclusive lock acquisition on `.vsplit/.locks/<slug>.lock`.
/// Fails fast with `LockError::Held` if another process or in-process task holds it.
pub fn try_acquire_workspace_lock(
    working_root: &Path,
    slug: &str,
    lease: LockLease,
) -> Result<WorkspaceLockGuard, LockError> {
    validate_slug(slug)?;
    let lock_path = workspace_lock_path(working_root, slug);
    let lease_path = workspace_lease_path(working_root, slug);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| LockError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let key = normalize_lock_key(&lock_path);

    {
        let mut map = in_process_workspace_locks().lock().unwrap();
        if let Some(existing) = map.get(&key) {
            return Err(LockError::Held {
                slug: slug.to_string(),
                lease: existing.clone(),
            });
        }
        map.insert(key.clone(), lease.clone());
    }

    let file = match open_sentinel(&lock_path) {
        Ok(f) => f,
        Err(e) => {
            in_process_workspace_locks().lock().unwrap().remove(&key);
            return Err(e);
        }
    };

    if file.try_lock_exclusive().is_err() {
        in_process_workspace_locks().lock().unwrap().remove(&key);
        let owner = read_lease_file(&lease_path).unwrap_or_else(fallback_lease);
        return Err(LockError::Held {
            slug: slug.to_string(),
            lease: owner,
        });
    }

    // Write sibling lease metadata atomically; peer readers never touch the locked sentinel.
    if let Err(e) = atomic_write_json(&lease_path, &lease) {
        let _ = file.unlock();
        in_process_workspace_locks().lock().unwrap().remove(&key);
        return Err(LockError::Io {
            path: lease_path,
            source: e,
        });
    }

    Ok(WorkspaceLockGuard {
        key,
        file,
        lease_path,
        lease,
    })
}

/// Inspect whether a workspace is actively locked by a live in-process task or peer OS process.
/// Ignores stale `.lease.json` files whose `.lock` sentinel can be acquired cleanly.
pub fn inspect_workspace_lock(working_root: &Path, slug: &str) -> Option<LockLease> {
    validate_slug(slug).ok()?;
    let lock_path = workspace_lock_path(working_root, slug);
    let lease_path = workspace_lease_path(working_root, slug);
    let key = normalize_lock_key(&lock_path);

    if let Ok(map) = in_process_workspace_locks().lock() {
        if let Some(lease) = map.get(&key) {
            return Some(lease.clone());
        }
    }

    if !lock_path.exists() {
        return None;
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .ok()?;
    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = file.unlock();
            None
        }
        Err(_) => Some(read_lease_file(&lease_path).unwrap_or_else(fallback_lease)),
    }
}

/// List all currently active workspace locks under `.vsplit/.locks/`.
pub fn list_active_locks(working_root: &Path) -> HashMap<String, LockLease> {
    let mut out = HashMap::new();
    let dir = locks_dir(working_root);
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str == "index.lock" {
            continue;
        }
        if let Some(slug) = name_str.strip_suffix(".lock") {
            if let Some(lease) = inspect_workspace_lock(working_root, slug) {
                out.insert(slug.to_string(), lease);
            }
        }
    }
    out
}
