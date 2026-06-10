//! HTTP surface: REST routes under `/api/v1` plus SSE live streams.
//!
//! Handlers are grouped by resource. Every handler returns [`ApiResult`], which
//! turns an `eyre` error into a 500 with a JSON body, so the happy path stays
//! free of error plumbing.

mod board;
mod data;
mod repos;
mod settings;
mod sse;
mod tasks;
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
        .route("/tasks/:id", get(tasks::get_task))
        .route("/tasks/:id/stream", get(sse::task_stream))
        .route("/tasks/:id/move", post(board::move_task))
        .route("/tasks/:id/hold", post(board::set_hold))
        .route("/repos", get(repos::list).post(repos::upsert))
        .route(
            "/repos/:id",
            axum::routing::put(repos::update).delete(repos::delete),
        )
        .route("/repos/import-org", post(repos::import_org))
        .route("/sync", post(repos::sync))
        .route("/settings", get(settings::get).patch(settings::update))
        .route("/settings/pause", post(settings::set_pause))
        .route("/settings/tokens", post(settings::set_tokens))
        .route(
            "/settings/env",
            get(settings::list_env).put(settings::set_env),
        )
        .route("/workspace/restart", post(workspace::restart))
        .route("/workspace/recreate", post(workspace::recreate))
        .route("/workspace/provision", post(workspace::provision))
        .route("/export", get(data::export))
        .route("/import", post(data::import))
        .with_state(state);

    Router::new()
        .nest("/api/v1", api)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
