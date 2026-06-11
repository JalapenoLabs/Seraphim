//! Shared application state and the server-sent-event broadcast bus.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use eyre::Result;
use octocrab::Octocrab;
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::db::queries;
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
    /// The agent asked the user something; drives toasts and native notifications.
    Notification {
        task_id: Uuid,
        task_title: String,
        prompt: String,
    },
}

/// Clonable, shared state handed to every request handler and background task.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub workspace: Workspace,
    pub events: broadcast::Sender<ServerEvent>,
    /// URL the workspace uses to reach this API (for the agent's helpers).
    pub internal_api_url: String,
    /// When set and still in the future, the agent is in a brief global cooldown
    /// after a transient (server-side) rate limit and is about to retry the
    /// current turn. Purely an ephemeral UI signal, so it lives in memory rather
    /// than the database; the board handler reads it into the settings payload.
    cooldown_until: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl AppState {
    pub fn new(db: PgPool, workspace: Workspace, internal_api_url: String) -> Self {
        let (events, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            db,
            workspace,
            events,
            internal_api_url,
            cooldown_until: Arc::new(RwLock::new(None)),
        }
    }

    /// The active rate-limit cooldown deadline, if one is set.
    pub fn cooldown_until(&self) -> Option<DateTime<Utc>> {
        *self.cooldown_until.read().expect("cooldown lock poisoned")
    }

    /// Sets (or clears with `None`) the rate-limit cooldown deadline. Callers
    /// follow this with [`Self::notify_board`] so the navbar status updates live.
    pub fn set_cooldown_until(&self, until: Option<DateTime<Utc>>) {
        *self.cooldown_until.write().expect("cooldown lock poisoned") = until;
    }

    /// Builds a GitHub client from the token stored in the database. Built on
    /// demand so a token added in the UI takes effect without a restart.
    pub async fn github(&self) -> Result<Octocrab> {
        let token = queries::get_github_token(&self.db).await?;
        let builder = Octocrab::builder();
        let builder = if token.is_empty() {
            builder
        } else {
            builder.personal_token(token)
        };
        builder.build().map_err(Into::into)
    }

    /// Signals that the board changed; ignores the error when no clients listen.
    pub fn notify_board(&self) {
        let _ = self.events.send(ServerEvent::Board);
    }

    /// Pushes an agent event onto a task's live stream.
    pub fn notify_task(&self, task_id: Uuid, payload: serde_json::Value) {
        let _ = self.events.send(ServerEvent::Task { task_id, payload });
    }

    /// Announces a new question so the UI can toast and notify the user.
    pub fn notify_question(&self, task_id: Uuid, task_title: String, prompt: String) {
        let _ = self.events.send(ServerEvent::Notification {
            task_id,
            task_title,
            prompt,
        });
    }
}
