//! Shared application state and the server-sent-event broadcast bus.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use eyre::Result;
use octocrab::Octocrab;
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::claude::oauth::PendingAuth;
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
    /// A turn died mid-flight (a "heart attack") and the defibrillator handled it;
    /// drives an alert toast and native notification so the operator notices.
    HeartAttack {
        task_id: Option<Uuid>,
        task_title: String,
        /// A one-line summary of what happened and what the defibrillator did.
        summary: String,
    },
    /// A task finished (auto-merged to Done); drives the completion sound.
    TaskFinished { task_id: Uuid, task_title: String },
    /// A streamed event from the compose assistant's turn (issue #181); the
    /// /compose page appends it to the chat transcript.
    Compose { payload: serde_json::Value },
    /// The compose assistant's drafts or stats changed; the page refetches them.
    ComposeChanged,
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
///
/// Railways run their turns in parallel, so the live overlay is held per railway
/// (keyed by `railway_id` on [`AppState::live_usage`]); each turn owns and clears
/// only its own railway's entry. The global stats aggregate across all entries.
#[derive(Debug, Clone, Copy)]
pub struct LiveUsage {
    /// The railway whose lane this turn runs on. The map key as well, kept on the
    /// value so an aggregate read needs no separate lookup.
    pub railway_id: Uuid,
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

/// The aggregate of every railway's live overlay, for the global stats endpoint.
///
/// Output tokens sum across lanes (each lane's output is additive), the context
/// gauge takes the **max** over lanes (context fill is per-session and does not
/// add up, so the global gauge shows the lane closest to compaction), and
/// `running_turns` counts the lanes currently generating so the client can tick
/// worked time at the correct combined rate.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveUsageAggregate {
    /// Summed live output tokens across all generating lanes.
    pub output_tokens: i64,
    /// The largest live context size across lanes (0 when no lane is generating).
    pub max_context_tokens: i64,
    /// How many lanes are currently generating (for the live worked-time tick).
    pub running_turns: i64,
}

/// Folds each lane's live overlay into the global aggregate.
///
/// Output tokens sum (each lane's output is additive), the context gauge takes the
/// max (per-session fill does not add up; the global gauge shows the lane closest
/// to compaction), and `running_turns` counts the lanes. Kept pure and free of the
/// lock so it can be unit-tested directly.
fn aggregate_live_usage(entries: impl Iterator<Item = LiveUsage>) -> LiveUsageAggregate {
    let mut aggregate = LiveUsageAggregate::default();
    for entry in entries {
        aggregate.output_tokens += entry.output_tokens;
        aggregate.max_context_tokens = aggregate.max_context_tokens.max(entry.context_tokens);
        aggregate.running_turns += 1;
    }
    aggregate
}

/// A cached subscription usage snapshot for the gauge, polled from
/// `/api/oauth/usage`. `None` when no subscription login is configured or no poll
/// has succeeded yet. Utilization values are percentages (0-100); reset times are
/// unix seconds.
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionUsage {
    pub five_hour_utilization: Option<f64>,
    pub five_hour_resets_at: Option<i64>,
    pub seven_day_utilization: Option<f64>,
    pub seven_day_resets_at: Option<i64>,
}

