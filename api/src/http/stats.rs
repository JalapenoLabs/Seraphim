//! Live agent statistics: cost, tokens, time, context window fill, and the
//! subscription usage limit, aggregated per task or globally.
//!
//! Several pieces lean on what we actually capture: cost and per-turn token usage
//! (which also drive cost) come from each turn's terminal `result` event, time is
//! summed turn elapsed time, the context gauge measures the latest turn's tokens
//! against the model's window, and the usage gauge reads the latest rate-limit
//! notice's global utilization. These are session/global totals; Seraphim runs
//! one shared Claude session, so they are not split per task or per source.

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{Settings, StatsAggregate};
use crate::db::queries;
use crate::state::AppState;

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

/// The context size a turn's `usage` block implies: the input it sent, cached or
/// not, which is the conversation currently in the window.
fn context_tokens(usage: &Value) -> i64 {
    let field = |key: &str| usage.get(key).and_then(Value::as_i64).unwrap_or(0);
    field("input_tokens") + field("cache_creation_input_tokens") + field("cache_read_input_tokens")
}

/// Pulls `(utilization 0-100, resetsAt)` from a rate-limit event payload. The
/// utilization may arrive as a fraction (0-1) or a percentage (0-100).
fn rate_limit_fields(payload: &Value) -> (Option<f64>, Option<i64>) {
    let info = payload.get("rate_limit_info").unwrap_or(payload);
    let utilization = info
        .get("utilization")
        .and_then(Value::as_f64)
        .map(|value| {
            let percent = if value <= 1.0 { value * 100.0 } else { value };
            percent.clamp(0.0, 100.0)
        });
    let resets_at = info.get("resetsAt").and_then(Value::as_i64);
    (utilization, resets_at)
}

fn build_response(
    settings: &Settings,
    agg: &StatsAggregate,
    running_since: Option<DateTime<Utc>>,
    latest_usage: Option<&Value>,
    rate_limit: Option<&Value>,
) -> StatsResponse {
    let input_total = agg.input_tokens + agg.cache_creation_tokens + agg.cache_read_tokens;
    let context = latest_usage.map_or(0, context_tokens);
    let (usage_utilization, usage_resets_at) = rate_limit.map_or((None, None), rate_limit_fields);

    StatsResponse {
        cost_usd: agg.cost_usd,
        input_tokens: input_total,
        output_tokens: agg.output_tokens,
        total_tokens: input_total + agg.output_tokens,
        worked_ms: agg.worked_ms,
        running_since,
        context_tokens: context,
        context_window: context_window(&settings.claude_model),
        usage_utilization,
        usage_resets_at,
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
    )))
}

/// `POST /api/v1/stats/reset` - reset the global statistics (non-destructive).
pub async fn reset(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    queries::reset_global_stats(&state.db).await?;
    state.notify_board();
    Ok(Json(json!({ "reset": true })))
}
