//! HTTP surface for the compose assistant (issue #181): the chat turn trigger,
//! draft CRUD (including the `seraphim-draft` helper endpoint), reset, and the
//! deterministic bulk-create to GitHub / Jira / internal.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{IssueDraft, TaskColumn};
use crate::db::queries;
use crate::git;
use crate::orchestrator::compose;
use crate::state::AppState;

/// The compose page's initial state: the chat transcript, the current drafts, and
/// whether a turn is in flight (so the input can disable while the agent works).
#[derive(serde::Serialize)]
pub struct ComposeState {
    events: Vec<crate::db::models::Event>,
    drafts: Vec<IssueDraft>,
    running: bool,
}

/// `GET /api/v1/compose` - the transcript, drafts, and running flag.
pub async fn get_state(State(state): State<AppState>) -> ApiResult<Json<ComposeState>> {
    Ok(Json(ComposeState {
        events: queries::list_compose_events(&state.db).await?,
        drafts: queries::list_drafts(&state.db).await?,
        running: queries::compose_turn_running(&state.db).await?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct MessageRequest {
    pub message: String,
}

/// `POST /api/v1/compose/message` - send the assistant a message, running one turn
/// in the background. Rejected while a turn is already in flight (single-threaded
/// like the main agent, but entirely separate from it).
pub async fn message(
    State(state): State<AppState>,
    Json(body): Json<MessageRequest>,
) -> ApiResult<Response> {
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Ok(bad_request("A message is required"));
    }
    if queries::compose_turn_running(&state.db).await? {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({ "error": "the assistant is already responding" })),
        )
            .into_response());
    }
    // Run the turn detached; the page follows it over the compose SSE stream.
    let task_state = state.clone();
    tokio::spawn(async move { compose::run(task_state, message).await });
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": true }))).into_response())
}

/// One draft as the `seraphim-draft` helper sends it: a target `repo` named as
/// `owner/name` (optional), which we resolve to a repo id.
#[derive(Debug, Deserialize)]
pub struct DraftInput {
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// `owner/name` of the target repo, if the issue belongs to one.
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceDraftsRequest {
    pub drafts: Vec<DraftInput>,
}

/// `POST /api/v1/compose/drafts` - replace the whole draft set (the
/// `seraphim-draft` helper always sends the full desired list). Blank-titled
/// entries are dropped; an unknown repo name is kept as no repo.
pub async fn replace_drafts(
    State(state): State<AppState>,
    Json(body): Json<ReplaceDraftsRequest>,
) -> ApiResult<Json<Vec<IssueDraft>>> {
    let repos = queries::list_repositories(&state.db).await?;
    let resolved: Vec<(String, String, Option<Uuid>)> = body
        .drafts
        .into_iter()
        .filter(|draft| !draft.title.trim().is_empty())
        .map(|draft| {
            let repo_id = draft.repo.as_deref().and_then(|name| {
                repos
                    .iter()
                    .find(|repo| repo.full_name.eq_ignore_ascii_case(name.trim()))
                    .map(|repo| repo.id)
            });
            (draft.title, draft.body, repo_id)
        })
        .collect();
    let drafts = queries::replace_drafts(&state.db, &resolved).await?;
    state.notify_compose_changed();
    Ok(Json(drafts))
}

#[derive(Debug, Deserialize)]
pub struct UpdateDraftRequest {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub repo_id: Option<Uuid>,
}

/// `PUT /api/v1/compose/drafts/:id` - the operator's manual edit of one draft.
pub async fn update_draft(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDraftRequest>,
) -> ApiResult<Response> {
    match queries::update_draft(&state.db, id, body.title.trim(), &body.body, body.repo_id).await? {
        Some(draft) => {
            state.notify_compose_changed();
            Ok(Json(draft).into_response())
        }
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "draft not found" })),
        )
            .into_response()),
    }
}