/// Clonable, shared state handed to every request handler and background task.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub workspace: Workspace,
    /// Handle to the Tailscale sidecar container, for the management UI.
    pub tailscale: crate::tailscale::Tailscale,
    pub events: broadcast::Sender<ServerEvent>,
    /// URL the workspace uses to reach this API (for the agent's helpers).
    pub internal_api_url: String,
    /// When set and still in the future, the agent is in a brief global cooldown
    /// after a transient (server-side) rate limit and is about to retry the
    /// current turn. Purely an ephemeral UI signal, so it lives in memory rather
    /// than the database; the board handler reads it into the settings payload.
    cooldown_until: Arc<RwLock<Option<DateTime<Utc>>>>,
    /// Live token usage of each railway's in-progress turn, keyed by `railway_id`.
    /// Railways generate in parallel, so this is a per-lane map rather than a single
    /// slot; an absent key means that lane is between turns. Ephemeral (like
    /// [`Self::cooldown_until`]); the stats endpoints overlay it on the persisted
    /// totals so the counters tick during generation.
    live_usage: Arc<RwLock<HashMap<Uuid, LiveUsage>>>,
    /// The PKCE secrets for an in-flight Claude subscription OAuth login, held
    /// between starting the flow (which returns the consent URL) and the operator
    /// pasting the code back. Ephemeral; only one login is in flight at a time.
    pending_oauth: Arc<RwLock<Option<PendingAuth>>>,
    /// The latest polled subscription usage snapshot, refreshed by the usage loop
    /// and read by the stats gauges. `None` until a poll succeeds.
    usage: Arc<RwLock<Option<SubscriptionUsage>>>,
    /// Serializes refreshes of the Claude subscription token. The provider rotates
    /// the refresh token on each use, so a turn and the background keepalive
    /// refreshing at once would race and one would persist an already-invalidated
    /// token; this lock makes the refresh-or-reuse decision atomic.
    claude_token_refresh: Arc<AsyncMutex<()>>,
    /// Bumped by a hard reset. A turn captures it at the start and abandons its
    /// post-turn handling (session persist, task move) if it changed, so a reset
    /// that lands mid-turn is never undone by the turn it interrupted.
    reset_epoch: Arc<AtomicU64>,
    /// Static self-update config (the build's commit/branch + host paths).
    pub update: crate::config::UpdateConfig,
    /// The cached result of the last self-update check, refreshed hourly.
    update_status: Arc<RwLock<UpdateStatus>>,
}

/// The cached result of the last self-update check, plus whether an update is
/// running. Refreshed hourly in the background and on demand from the UI.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    /// The commit the running build is on (`unknown` if not stamped at build).
    pub current_sha: String,
    pub current_branch: String,
    /// The latest commit on the branch upstream, when the last check succeeded.
    pub latest_sha: Option<String>,
    pub update_available: bool,
    /// Whether self-update is wired up (a host repo dir is set) so it can run.
    pub configured: bool,
    /// True from the moment an update is triggered until this process is replaced.
    pub updating: bool,
    pub checked_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

