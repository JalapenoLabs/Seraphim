//! Live agent statistics: cost, tokens, time, context window fill, and the
//! subscription usage limit, aggregated per task or globally.
//!
//! Several pieces lean on what we actually capture: cost and per-turn token usage
//! (which also drive cost) come from each turn's terminal `result` event, time is
//! summed turn elapsed time, the context gauge measures the latest turn's largest
//! single-request prompt against the model's window, and the usage gauge reads the
//! latest rate-limit notice's utilization when present, otherwise its categorical
//! status. These are session/global totals; Seraphim runs one shared Claude
//! session, so they are not split per task or per source.

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{Settings, StatsAggregate};
use crate::db::queries;
use crate::state::{AppState, LiveUsage, SubscriptionUsage};

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    /// Total cost in USD over the scope.
    pub cost_usd: f64,
    /// Total input tokens, including cache creation and cache reads.
    pub input_tokens: i64,
    /// Total output tokens (includes reasoning).
    pub output_tokens: i64,
    pub total_tokens: i64,
    /// Total time worked, in milliseconds, summed over completed turns.
    pub worked_ms: i64,
    /// If a turn is in progress, when it started, so the UI can tick live.
    pub running_since: Option<DateTime<Utc>>,
    /// The latest turn's context size (input + cache tokens), as a stand-in for
    /// how full the context window currently is.
    pub context_tokens: i64,
    /// The active model's context window (the denominator for the context gauge).
    pub context_window: i64,
    /// Subscription usage-limit utilization, 0-100, or null if unknown.
    pub usage_utilization: Option<f64>,
    /// When the usage window resets (unix seconds), if known.
    pub usage_resets_at: Option<i64>,
    /// The rate-limit status (e.g. "allowed") when the stream reports no numeric
    /// utilization, so the UI can show a status instead of a misleading 0%.
    pub usage_status: Option<String>,
    /// Subscription 7-day usage utilization (0-100) and its reset, when a
    /// subscription login is configured (polled from `/api/oauth/usage`).
    pub usage_seven_day_utilization: Option<f64>,
    pub usage_seven_day_resets_at: Option<i64>,
    pub turns: i64,
}

/// The active model's context window. The `[1m]` suffix is Claude Code's opt-in
/// to the 1M window; everything else is the standard 200K.
fn context_window(model: &str) -> i64 {
    if model.contains("[1m]") {
        1_000_000
    } else {
        200_000
    }
}

/// The prompt size of a single API request: its fresh input plus cached tokens.
fn request_total(usage: &Value) -> i64 {
    let field = |key: &str| usage.get(key).and_then(Value::as_i64).unwrap_or(0);
    field("input_tokens") + field("cache_creation_input_tokens") + field("cache_read_input_tokens")
}

/// The context-window occupancy a turn's `usage` block implies: a single API
/// request's prompt size (input + cache), not the whole turn's total.
///
/// A turn is one `claude -p` run that makes many internal API requests; the
/// top-level `usage` sums them, and `cache_read_input_tokens` re-counts the
/// cached context on every request, so the total runs to many times the window.
/// The real occupancy is one request's prompt, so take the largest per-request
/// total from the `iterations` breakdown, falling back to the top-level totals
/// only for a single-request turn that carries no `iterations`.
fn context_tokens(usage: &Value) -> i64 {
    usage
        .get("iterations")
        .and_then(Value::as_array)
        .and_then(|requests| requests.iter().map(request_total).max())
        .filter(|&largest| largest > 0)
        .unwrap_or_else(|| request_total(usage))
}

/// Pulls `(utilization 0-100, resetsAt, status)` from a rate-limit event payload.
///
/// The headless `claude -p` stream reports a categorical `status` and a reset
/// time but usually no numeric `utilization`, so the percentage is often `None`
/// and the UI falls back to the status. When utilization is present it may arrive
/// as a fraction (0-1) or a percentage (0-100).
fn rate_limit_fields(payload: &Value) -> (Option<f64>, Option<i64>, Option<String>) {
    let info = payload.get("rate_limit_info").unwrap_or(payload);
    let utilization = info
        .get("utilization")
        .and_then(Value::as_f64)
        .map(|value| {
            let percent = if value <= 1.0 { value * 100.0 } else { value };
            percent.clamp(0.0, 100.0)
        });
    let resets_at = info.get("resetsAt").and_then(Value::as_i64);
    let status = info
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    (utilization, resets_at, status)
}

