//! Issue source configuration endpoints.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{IssueSource, SourceKind};
use crate::db::queries;
use crate::state::AppState;

/// `GET /api/v1/sources`
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<IssueSource>>> {
    Ok(Json(queries::list_issue_sources(&state.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct CreateSourceRequest {
    pub kind: SourceKind,
    /// Provider config, e.g. `{"owner":"navarrotech","repo":"seraphim"}`.
    pub config: serde_json::Value,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: i32,
}

fn default_poll_interval() -> i32 {
    120
}

/// `POST /api/v1/sources` - register an issue source to poll.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateSourceRequest>,
) -> ApiResult<Json<IssueSource>> {
    let source =
        queries::create_issue_source(&state.db, body.kind, body.config, body.poll_interval_secs)
            .await?;
    Ok(Json(source))
}

/// `DELETE /api/v1/sources/:id`
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    queries::delete_issue_source(&state.db, id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// `POST /api/v1/sources/sync` - run an immediate issue sync ("Check issues").
pub async fn sync(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    crate::orchestrator::sync_once(&state).await?;
    Ok(Json(serde_json::json!({ "status": "synced" })))
}
