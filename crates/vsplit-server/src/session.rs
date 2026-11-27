use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use vsplit_core::config::{load_app_config, save_app_config};
use vsplit_core::hash::bytes_sha256_hex;
use vsplit_core::index::load_index;
use vsplit_core::jobs::{JobCoordinator, JobEvent};
use vsplit_core::locks::inspect_workspace_lock;
use vsplit_core::manifest::load_manifest;
use vsplit_core::paths::workspace_dir;
use vsplit_core::schemas::app_config::clean_display_path;
use vsplit_core::watcher::{WatchEvent, WorkspaceWatcher};

use crate::sse::{EventBroker, SseEvent};

struct ActiveWatch {
    watch_id: u64,
    watcher: WorkspaceWatcher,
    task: JoinHandle<()>,
}

struct ProjectInner {
    working_root: Option<PathBuf>,
    watch: Option<ActiveWatch>,
    next_watch_id: u64,
}

/// RAII guard that keeps a set of workspace slugs unwatched and suppressed from
/// `sync_watches()` until dropped (even on error, panic, or task cancellation).
pub struct WorkspaceWatchGuard {
    project: Arc<Mutex<ProjectInner>>,
    working_root: PathBuf,
    watch_id: Option<u64>,
    slugs: Vec<String>,
}

