use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tokio::sync::mpsc;

use crate::paths::{locks_dir, working_state_dir, workspace_dir, workspaces_root};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    IndexChanged,
    ConfigChanged,
    ManifestChanged { slug: String },
    CutsChanged { slug: String },
    LockChanged { slug: String },
    PollingFallback { reason: String },
    WatcherError { message: String },
}

/// Non-recursive debounced watcher scoped exclusively to `.vsplit/` metadata directories
/// (never watches `preview/images/` or `output/`). Falls back to bounded polling on `ENOSPC`.
pub struct WorkspaceWatcher {
    _native: Option<Debouncer<notify::RecommendedWatcher>>,
    stop_poll: Option<tokio::sync::watch::Sender<bool>>,
    suppressed_slugs: HashMap<String, usize>,
}

impl Drop for WorkspaceWatcher {
    fn drop(&mut self) {
        if let Some(ref tx) = self.stop_poll {
            let _ = tx.send(true);
        }
    }
}

fn classify_path(working_root: &Path, path: &Path) -> Option<WatchEvent> {
    let vsplit_dir = working_state_dir(working_root);
    let rel = path.strip_prefix(&vsplit_dir).ok()?;
    let comps: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    match comps.as_slice() {
        ["index.json"] => Some(WatchEvent::IndexChanged),
        ["config.yaml"] => Some(WatchEvent::ConfigChanged),
        [".locks", name] => {
            let slug = name
                .strip_suffix(".lease.json")
                .or_else(|| name.strip_suffix(".lock"))?;
            if slug == "index" {
                None
            } else {
                Some(WatchEvent::LockChanged {
                    slug: slug.to_string(),
                })
            }
        }
        ["workspaces", slug, "manifest.json"] => Some(WatchEvent::ManifestChanged {
            slug: (*slug).to_string(),
        }),
        ["workspaces", slug, "decisions", "cuts.json"] => Some(WatchEvent::CutsChanged {
            slug: (*slug).to_string(),
        }),
        _ => None,
    }
}

fn collect_watch_dirs(
    working_root: &Path,
    suppressed_slugs: &HashMap<String, usize>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let vsplit = working_state_dir(working_root);
    if vsplit.is_dir() {
        dirs.push(vsplit);
    }
    let locks = locks_dir(working_root);
    if locks.is_dir() {
        dirs.push(locks);
    }
    let ws_root = workspaces_root(working_root);
    if ws_root.is_dir() {
        dirs.push(ws_root.clone());
        if let Ok(entries) = fs::read_dir(&ws_root) {
            for entry in entries.flatten() {
                if let Some(slug) = entry.file_name().to_str() {
                    if suppressed_slugs.get(slug).copied().unwrap_or(0) > 0 {
                        continue;
                    }
                }
                let p = entry.path();
                if p.is_dir() {
                    dirs.push(p.clone());
                    let dec = p.join("decisions");
                    if dec.is_dir() {
                        dirs.push(dec);
                    }
                }
            }
        }
    }
    dirs
}

fn snapshot_mtimes(working_root: &Path) -> HashMap<PathBuf, SystemTime> {
    let mut snap = HashMap::new();
    let vsplit = working_state_dir(working_root);
    for file in [vsplit.join("index.json"), vsplit.join("config.yaml")] {
        if let Ok(m) = fs::metadata(&file).and_then(|md| md.modified()) {
            snap.insert(file, m);
        }
    }
    if let Ok(entries) = fs::read_dir(locks_dir(working_root)) {
        for e in entries.flatten() {
            let p = e.path();
            if let Ok(m) = e.metadata().and_then(|md| md.modified()) {
                snap.insert(p, m);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(workspaces_root(working_root)) {
        for e in entries.flatten() {
            let ws = e.path();
            if !ws.is_dir() {
                continue;
            }
            for f in [
                ws.join("manifest.json"),
                ws.join("decisions").join("cuts.json"),
            ] {
                if let Ok(m) = fs::metadata(&f).and_then(|md| md.modified()) {
                    snap.insert(f, m);
                }
            }
        }
    }
    snap
}

fn spawn_polling_fallback(
    working_root: PathBuf,
    tx: mpsc::UnboundedSender<WatchEvent>,
    reason: String,
) -> tokio::sync::watch::Sender<bool> {
    let _ = tx.send(WatchEvent::PollingFallback { reason });
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let mut prev = snapshot_mtimes(&working_root);
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        break;
                    }
                }
                _ = interval.tick() => {
                    let curr = snapshot_mtimes(&working_root);
                    let mut changed_paths = Vec::new();
                    for (p, mt) in &curr {
                        if prev.get(p) != Some(mt) {
                            changed_paths.push(p.clone());
                        }
                    }
                    for p in prev.keys() {
                        if !curr.contains_key(p) {
                            changed_paths.push(p.clone());
                        }
                    }
                    for p in changed_paths {
                        if let Some(ev) = classify_path(&working_root, &p) {
                            let _ = tx.send(ev);
                        }
                    }
                    prev = curr;
                }
            }
        }
    });
    stop_tx
}