fn build_response(
    settings: &Settings,
    agg: &StatsAggregate,
    running_since: Option<DateTime<Utc>>,
    latest_usage: Option<&Value>,
    rate_limit: Option<&Value>,
    live_usage: Option<LiveUsage>,
    usage: Option<SubscriptionUsage>,
) -> StatsResponse {
    let input_total = agg.input_tokens + agg.cache_creation_tokens + agg.cache_read_tokens;
    let (rate_utilization, rate_resets_at, usage_status) =
        rate_limit.map_or((None, None, None), rate_limit_fields);
    // Prefer the polled subscription usage (the real 5-hour percentage from
    // /api/oauth/usage) over the stream's status-only rate-limit event, which
    // carries no number. Fall back to the rate-limit fields when no subscription
    // login is configured.
    let usage_utilization = usage
        .as_ref()
        .and_then(|snapshot| snapshot.five_hour_utilization)
        .or(rate_utilization);
    let usage_resets_at = usage
        .as_ref()
        .and_then(|snapshot| snapshot.five_hour_resets_at)
        .or(rate_resets_at);
    let usage_seven_day_utilization = usage
        .as_ref()
        .and_then(|snapshot| snapshot.seven_day_utilization);
    let usage_seven_day_resets_at = usage
        .as_ref()
        .and_then(|snapshot| snapshot.seven_day_resets_at);

    // Overlay the in-progress turn's live usage (if any) on top of the persisted,
    // completed-turn totals so the counter ticks mid-turn. Only output is added to
    // the totals (output is turn-cumulative); input recurs per round-trip, so the
    // live input feeds only the context gauge, not the cumulative input total. The
    // persisted `result` event remains the source of truth once the turn lands.
    let live_output = live_usage.map_or(0, |usage| usage.output_tokens);
    let context = live_usage
        .map(|usage| usage.context_tokens)
        .filter(|&tokens| tokens > 0)
        .unwrap_or_else(|| latest_usage.map_or(0, context_tokens));

    StatsResponse {
        cost_usd: agg.cost_usd,
        input_tokens: input_total,
        output_tokens: agg.output_tokens + live_output,
        total_tokens: input_total + agg.output_tokens + live_output,
        worked_ms: agg.worked_ms,
        running_since,
        context_tokens: context,
        context_window: context_window(&settings.claude_model),
        usage_utilization,
        usage_resets_at,
        usage_status,
        usage_seven_day_utilization,
        usage_seven_day_resets_at,
        turns: agg.turns,
    }
}

/// `GET /api/v1/stats` - lifetime totals across every task.
pub async fn global(State(state): State<AppState>) -> ApiResult<Json<StatsResponse>> {
    let settings = queries::get_settings(&state.db).await?;
    let agg = queries::global_stats(&state.db).await?;
    let running_since = queries::global_running_since(&state.db).await?;
    let latest_usage = queries::global_latest_usage(&state.db).await?;
    let rate_limit = queries::latest_rate_limit(&state.db).await?;
    Ok(Json(build_response(
        &settings,
        &agg,
        running_since,
        latest_usage.as_ref(),
        rate_limit.as_ref(),
        // One shared agent, so any in-progress turn's live usage counts globally.
        state.live_usage(),
        state.usage(),
    )))
}

/// `GET /api/v1/tasks/:id/stats` - this task's totals since its last hard reset.
pub async fn task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StatsResponse>> {
    let settings = queries::get_settings(&state.db).await?;
    let agg = queries::task_stats(&state.db, id).await?;
    let running_since = queries::task_running_since(&state.db, id).await?;
    let latest_usage = queries::task_latest_usage(&state.db, id).await?;
    let rate_limit = queries::latest_rate_limit(&state.db).await?;
    Ok(Json(build_response(
        &settings,
        &agg,
        running_since,
        latest_usage.as_ref(),
        rate_limit.as_ref(),
        // Only overlay the live counter when the running turn is this task's.
        state.live_usage().filter(|usage| usage.task_id == id),
        state.usage(),
    )))
}

/// `GET /api/v1/railways/:id/stats` - this railway's totals (context, cost,
/// tokens, time) over its tasks' turns since the global stats reset.
///
/// The subscription usage gauge in the response is the same shared, global figure
/// (one subscription powers every lane); the board's top bar renders that gauge
/// once from the global stats, while each lane reads only the per-railway context,
/// cost, tokens, and time from here.
pub async fn railway(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StatsResponse>> {
    let settings = queries::get_settings(&state.db).await?;
    let agg = queries::railway_stats(&state.db, id).await?;
    let running_since = queries::railway_running_since(&state.db, id).await?;
    let latest_usage = queries::railway_latest_usage(&state.db, id).await?;
    let rate_limit = queries::latest_rate_limit(&state.db).await?;

    // Overlay the live mid-turn counter only when the running turn's task belongs
    // to this lane, so a lane that is not the one currently working never borrows
    // another lane's live tokens.
    let live_usage = match state.live_usage() {
        Some(usage) => {
            let running_railway = queries::get_task(&state.db, usage.task_id)
                .await?
                .map(|task| task.railway_id);
            running_railway
                .filter(|&railway_id| railway_id == id)
                .map(|_| usage)
        }
        None => None,
    };

    Ok(Json(build_response(
        &settings,
        &agg,
        running_since,
        latest_usage.as_ref(),
        rate_limit.as_ref(),
        live_usage,
        state.usage(),
    )))
}

/// `GET /api/v1/compose/stats` - the compose assistant's own usage totals, for
/// its dedicated stats bar (issue #181). Its turns are separate from the board's,
/// so this never mixes with the global or per-task numbers.
pub async fn compose(State(state): State<AppState>) -> ApiResult<Json<StatsResponse>> {
    let settings = queries::get_settings(&state.db).await?;
    let agg = queries::compose_stats(&state.db).await?;
    let running_since = queries::compose_running_since(&state.db).await?;
    let latest_usage = queries::compose_latest_usage(&state.db).await?;
    let rate_limit = queries::latest_rate_limit(&state.db).await?;
    Ok(Json(build_response(
        &settings,
        &agg,
        running_since,
        latest_usage.as_ref(),
        rate_limit.as_ref(),
        // The compose stats settle at turn end; no live mid-turn overlay.
        None,
        state.usage(),
    )))
}

/// `POST /api/v1/stats/reset` - reset the global statistics (non-destructive).
pub async fn reset(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    queries::reset_global_stats(&state.db).await?;
    state.notify_board();
    Ok(Json(json!({ "reset": true })))
}
