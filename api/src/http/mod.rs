//! HTTP surface: REST routes under `/api/v1` plus SSE live streams.
//!
//! Handlers are grouped by resource. Every handler returns [`ApiResult`], which
//! turns an `eyre` error into a 500 with a JSON body, so the happy path stays
//! free of error plumbing.

mod board;
mod data;
mod jira;
mod notepad;
mod questions;
mod repos;
mod settings;
mod sse;
mod stats;
mod suggestions;
mod tasks;
mod webhooks;
mod workspace;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Wraps an application error so it renders as a JSON 500.
pub struct ApiError(eyre::Report);

/// Convenience alias for handler results.
pub type ApiResult<T> = Result<T, ApiError>;

impl<E> From<E> for ApiError
where
    E: Into<eyre::Report>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}

/// Builds the full application router.
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/ping", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/board", get(board::get_board))
        .route("/board/stream", get(sse::board_stream))
        .route("/activity/stream", get(sse::activity_stream))
        .route("/tasks", post(tasks::create))
        .route("/tasks/:id", get(tasks::get_task))
        .route("/tasks/:id/issue", get(tasks::get_issue))
        .route("/tasks/:id/issue/state", post(tasks::set_issue_state))
        .route("/tasks/:id/comment", post(tasks::add_comment))
        .route("/tasks/:id/stream", get(sse::task_stream))
        .route("/tasks/:id/move", post(board::move_task))
        .route("/tasks/:id/hold", post(board::set_hold))
        .route("/tasks/:id/blocking", post(board::set_blocking))
        .route("/tasks/:id/notes", axum::routing::put(tasks::set_notes))
        .route("/tasks/:id/stats", get(stats::task))
        .route("/stats", get(stats::global))
        .route("/stats/reset", post(stats::reset))
        .route("/agent/suggestions", post(suggestions::create))
        .route("/suggestions/:id/ack", post(suggestions::acknowledge))
        .route("/agent/questions", post(questions::ask))
        .route("/questions/pending", get(questions::pending))
        .route("/questions/:id/answer", post(questions::answer))
        .route("/notifications/stream", get(sse::notification_stream))
        .route("/repos", get(repos::list).post(repos::upsert))
        .route(
            "/repos/:id",
            axum::routing::put(repos::update).delete(repos::delete),
        )
        .route("/repos/:id/deletion-impact", get(repos::deletion_impact))
        .route("/repos/import-org", post(repos::import_org))
        .route("/sync", post(repos::sync))
        // Inbound realtime issue webhooks (authenticated by their shared secret).
        .route("/webhooks/github", post(webhooks::github))
        .route("/webhooks/jira", post(webhooks::jira))
        .route("/jira/test", post(jira::test))
        .route("/jira/discover", post(jira::discover))
        .route("/jira/boards", get(jira::list))
        .route(
            "/jira/boards/:id",
            axum::routing::put(jira::update).delete(jira::delete),
        )
        .route("/settings", get(settings::get).patch(settings::update))
        .route("/settings/pause", post(settings::set_pause))
        .route("/notepad", get(notepad::get).put(notepad::set))
        .route("/settings/tokens", post(settings::set_tokens))
        .route(
            "/settings/env",
            get(settings::list_env).put(settings::set_env),
        )
        .route("/workspace/restart", post(workspace::restart))
        .route("/workspace/recreate", post(workspace::recreate))
        .route("/workspace/provision", post(workspace::provision))
        .route("/agent/reset", post(workspace::reset))
        .route("/export", get(data::export))
        .route("/import", post(data::import))
        .with_state(state);

    Router::new()
        .nest("/api/v1", api)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