impl WorkspaceWatcher {
    pub fn start(working_root: PathBuf, tx: mpsc::UnboundedSender<WatchEvent>) -> Self {
        let watched = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));
        let root_cb = working_root.clone();
        let tx_cb = tx.clone();

        let debouncer_res = new_debouncer(
            Duration::from_millis(100),
            move |res: DebounceEventResult| match res {
                Ok(events) => {
                    let mut emitted = Vec::new();
                    for ev in events {
                        if let Some(classified) = classify_path(&root_cb, &ev.path) {
                            if !emitted.contains(&classified) {
                                emitted.push(classified.clone());
                                let _ = tx_cb.send(classified);
                            }
                        }
                    }
                }
                Err(err) => {
                    let _ = tx_cb.send(WatchEvent::WatcherError {
                        message: err.to_string(),
                    });
                }
            },
        );

        let suppressed_slugs = HashMap::new();

        let mut debouncer = match debouncer_res {
            Ok(d) => d,
            Err(err) => {
                let stop_tx = spawn_polling_fallback(working_root, tx, err.to_string());
                return Self {
                    _native: None,
                    stop_poll: Some(stop_tx),
                    suppressed_slugs,
                };
            }
        };

        // Ensure .vsplit/ exists so we can watch it non-recursively.
        let vsplit = working_state_dir(&working_root);
        let _ = fs::create_dir_all(&vsplit);

        let dirs = collect_watch_dirs(&working_root, &suppressed_slugs);
        for d in dirs {
            if let Err(err) = debouncer.watcher().watch(&d, RecursiveMode::NonRecursive) {
                let msg = err.to_string();
                let stop_tx = spawn_polling_fallback(working_root, tx, msg);
                return Self {
                    _native: None,
                    stop_poll: Some(stop_tx),
                    suppressed_slugs,
                };
            }
            watched.lock().unwrap().insert(d);
        }

        Self {
            _native: Some(debouncer),
            stop_poll: None,
            suppressed_slugs,
        }
    }

    /// Register any newly-created workspace or `decisions/` directories non-recursively,
    /// skipping workspaces that are currently suppressed.
    pub fn sync_watches(&mut self, working_root: &Path) {
        let Some(ref mut debouncer) = self._native else {
            return;
        };
        for d in collect_watch_dirs(working_root, &self.suppressed_slugs) {
            let _ = debouncer.watcher().watch(&d, RecursiveMode::NonRecursive);
        }
    }

    /// Increment ref-counted suppression for the given workspace slugs and unwatch their
    /// directories (`<ws>/decisions` and `<ws>`) so Windows `ReadDirectoryChangesW` handles
    /// do not block `fs::rename` and concurrent `sync_watches()` calls will not re-arm them.
    pub fn suppress_workspaces(&mut self, working_root: &Path, slugs: &[String]) {
        let mut unique: Vec<&str> = slugs.iter().map(String::as_str).collect();
        unique.sort_unstable();
        unique.dedup();
        for slug in unique {
            *self.suppressed_slugs.entry(slug.to_string()).or_insert(0) += 1;
            if let Some(ref mut debouncer) = self._native {
                let ws = workspace_dir(working_root, slug);
                let _ = debouncer.watcher().unwatch(&ws.join("decisions"));
                let _ = debouncer.watcher().unwatch(&ws);
            }
        }
    }

    /// Decrement ref-counted suppression for the given workspace slugs (removing entries at 0)
    /// and re-synchronize watches so resumed or renamed workspaces are re-armed.
    pub fn resume_workspaces(&mut self, working_root: &Path, slugs: &[String]) {
        let mut unique: Vec<&str> = slugs.iter().map(String::as_str).collect();
        unique.sort_unstable();
        unique.dedup();
        for slug in unique {
            if let Some(count) = self.suppressed_slugs.get_mut(slug) {
                if *count <= 1 {
                    self.suppressed_slugs.remove(slug);
                } else {
                    *count -= 1;
                }
            }
        }
        self.sync_watches(working_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn collect_watch_dirs_skips_suppressed_slugs_and_decisions_subdirs() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let ws_a = workspace_dir(root, "alpha");
        let ws_b = workspace_dir(root, "beta");
        fs::create_dir_all(ws_a.join("decisions")).unwrap();
        fs::create_dir_all(ws_b.join("decisions")).unwrap();
        fs::create_dir_all(locks_dir(root)).unwrap();

        let mut suppressed = HashMap::new();
        suppressed.insert("alpha".to_string(), 1);
        suppressed.insert("zero-count".to_string(), 0);

        let dirs = collect_watch_dirs(root, &suppressed);
        assert!(dirs.contains(&working_state_dir(root)));
        assert!(dirs.contains(&locks_dir(root)));
        assert!(dirs.contains(&workspaces_root(root)));
        assert!(dirs.contains(&ws_b));
        assert!(dirs.contains(&ws_b.join("decisions")));
        assert!(!dirs.contains(&ws_a));
        assert!(!dirs.contains(&ws_a.join("decisions")));
    }

    #[test]
    fn suppress_and_resume_workspaces_is_refcounted() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let mut watcher = WorkspaceWatcher {
            _native: None,
            stop_poll: None,
            suppressed_slugs: HashMap::new(),
        };

        let slugs = vec!["alpha".to_string(), "alpha".to_string()];
        watcher.suppress_workspaces(root, &slugs);
        assert_eq!(watcher.suppressed_slugs.get("alpha"), Some(&1));

        watcher.suppress_workspaces(root, &["alpha".to_string()]);
        assert_eq!(watcher.suppressed_slugs.get("alpha"), Some(&2));

        watcher.resume_workspaces(root, &slugs);
        assert_eq!(watcher.suppressed_slugs.get("alpha"), Some(&1));

        watcher.resume_workspaces(root, &["alpha".to_string()]);
        assert!(!watcher.suppressed_slugs.contains_key("alpha"));
    }
}
