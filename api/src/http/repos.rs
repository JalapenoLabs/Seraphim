//! Repository configuration endpoints.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{Repository, ReviewPolicy};
use crate::db::queries;
use crate::state::AppState;

/// `GET /api/v1/repos`
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Repository>>> {
    Ok(Json(queries::list_repositories(&state.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpsertRepoRequest {
    pub full_name: String,
    pub clone_url: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default = "default_branch_template")]
    pub branch_template: String,
    #[serde(default)]
    pub setup_script: String,
    #[serde(default)]
    pub instructions: String,
    pub review_policy: Option<ReviewPolicy>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_branch_template() -> String {
    "seraphim/issue-{number}-{slug}".to_string()
}

fn default_true() -> bool {
    true
}

/// `POST /api/v1/repos` - create or update a repository by `full_name`.
pub async fn upsert(
    State(state): State<AppState>,
    Json(body): Json<UpsertRepoRequest>,
) -> ApiResult<Json<Repository>> {
    let repo = queries::upsert_repository(
        &state.db,
        &body.full_name,
        &body.clone_url,
        &body.default_branch,
        &body.branch_template,
        &body.setup_script,
        &body.instructions,
        body.review_policy,
        body.enabled,
    )
    .await?;
    Ok(Json(repo))
}

/// `DELETE /api/v1/repos/:id`
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    queries::delete_repository(&state.db, id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
