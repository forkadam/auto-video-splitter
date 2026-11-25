use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use serde::Serialize;
use tokio::sync::broadcast;
use vsplit_core::locks::LockLease;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    WorkspaceUpdated {
        slug: String,
        stages: BTreeMap<String, String>,
        updated_at: String,
    },
    WorkspacesAdded {
        slugs: Vec<String>,
    },
    WorkspacesRemoved {
        slugs: Vec<String>,
    },
    CutsChanged {
        slug: String,
        hash: String,
    },
    ConfigChanged,
    LockChanged {
        slug: String,
        locked_by: Option<LockLease>,
    },
    RunStarted {
        slug: String,
        stages: Vec<String>,
    },
    RunFinished {
        slug: String,
        ok: bool,
    },
    ProjectChanged {
        working_root: Option<String>,
    },
    SyncLagged,
    Heartbeat,
}

#[derive(Clone)]
pub struct EventBroker {
    tx: broadcast::Sender<SseEvent>,
}

impl Default for EventBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBroker {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn publish(&self, ev: SseEvent) {
        let _ = self.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SseEvent> {
        self.tx.subscribe()
    }

    pub fn stream(&self) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
        let rx = self.subscribe();
        let stream = stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Ok(ev) => {
                    let json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                    Some((Ok(Event::default().data(json)), rx))
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let json = serde_json::to_string(&SseEvent::SyncLagged)
                        .unwrap_or_else(|_| "{\"type\":\"sync_lagged\"}".into());
                    Some((Ok(Event::default().data(json)), rx))
                }
                Err(broadcast::error::RecvError::Closed) => None,
            }
        });
        let heartbeat_json = serde_json::to_string(&SseEvent::Heartbeat)
            .unwrap_or_else(|_| "{\"type\":\"heartbeat\"}".into());
        Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text(heartbeat_json),
        )
    }
}
