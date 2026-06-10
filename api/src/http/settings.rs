//! Org/environment settings endpoints, including the agent pause switch.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use super::ApiResult;
use crate::db::models::{ReviewPolicy, Settings};
use crate::db::queries;
use crate::state::AppState;

/// `GET /api/v1/settings`
pub async fn get(State(state): State<AppState>) -> ApiResult<Json<Settings>> {
    Ok(Json(queries::get_settings(&state.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub org_name: Option<String>,
    pub global_instructions: Option<String>,
    pub default_review_policy: Option<ReviewPolicy>,
    pub claude_model: Option<String>,
    pub base_setup_script: Option<String>,
    pub config_repo_url: Option<String>,
    pub default_branch_template: Option<String>,
}

/// `PATCH /api/v1/settings` - patch the org profile (omitted fields untouched).
pub async fn update(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsRequest>,
) -> ApiResult<Json<Settings>> {
    let settings = queries::update_settings(
        &state.db,
        body.org_name,
        body.global_instructions,
        body.default_review_policy,
        body.claude_model,
        body.base_setup_script,
        body.config_repo_url,
        body.default_branch_template,
    )
    .await?;
    state.notify_board();
    Ok(Json(settings))
}

#[derive(Debug, Deserialize)]
pub struct PauseRequest {
    pub paused: bool,
}

/// `POST /api/v1/settings/pause` - stop or resume the agent pulling new work.
pub async fn set_pause(
    State(state): State<AppState>,
    Json(body): Json<PauseRequest>,
) -> ApiResult<Json<Settings>> {
    queries::set_paused(&state.db, body.paused).await?;
    state.notify_board();
    Ok(Json(queries::get_settings(&state.db).await?))
}
