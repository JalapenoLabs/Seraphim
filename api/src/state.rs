//! Shared application state and the server-sent-event broadcast bus.

use std::sync::atomic::{AtomicU64, Ordering};
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
use crate::jira::{JiraClient, JiraConfig};

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
    /// A throttled tick that the in-progress turn's token usage advanced, so the
    /// stats gauges refetch and the counter ticks live mid-turn. Carries no
    /// numbers; the live values live on [`AppState::live_usage`] and are read by
    /// the stats endpoints, keeping one source of truth.
    Usage { task_id: Uuid },
}

/// The token usage of the turn currently generating, surfaced so the stats
/// gauges advance smoothly mid-turn instead of only at message/turn boundaries.
/// It is a live UI affordance, not the billing record (the `result` event remains
/// the source of truth for persisted cost/usage).
#[derive(Debug, Clone, Copy)]
pub struct LiveUsage {
    /// The task whose turn is generating.
    pub task_id: Uuid,
    /// Turn-cumulative output tokens so far: the finalized output of completed
    /// assistant messages this turn plus the current message's live count.
    pub output_tokens: i64,
    /// The current assistant message's prompt size (input + cache), for the
    /// context gauge. Input recurs per round-trip, so this is the latest message's
    /// value, not a turn sum.
    pub context_tokens: i64,
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
    /// Live token usage of the turn currently generating, or `None` between turns.
    /// Ephemeral (like [`Self::cooldown_until`]); the stats endpoints overlay it on
    /// the persisted totals so the counter ticks during generation.
    live_usage: Arc<RwLock<Option<LiveUsage>>>,
    /// Bumped by a hard reset. A turn captures it at the start and abandons its
    /// post-turn handling (session persist, task move) if it changed, so a reset
    /// that lands mid-turn is never undone by the turn it interrupted.
    reset_epoch: Arc<AtomicU64>,
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
            live_usage: Arc::new(RwLock::new(None)),
            reset_epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The current hard-reset generation. A turn captures this at its start and
    /// compares it later; a change means a reset happened during the turn.
    pub fn reset_epoch(&self) -> u64 {
        self.reset_epoch.load(Ordering::SeqCst)
    }

    /// Marks a hard reset, so any in-flight turn yields its post-turn handling.
    pub fn bump_reset_epoch(&self) {
        self.reset_epoch.fetch_add(1, Ordering::SeqCst);
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

    /// The live token usage of the in-progress turn, if one is generating.
    pub fn live_usage(&self) -> Option<LiveUsage> {
        *self.live_usage.read().expect("live usage lock poisoned")
    }

    /// Sets (or clears with `None`) the in-progress turn's live token usage.
    /// Cheap and called often; pair with the throttled [`Self::notify_usage`] for
    /// the SSE tick rather than emitting on every update.
    pub fn set_live_usage(&self, usage: Option<LiveUsage>) {
        *self.live_usage.write().expect("live usage lock poisoned") = usage;
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

    /// Builds a Jira client from the stored connection, or `None` when Jira is
    /// disabled or unconfigured. Built on demand, like [`Self::github`].
    pub async fn jira(&self) -> Result<Option<JiraClient>> {
        let settings = queries::get_settings(&self.db).await?;
        let token = queries::get_jira_token(&self.db).await?;
        match JiraConfig::from_settings(&settings, &token) {
            Some(config) => Ok(Some(JiraClient::new(config)?)),
            None => Ok(None),
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

    /// Ticks the stats gauges that the in-progress turn's usage advanced. Throttled
    /// by the caller; carries no numbers (clients refetch the stats endpoint).
    pub fn notify_usage(&self, task_id: Uuid) {
        let _ = self.events.send(ServerEvent::Usage { task_id });
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
