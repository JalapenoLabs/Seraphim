//! Org/environment settings endpoints, including the agent pause switch and the
//! user-defined environment variables.

use axum::extract::State;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as SqlxJson;

use super::ApiResult;
use crate::db::models::{
    AvailabilityWindow, EnvVarWrite, NetworkAccessLevel, ReviewPolicy, Settings,
};
use crate::db::queries;
use crate::secrets::mask;
use crate::state::AppState;

/// Loads the settings and fills in the masked token previews, so the UI can show
/// a recognizable hint of each stored secret without ever receiving the raw value.
async fn settings_view(state: &AppState) -> ApiResult<Settings> {
    let mut settings = queries::get_settings(&state.db).await?;
    let claude = queries::get_claude_token(&state.db).await?;
    let github = queries::get_github_token(&state.db).await?;
    settings.claude_token_preview = (!claude.is_empty()).then(|| mask(&claude));
    settings.github_token_preview = (!github.is_empty()).then(|| mask(&github));
    Ok(settings)
}

/// `GET /api/v1/settings`
pub async fn get(State(state): State<AppState>) -> ApiResult<Json<Settings>> {
    Ok(Json(settings_view(&state).await?))
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
    pub availability_enabled: Option<bool>,
    pub availability_timezone: Option<String>,
    pub availability_windows: Option<Vec<AvailabilityWindow>>,
    pub availability_skip_dates: Option<Vec<NaiveDate>>,
    pub network_access_level: Option<NetworkAccessLevel>,
    pub network_access_domains: Option<Vec<String>>,
    pub network_access_include_defaults: Option<bool>,
}

/// `PATCH /api/v1/settings` - patch the org profile (omitted fields untouched).
pub async fn update(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsRequest>,
) -> ApiResult<Json<Settings>> {
    queries::update_settings(
        &state.db,
        body.org_name,
        body.global_instructions,
        body.default_review_policy,
        body.claude_model,
        body.base_setup_script,
        body.config_repo_url,
        body.default_branch_template,
        body.availability_enabled,
        body.availability_timezone,
        body.availability_windows.map(SqlxJson),
        body.availability_skip_dates.map(SqlxJson),
        body.network_access_level,
        body.network_access_domains.map(SqlxJson),
        body.network_access_include_defaults,
    )
    .await?;
    state.notify_board();
    Ok(Json(settings_view(&state).await?))
}

#[derive(Debug, Deserialize)]
pub struct TokensRequest {
    pub claude_oauth_token: Option<String>,
    pub github_token: Option<String>,
}

/// `POST /api/v1/settings/tokens` - store the app tokens (write-only). Empty
/// values are ignored so you can set one without resending the other, and the
/// raw tokens are never returned by the API.
pub async fn set_tokens(
    State(state): State<AppState>,
    Json(body): Json<TokensRequest>,
) -> ApiResult<Json<Settings>> {
    queries::set_tokens(
        &state.db,
        body.claude_oauth_token.filter(|token| !token.is_empty()),
        body.github_token.filter(|token| !token.is_empty()),
    )
    .await?;
    Ok(Json(settings_view(&state).await?))
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
    Ok(Json(settings_view(&state).await?))
}

// --- Environment variables ---------------------------------------------------

/// One environment variable as the UI sees it. A secret's `value` is the masked
/// preview, never the raw secret.
#[derive(Debug, Serialize)]
pub struct EnvVarView {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

#[derive(Debug, Serialize)]
pub struct EnvVarsResponse {
    pub variables: Vec<EnvVarView>,
}

/// Builds the masked, UI-facing view of the stored environment variables.
async fn env_vars_view(state: &AppState) -> ApiResult<EnvVarsResponse> {
    let variables = queries::list_environment_variables(&state.db)
        .await?
        .into_iter()
        .map(|variable| EnvVarView {
            key: variable.key,
            // Secrets are only ever exposed masked, so the operator can identify
            // a value without it being revealed.
            value: if variable.is_secret {
                mask(&variable.value)
            } else {
                variable.value
            },
            is_secret: variable.is_secret,
        })
        .collect();
    Ok(EnvVarsResponse { variables })
}

/// `GET /api/v1/settings/env` - list environment variables (secrets masked).
pub async fn list_env(State(state): State<AppState>) -> ApiResult<Json<EnvVarsResponse>> {
    Ok(Json(env_vars_view(&state).await?))
}

#[derive(Debug, Deserialize)]
pub struct SetEnvRequest {
    pub variables: Vec<EnvVarWrite>,
}

/// `PUT /api/v1/settings/env` - replace the whole set of environment variables.
///
/// A secret whose `value` is omitted keeps its stored value (the UI never holds
/// the raw secret to resend). Responds with the refreshed, masked list.
pub async fn set_env(
    State(state): State<AppState>,
    Json(body): Json<SetEnvRequest>,
) -> ApiResult<Json<EnvVarsResponse>> {
    queries::replace_environment_variables(&state.db, &body.variables).await?;
    Ok(Json(env_vars_view(&state).await?))
}
