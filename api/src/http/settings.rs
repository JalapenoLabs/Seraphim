//! Org/environment settings endpoints, including the agent pause switch and the
//! user-defined environment variables.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as SqlxJson;

use super::ApiResult;
use crate::db::models::{
    AvailabilityWindow, EnvVarWrite, JiraDeployment, NetworkAccessLevel, ReviewPolicy, Settings,
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
    let jira = queries::get_jira_token(&state.db).await?;
    settings.claude_token_preview = (!claude.is_empty()).then(|| mask(&claude));
    settings.github_token_preview = (!github.is_empty()).then(|| mask(&github));
    settings.jira_token_preview = (!jira.is_empty()).then(|| mask(&jira));
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
    pub usage_limit_pause_enabled: Option<bool>,
    pub usage_limit_threshold: Option<i32>,
    pub post_thoughts_enabled: Option<bool>,
    pub close_issue_on_done: Option<bool>,
    pub jira_enabled: Option<bool>,
    pub jira_deployment: Option<JiraDeployment>,
    pub jira_base_url: Option<String>,
    pub jira_email: Option<String>,
    pub attention_sound_enabled: Option<bool>,
    pub completion_sound_enabled: Option<bool>,
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
        body.usage_limit_pause_enabled,
        body.usage_limit_threshold,
        body.post_thoughts_enabled,
        body.jira_enabled,
        body.jira_deployment,
        body.jira_base_url,
        body.jira_email,
        body.close_issue_on_done,
        body.attention_sound_enabled,
        body.completion_sound_enabled,
    )
    .await?;
    state.notify_board();
    Ok(Json(settings_view(&state).await?))
}

#[derive(Debug, Deserialize)]
pub struct TokensRequest {
    pub claude_oauth_token: Option<String>,
    pub github_token: Option<String>,
    pub jira_api_token: Option<String>,
    pub github_webhook_secret: Option<String>,
    pub jira_webhook_secret: Option<String>,
}

/// `POST /api/v1/settings/tokens` - store the app tokens and webhook secrets
/// (write-only). Empty values are ignored so you can set one without resending
/// the others, and the raw secrets are never returned by the API.
pub async fn set_tokens(
    State(state): State<AppState>,
    Json(body): Json<TokensRequest>,
) -> ApiResult<Json<Settings>> {
    queries::set_tokens(
        &state.db,
        body.claude_oauth_token.filter(|token| !token.is_empty()),
        body.github_token.filter(|token| !token.is_empty()),
        body.jira_api_token.filter(|token| !token.is_empty()),
        body.github_webhook_secret
            .filter(|secret| !secret.is_empty()),
        body.jira_webhook_secret.filter(|secret| !secret.is_empty()),
    )
    .await?;
    Ok(Json(settings_view(&state).await?))
}

// --- Claude authentication ---------------------------------------------------

#[derive(Debug, Serialize)]
pub struct OauthStartResponse {
    /// The consent URL to open in a new tab.
    pub authorize_url: String,
}

/// `POST /api/v1/settings/claude/oauth/start` - begins a Claude subscription
/// OAuth login. Returns the consent URL; the PKCE secrets are held server-side
/// until the operator pastes the resulting code back via `.../oauth/finish`.
pub async fn claude_oauth_start(
    State(state): State<AppState>,
) -> ApiResult<Json<OauthStartResponse>> {
    let (authorize_url, pending) = crate::claude::oauth::start();
    state.set_pending_oauth(pending);
    Ok(Json(OauthStartResponse { authorize_url }))
}

#[derive(Debug, Deserialize)]
pub struct OauthFinishRequest {
    /// The value from the consent callback page (`<code>#<state>` or a bare code).
    pub code: String,
}