impl Drop for WorkspaceWatchGuard {
    fn drop(&mut self) {
        let Some(captured_id) = self.watch_id else {
            return;
        };
        let mut guard = self.project.lock().unwrap_or_else(|e| e.into_inner());
        if guard.working_root.as_deref() == Some(self.working_root.as_path()) {
            if let Some(ref mut w) = guard.watch {
                if w.watch_id == captured_id {
                    w.watcher.resume_workspaces(&self.working_root, &self.slugs);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub broker: EventBroker,
    pub coordinator: JobCoordinator,
    project: Arc<Mutex<ProjectInner>>,
}

#[derive(Debug)]
pub enum OpenProjectError {
    ActiveJobs(usize),
    InvalidPath(String),
}

impl AppState {
    pub fn new(token: String, initial_root: Option<PathBuf>) -> Self {
        let broker = EventBroker::new();
        let coordinator = JobCoordinator::new();

        // Forward JobCoordinator lifecycle events to SSE broker.
        let mut job_rx = coordinator.subscribe();
        let broker_bg = broker.clone();
        tokio::spawn(async move {
            loop {
                match job_rx.recv().await {
                    Ok(JobEvent::RunStarted { slug, stages }) => {
                        broker_bg.publish(SseEvent::RunStarted { slug, stages });
                    }
                    Ok(JobEvent::RunFinished { slug, ok }) => {
                        broker_bg.publish(SseEvent::RunFinished { slug, ok });
                    }
                    Ok(JobEvent::StageStarted { .. })
                    | Ok(JobEvent::StageFinished { .. })
                    | Ok(JobEvent::StageFailed { .. }) => {
                        // Manifest watcher also fires WorkspaceUpdated; no duplicate noise needed.
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        broker_bg.publish(SseEvent::SyncLagged);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let state = Self {
            token,
            broker,
            coordinator,
            project: Arc::new(Mutex::new(ProjectInner {
                working_root: None,
                watch: None,
                next_watch_id: 1,
            })),
        };

        if let Some(root) = initial_root {
            if root.is_dir() {
                let _ = state.open_project(root, false);
            }
        }
        state
    }

    pub fn working_root(&self) -> Option<PathBuf> {
        self.project.lock().unwrap().working_root.clone()
    }

    pub fn sync_watches(&self) {
        let mut guard = self.project.lock().unwrap();
        let root = guard.working_root.clone();
        if let (Some(ref r), Some(ref mut w)) = (root, guard.watch.as_mut()) {
            w.watcher.sync_watches(r);
        }
    }

    pub fn suspend_workspace_watches(
        &self,
        working_root: &Path,
        slugs: &[String],
    ) -> WorkspaceWatchGuard {
        let mut deduped: Vec<String> = slugs.to_vec();
        deduped.sort();
        deduped.dedup();
        if deduped.is_empty() {
            return WorkspaceWatchGuard {
                project: Arc::clone(&self.project),
                working_root: working_root.to_path_buf(),
                watch_id: None,
                slugs: Vec::new(),
            };
        }

        let mut guard = self.project.lock().unwrap_or_else(|e| e.into_inner());
        let watch_id = if guard.working_root.as_deref() == Some(working_root) {
            if let Some(ref mut w) = guard.watch {
                w.watcher.suppress_workspaces(working_root, &deduped);
                Some(w.watch_id)
            } else {
                None
            }
        } else {
            None
        };

        WorkspaceWatchGuard {
            project: Arc::clone(&self.project),
            working_root: working_root.to_path_buf(),
            watch_id,
            slugs: deduped,
        }
    }

    pub fn open_project(
        &self,
        path: PathBuf,
        persist_recent: bool,
    ) -> Result<(PathBuf, Vec<String>), OpenProjectError> {
        let active = self.coordinator.active_count();
        if active > 0 {
            return Err(OpenProjectError::ActiveJobs(active));
        }
        if !path.is_dir() {
            return Err(OpenProjectError::InvalidPath(format!(
                "directory does not exist: {}",
                path.display()
            )));
        }
        let canonical = clean_display_path(path.canonicalize().unwrap_or(path));

        // Tear down any previous watcher before binding the new working root.
        {
            let mut guard = self.project.lock().unwrap();
            if let Some(old) = guard.watch.take() {
                old.task.abort();
                drop(old.watcher);
            }
            guard.working_root = Some(canonical.clone());
        }

        // Start non-recursive debounced watcher for the new root.
        let (tx, rx) = mpsc::unbounded_channel();
        let watcher = WorkspaceWatcher::start(canonical.clone(), tx);
        let broker = self.broker.clone();
        let root_for_task = canonical.clone();
        let state_for_task = self.clone();
        let task = tokio::spawn(async move {
            run_watch_pump(root_for_task, state_for_task, broker, rx).await;
        });

        {
            let mut guard = self.project.lock().unwrap();
            let watch_id = guard.next_watch_id;
            guard.next_watch_id = guard.next_watch_id.wrapping_add(1);
            guard.watch = Some(ActiveWatch {
                watch_id,
                watcher,
                task,
            });
        }

        let recent: Vec<String> = if persist_recent {
            let mut cfg = load_app_config().unwrap_or_default();
            cfg.prune_missing_recent_folders();
            cfg.record_recent_folder(canonical.clone());
            let _ = save_app_config(&cfg);
            cfg.recent_folders
                .iter()
                .map(|p| p.display().to_string())
                .collect()
        } else {
            let mut cfg = load_app_config().unwrap_or_default();
            if cfg.prune_missing_recent_folders() {
                let _ = save_app_config(&cfg);
            }
            cfg.recent_folders
                .iter()
                .map(|p| p.display().to_string())
                .collect()
        };

        self.broker.publish(SseEvent::ProjectChanged {
            working_root: Some(canonical.display().to_string()),
        });

        Ok((canonical, recent))
    }
}

async fn run_watch_pump(
    working_root: PathBuf,
    state: AppState,
    broker: EventBroker,
    mut rx: mpsc::UnboundedReceiver<WatchEvent>,
) {
    let mut known_slugs: HashSet<String> = load_index(&working_root)
        .map(|i| i.workspaces.keys().cloned().collect())
        .unwrap_or_default();

    while let Some(ev) = rx.recv().await {
        match ev {
            WatchEvent::IndexChanged => {
                state.sync_watches();
                if let Ok(idx) = load_index(&working_root) {
                    let current: HashSet<String> = idx.workspaces.keys().cloned().collect();
                    let mut added: Vec<String> =
                        current.difference(&known_slugs).cloned().collect();
                    let mut removed: Vec<String> =
                        known_slugs.difference(&current).cloned().collect();
                    added.sort();
                    removed.sort();
                    if !added.is_empty() {
                        broker.publish(SseEvent::WorkspacesAdded { slugs: added });
                    }
                    if !removed.is_empty() {
                        broker.publish(SseEvent::WorkspacesRemoved { slugs: removed });
                    }
                    known_slugs = current;
                }
            }
            WatchEvent::ConfigChanged => {
                broker.publish(SseEvent::ConfigChanged);
            }
            WatchEvent::ManifestChanged { slug } => {
                state.sync_watches();
                emit_manifest_updated(&working_root, &slug, &broker);
            }
            WatchEvent::CutsChanged { slug } => {
                let cuts_path = workspace_dir(&working_root, &slug)
                    .join("decisions")
                    .join("cuts.json");
                if let Ok(bytes) = fs::read(&cuts_path) {
                    let hash = bytes_sha256_hex(&bytes);
                    broker.publish(SseEvent::CutsChanged { slug, hash });
                }
            }
            WatchEvent::LockChanged { slug } => {
                let locked_by = inspect_workspace_lock(&working_root, &slug);
                broker.publish(SseEvent::LockChanged { slug, locked_by });
            }
            WatchEvent::PollingFallback { reason } => {
                tracing::warn!(reason = %reason, "notify watcher fell back to polling");
            }
            WatchEvent::WatcherError { message } => {
                tracing::warn!(error = %message, "workspace watcher error");
            }
        }
    }
}

fn emit_manifest_updated(working_root: &Path, slug: &str, broker: &EventBroker) {
    let ws = workspace_dir(working_root, slug);
    if let Ok(m) = load_manifest(&ws) {
        let stages: BTreeMap<String, String> = m
            .stages
            .iter()
            .map(|(k, b)| (k.clone(), b.status.as_str().to_string()))
            .collect();
        broker.publish(SseEvent::WorkspaceUpdated {
            slug: slug.to_string(),
            stages,
            updated_at: m.updated_at.to_rfc3339(),
        });
    }
}
