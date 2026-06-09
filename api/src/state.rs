//! Shared application state and the server-sent-event broadcast bus.

use std::sync::Arc;

use octocrab::Octocrab;
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::config::Config;
use crate::docker::Workspace;

/// How many pending server events a slow SSE client may lag before it is
/// dropped from the broadcast. Generous enough for a single-user board.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// A live update pushed to connected browsers over SSE.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ServerEvent {
    /// The board changed (a card moved, a status advanced); clients refetch.
    Board,
    /// An agent event for a specific task's live stream.
    Task {
        task_id: Uuid,
        payload: serde_json::Value,
    },
}

/// Clonable, shared state handed to every request handler and background task.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub workspace: Workspace,
    pub github: Octocrab,
    pub events: broadcast::Sender<ServerEvent>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config, workspace: Workspace, github: Octocrab) -> Self {
        let (events, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            db,
            config: Arc::new(config),
            workspace,
            github,
            events,
        }
    }

    /// Signals that the board changed; ignores the error when no clients listen.
    pub fn notify_board(&self) {
        let _ = self.events.send(ServerEvent::Board);
    }

    /// Pushes an agent event onto a task's live stream.
    pub fn notify_task(&self, task_id: Uuid, payload: serde_json::Value) {
        let _ = self.events.send(ServerEvent::Task { task_id, payload });
    }
}