/// `DELETE /api/v1/compose/drafts/:id` - drop one draft.
pub async fn delete_draft(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    queries::delete_draft(&state.db, id).await?;
    state.notify_compose_changed();
    Ok(Json(json!({ "deleted": true })))
}

/// `POST /api/v1/compose/reset` - clear the drafts and wipe the conversation.
pub async fn reset(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    compose::reset(&state).await?;
    Ok(Json(json!({ "reset": true })))
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkTarget {
    Internal,
    Github,
    Jira,
}

#[derive(Debug, Deserialize)]
pub struct BulkCreateRequest {
    pub target: BulkTarget,
}

/// `POST /api/v1/compose/bulk-create` - deterministically create every draft as
/// the chosen tracker's issue, then clear the drafts that succeeded. Per-draft
/// failures are reported but never abort the rest, so a half-created batch is
/// transparent rather than silently lost.
pub async fn bulk_create(
    State(state): State<AppState>,
    Json(body): Json<BulkCreateRequest>,
) -> ApiResult<Response> {
    let drafts = queries::list_drafts(&state.db).await?;
    if drafts.is_empty() {
        return Ok(bad_request("There are no drafts to create"));
    }

    let mut created_urls: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    // Track the ids that succeeded so a partial failure leaves the rest in place.
    let mut created_ids: Vec<Uuid> = Vec::new();

    for draft in &drafts {
        let result = match body.target {
            BulkTarget::Internal => create_internal(&state, draft).await,
            BulkTarget::Github => create_github(&state, draft).await,
            BulkTarget::Jira => create_jira(&state, draft).await,
        };
        match result {
            Ok(url) => {
                created_ids.push(draft.id);
                if let Some(url) = url {
                    created_urls.push(url);
                }
            }
            Err(message) => errors.push(format!("{}: {message}", draft.title)),
        }
    }

    for id in &created_ids {
        queries::delete_draft(&state.db, *id).await?;
    }
    state.notify_compose_changed();
    state.notify_board();

    Ok(Json(json!({
        "created": created_ids.len(),
        "urls": created_urls,
        "errors": errors,
    }))
    .into_response())
}

/// Creates an internal ticket from a draft (its repo, if set, becomes the target).
async fn create_internal(state: &AppState, draft: &IssueDraft) -> Result<Option<String>, String> {
    let position = queries::max_position_in_column(&state.db, TaskColumn::Available)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or(0.0)
        + 1.0;
    queries::create_internal_task(
        &state.db,
        draft.title.trim(),
        &draft.body,
        "open",
        draft.repo_id,
        position,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(None)
}

/// Creates a GitHub issue from a draft in its target repo, returning the URL.
async fn create_github(state: &AppState, draft: &IssueDraft) -> Result<Option<String>, String> {
    let Some(repo_id) = draft.repo_id else {
        return Err("no target repository set for a GitHub issue".to_string());
    };
    let repo = queries::get_repository(&state.db, repo_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the target repository no longer exists".to_string())?;
    let Some((owner, name)) = repo.full_name.split_once('/') else {
        return Err(format!("repository '{}' is not owner/repo", repo.full_name));
    };
    let github = state.github().await.map_err(|error| error.to_string())?;
    let issue = git::create_issue(&github, owner, name, draft.title.trim(), &draft.body)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(issue.html_url))
}

/// Creates a Jira ticket from a draft in the first followed board's project.
async fn create_jira(state: &AppState, draft: &IssueDraft) -> Result<Option<String>, String> {
    let Some(jira) = state.jira().await.map_err(|error| error.to_string())? else {
        return Err("Jira isn't configured; connect it in Settings first".to_string());
    };
    let project_key = queries::list_jira_boards(&state.db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|board| board.project_key)
        .find(|key| !key.trim().is_empty())
        .ok_or_else(|| {
            "no Jira project to create in; follow a board in Settings first".to_string()
        })?;
    let issue = jira
        .create_issue(&project_key, draft.title.trim(), &draft.body)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(issue.url))
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}
