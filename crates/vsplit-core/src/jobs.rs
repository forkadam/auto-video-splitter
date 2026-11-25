use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    RunStarted {
        slug: String,
        stages: Vec<String>,
    },
    StageStarted {
        slug: String,
        stage: String,
    },
    StageFinished {
        slug: String,
        stage: String,
        status: String,
    },
    StageFailed {
        slug: String,
        stage: String,
        message: String,
    },
    RunFinished {
        slug: String,
        ok: bool,
    },
}

/// Tracks active in-process workspace runs, binds child `CancellationToken`s,
/// and broadcasts structured lifecycle events to `vsplit-server` SSE clients.
#[derive(Clone)]
pub struct JobCoordinator {
    root_cancel: CancellationToken,
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
    events: broadcast::Sender<JobEvent>,
}

impl Default for JobCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl JobCoordinator {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            root_cancel: CancellationToken::new(),
            active: Arc::new(Mutex::new(HashMap::new())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.events.subscribe()
    }

    pub fn emit(&self, ev: JobEvent) {
        let _ = self.events.send(ev);
    }

    /// Register a workspace run and obtain a child `CancellationToken`.
    pub fn register(&self, slug: &str) -> CancellationToken {
        let tok = self.root_cancel.child_token();
        self.active
            .lock()
            .unwrap()
            .insert(slug.to_string(), tok.clone());
        tok
    }

    pub fn unregister(&self, slug: &str) {
        self.active.lock().unwrap().remove(slug);
    }

    /// Cancel either a single workspace or all active workspaces; returns how many were signalled.
    pub fn cancel(&self, slug: Option<&str>) -> usize {
        let guard = self.active.lock().unwrap();
        match slug {
            Some(s) => {
                if let Some(tok) = guard.get(s) {
                    tok.cancel();
                    1
                } else {
                    0
                }
            }
            None => {
                let count = guard.len();
                for tok in guard.values() {
                    tok.cancel();
                }
                count
            }
        }
    }

    pub fn cancel_all(&self) {
        self.root_cancel.cancel();
        let guard = self.active.lock().unwrap();
        for tok in guard.values() {
            tok.cancel();
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.lock().unwrap().len()
    }

    pub fn active_slugs(&self) -> Vec<String> {
        let mut slugs: Vec<String> = self.active.lock().unwrap().keys().cloned().collect();
        slugs.sort();
        slugs
    }
}