impl AppState {
    pub fn new(
        db: PgPool,
        workspace: Workspace,
        tailscale: crate::tailscale::Tailscale,
        internal_api_url: String,
        update: crate::config::UpdateConfig,
    ) -> Self {
        let (events, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let update_status = UpdateStatus {
            current_sha: update.git_sha.clone(),
            current_branch: update.git_branch.clone(),
            latest_sha: None,
            update_available: false,
            configured: !update.host_repo_dir.trim().is_empty(),
            updating: false,
            checked_at: None,
            error: None,
        };
        Self {
            db,
            workspace,
            tailscale,
            events,
            internal_api_url,
            cooldown_until: Arc::new(RwLock::new(None)),
            live_usage: Arc::new(RwLock::new(HashMap::new())),
            pending_oauth: Arc::new(RwLock::new(None)),
            usage: Arc::new(RwLock::new(None)),
            claude_token_refresh: Arc::new(AsyncMutex::new(())),
            reset_epoch: Arc::new(AtomicU64::new(0)),
            update,
            update_status: Arc::new(RwLock::new(update_status)),
        }
    }

    /// The cached self-update status.
    pub fn update_status(&self) -> UpdateStatus {
        self.update_status
            .read()
            .expect("update status lock poisoned")
            .clone()
    }

    /// Replaces the cached self-update status.
    pub fn set_update_status(&self, status: UpdateStatus) {
        *self
            .update_status
            .write()
            .expect("update status lock poisoned") = status;
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

    /// The live token usage of one railway's in-progress turn, if it is generating.
    pub fn live_usage_for(&self, railway_id: Uuid) -> Option<LiveUsage> {
        self.live_usage
            .read()
            .expect("live usage lock poisoned")
            .get(&railway_id)
            .copied()
    }

    /// The aggregate of every railway's live overlay, for the global stats endpoint.
    /// Sums output tokens, takes the max context, and counts the generating lanes.
    pub fn live_usage_aggregate(&self) -> LiveUsageAggregate {
        let usage = self.live_usage.read().expect("live usage lock poisoned");
        aggregate_live_usage(usage.values().copied())
    }

    /// Records one railway's in-progress turn live token usage. Cheap and called
    /// often; pair with the throttled [`Self::notify_usage`] for the SSE tick rather
    /// than emitting on every update.
    pub fn set_live_usage(&self, usage: LiveUsage) {
        self.live_usage
            .write()
            .expect("live usage lock poisoned")
            .insert(usage.railway_id, usage);
    }

    /// Drops one railway's live overlay (the turn ended), leaving other lanes' live
    /// usage intact.
    pub fn clear_live_usage_for(&self, railway_id: Uuid) {
        self.live_usage
            .write()
            .expect("live usage lock poisoned")
            .remove(&railway_id);
    }

    /// Drops every railway's live overlay at once (used by the global hard reset).
    pub fn clear_live_usage(&self) {
        self.live_usage
            .write()
            .expect("live usage lock poisoned")
            .clear();
    }

    /// Stashes the PKCE secrets for an in-flight subscription OAuth login.
    pub fn set_pending_oauth(&self, pending: PendingAuth) {
        *self.pending_oauth.write().expect("oauth lock poisoned") = Some(pending);
    }

    /// Consumes the in-flight OAuth secrets (so a code can't be redeemed twice).
    pub fn take_pending_oauth(&self) -> Option<PendingAuth> {
        self.pending_oauth
            .write()
            .expect("oauth lock poisoned")
            .take()
    }

    /// The latest polled subscription usage snapshot, if any.
    pub fn usage(&self) -> Option<SubscriptionUsage> {
        self.usage.read().expect("usage lock poisoned").clone()
    }

    /// Replaces (or clears with `None`) the cached subscription usage snapshot.
    pub fn set_usage(&self, usage: Option<SubscriptionUsage>) {
        *self.usage.write().expect("usage lock poisoned") = usage;
    }

    /// The lock that serializes Claude subscription token refreshes. Callers hold
    /// the returned guard across the read-decide-refresh-persist sequence so two
    /// refreshers never race on the rotating refresh token.
    pub fn claude_token_refresh(&self) -> &AsyncMutex<()> {
        &self.claude_token_refresh
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

    /// Announces a heart attack so the UI alerts the operator immediately, in
    /// addition to the persistent board banner driven by [`Self::notify_board`].
    pub fn notify_heart_attack(&self, task_id: Option<Uuid>, task_title: String, summary: String) {
        let _ = self.events.send(ServerEvent::HeartAttack {
            task_id,
            task_title,
            summary,
        });
    }

    /// Announces that a task finished, so the UI can play the completion sound.
    pub fn notify_task_finished(&self, task_id: Uuid, task_title: String) {
        let _ = self.events.send(ServerEvent::TaskFinished {
            task_id,
            task_title,
        });
    }

    /// Pushes a compose-assistant event onto its live chat stream (issue #181).
    pub fn notify_compose(&self, payload: serde_json::Value) {
        let _ = self.events.send(ServerEvent::Compose { payload });
    }

    /// Signals that the compose drafts or stats changed; clients refetch them.
    pub fn notify_compose_changed(&self) {
        let _ = self.events.send(ServerEvent::ComposeChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(output: i64, context: i64) -> LiveUsage {
        LiveUsage {
            railway_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            output_tokens: output,
            context_tokens: context,
        }
    }

    #[test]
    fn aggregate_is_empty_with_no_lanes() {
        let aggregate = aggregate_live_usage(std::iter::empty());
        assert_eq!(aggregate.output_tokens, 0);
        assert_eq!(aggregate.max_context_tokens, 0);
        assert_eq!(aggregate.running_turns, 0);
    }

    #[test]
    fn aggregate_single_lane_matches_that_lane() {
        let aggregate = aggregate_live_usage([live(120, 8_000)].into_iter());
        // The single-railway case reads exactly as the lane's own live usage.
        assert_eq!(aggregate.output_tokens, 120);
        assert_eq!(aggregate.max_context_tokens, 8_000);
        assert_eq!(aggregate.running_turns, 1);
    }

    #[test]
    fn aggregate_sums_output_and_counts_two_lanes() {
        let aggregate = aggregate_live_usage([live(120, 8_000), live(80, 5_000)].into_iter());
        // Output tokens add across lanes so the live counter roughly doubles.
        assert_eq!(aggregate.output_tokens, 200);
        assert_eq!(aggregate.running_turns, 2);
    }

    #[test]
    fn aggregate_takes_max_context_not_sum() {
        let aggregate = aggregate_live_usage([live(10, 8_000), live(10, 5_000)].into_iter());
        // Context fill is per-session, so the global gauge shows the largest lane,
        // never the sum (which would overflow the window).
        assert_eq!(aggregate.max_context_tokens, 8_000);
    }
}