/// `POST /api/v1/settings/claude/oauth/finish` - completes the login: exchanges
/// the pasted code, mints the long-lived inference token the agent runs on, and
/// stores it alongside the refreshing usage credentials (switching to
/// subscription mode).
pub async fn claude_oauth_finish(
    State(state): State<AppState>,
    Json(body): Json<OauthFinishRequest>,
) -> ApiResult<Json<Settings>> {
    let pending = state
        .take_pending_oauth()
        .ok_or_else(|| eyre::eyre!("no Claude login is in progress; start one first"))?;
    let tokens =
        crate::claude::oauth::exchange_code(&body.code, &pending.verifier, &pending.state).await?;
    let inference_token = crate::claude::oauth::mint_inference_token(&tokens.access_token).await?;
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(tokens.expires_in);
    queries::set_subscription_credentials(
        &state.db,
        &inference_token,
        &tokens.access_token,
        &tokens.refresh_token,
        expires_at,
        &tokens.scopes,
    )
    .await?;
    Ok(Json(settings_view(&state).await?))
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyRequest {
    pub api_key: String,
}

/// `POST /api/v1/settings/claude/api-key` - stores an Anthropic API key and
/// switches the agent to API-key auth (no subscription usage gauge applies).
pub async fn claude_api_key(
    State(state): State<AppState>,
    Json(body): Json<ApiKeyRequest>,
) -> ApiResult<Json<Settings>> {
    let key = body.api_key.trim();
    if key.is_empty() {
        return Err(eyre::eyre!("the API key is empty").into());
    }
    queries::set_api_key(&state.db, key).await?;
    Ok(Json(settings_view(&state).await?))
}

// --- Notification sounds ------------------------------------------------------

/// The biggest custom clip we accept. Notification sounds are short, so this is
/// generous; it also keeps a stray large upload from bloating the settings row.
const MAX_SOUND_BYTES: usize = 1_048_576;

/// Which notification event a `:kind` path segment refers to.
enum SoundKind {
    Attention,
    Completion,
}

impl SoundKind {
    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "attention" => Some(Self::Attention),
            "completion" => Some(Self::Completion),
            _ => None,
        }
    }
}

/// `GET /api/v1/settings/sounds/:kind` - stream the uploaded custom clip for an
/// event (`attention` or `completion`). Returns 404 when none is uploaded, which
/// the UI treats as "play the bundled default".
pub async fn get_sound(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> ApiResult<Response> {
    let Some(kind) = SoundKind::parse(&kind) else {
        return Ok((StatusCode::NOT_FOUND, "unknown sound").into_response());
    };
    let (audio, mime) = match kind {
        SoundKind::Attention => queries::get_attention_sound(&state.db).await?,
        SoundKind::Completion => queries::get_completion_sound(&state.db).await?,
    };
    if audio.is_empty() {
        return Ok((StatusCode::NOT_FOUND, "no custom sound").into_response());
    }
    let content_type = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            // The settings row carries no version, so never let a stale clip linger.
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        audio,
    )
        .into_response())
}

/// `POST /api/v1/settings/sounds/:kind` - upload a custom clip (the raw audio is
/// the request body; its `Content-Type` is stored so playback gets the right MIME).
/// Responds with the refreshed settings so the UI's "custom set" flag updates.
pub async fn upload_sound(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let Some(kind) = SoundKind::parse(&kind) else {
        return Ok((StatusCode::NOT_FOUND, "unknown sound").into_response());
    };
    if body.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, "empty audio").into_response());
    }
    if body.len() > MAX_SOUND_BYTES {
        return Ok((
            StatusCode::PAYLOAD_TOO_LARGE,
            "audio is too large (max 1 MB); use a short clip",
        )
            .into_response());
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !mime.starts_with("audio/") {
        return Ok((StatusCode::BAD_REQUEST, "file must be an audio clip").into_response());
    }

    match kind {
        SoundKind::Attention => queries::set_attention_sound(&state.db, &body, mime).await?,
        SoundKind::Completion => queries::set_completion_sound(&state.db, &body, mime).await?,
    }
    state.notify_board();
    Ok(Json(settings_view(&state).await?).into_response())
}

/// `DELETE /api/v1/settings/sounds/:kind` - clear a custom clip so the event falls
/// back to the bundled default chime.
pub async fn clear_sound(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> ApiResult<Response> {
    let Some(kind) = SoundKind::parse(&kind) else {
        return Ok((StatusCode::NOT_FOUND, "unknown sound").into_response());
    };
    match kind {
        SoundKind::Attention => queries::set_attention_sound(&state.db, &[], "").await?,
        SoundKind::Completion => queries::set_completion_sound(&state.db, &[], "").await?,
    }
    state.notify_board();
    Ok(Json(settings_view(&state).await?).into_response())
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
