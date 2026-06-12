//! Environment setup suggestions the agent makes, and the user acknowledging
//! them.
//!
//! The agent's `seraphim-suggest` helper posts recommendations
//! (`POST /agent/suggestions`); the task view checks them off
//! (`POST /suggestions/:id/ack`) or turns one into a tracked issue
//! (`POST /suggestions/:id/create`). They are listed as part of the task detail.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{EnvSuggestion, EnvSuggestionWrite, Task, TaskColumn};
use crate::db::queries;
use crate::git;
use crate::state::AppState;

/// The most suggestions a single post may record, to bound a runaway agent.
const MAX_SUGGESTIONS: usize = 10;

#[derive(Debug, Deserialize)]
pub struct SuggestRequest {
    pub task_id: Uuid,
    pub suggestions: Vec<EnvSuggestionWrite>,
}

/// `POST /api/v1/agent/suggestions` - the agent records setup recommendations.
///
/// Called from inside the workspace by `seraphim-suggest`. Blank-titled entries
/// are skipped, and the board badge lights up for the task.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<SuggestRequest>,
) -> ApiResult<axum::response::Response> {
    if queries::get_task(&state.db, body.task_id).await?.is_none() {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        )
            .into_response());
    }

    let mut ids = Vec::new();
    for suggestion in body.suggestions.into_iter().take(MAX_SUGGESTIONS) {
        let title = suggestion.title.trim();
        if title.is_empty() {
            continue;
        }
        let created =
            queries::create_suggestion(&state.db, body.task_id, title, suggestion.detail.trim())
                .await?;
        ids.push(created.id);
    }

    // The board badge reflects the new unacknowledged suggestions.
    state.notify_board();

    Ok(Json(json!({ "suggestion_ids": ids })).into_response())
}

#[derive(Debug, Deserialize)]
pub struct AckRequest {
    pub acknowledged: bool,
}

/// `POST /api/v1/suggestions/:id/ack` - the user checks (or unchecks) a
/// suggestion. Once acknowledged it stops being loud on the board.
pub async fn acknowledge(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AckRequest>,
) -> ApiResult<Json<crate::db::models::EnvSuggestion>> {
    let suggestion = queries::set_suggestion_acknowledged(&state.db, id, body.acknowledged).await?;
    state.notify_board();
    Ok(Json(suggestion))
}

/// Where a one-click "create issue from this recommendation" lands.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreateTarget {
    /// A Seraphim-only internal ticket.
    Internal,
    /// A new GitHub issue in the originating task's repo.
    Github,
    /// A new Jira ticket.
    Jira,
}

#[derive(Debug, Deserialize)]
pub struct CreateFromSuggestionRequest {
    pub target: CreateTarget,
}

#[derive(Debug, Serialize)]
pub struct CreateFromSuggestionResponse {
    /// The recommendation, now marked done.
    pub suggestion: EnvSuggestion,
    /// A link to the created issue, when the target has one.
    pub url: Option<String>,
}

/// `POST /api/v1/suggestions/:id/create` - turn a recommendation into a tracked
/// issue (internal / GitHub / Jira), then mark the recommendation done.
pub async fn create_issue(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateFromSuggestionRequest>,
) -> ApiResult<Response> {
    let Some(suggestion) = queries::get_suggestion(&state.db, id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "suggestion not found" })),
        )
            .into_response());
    };
    let task = queries::get_task(&state.db, suggestion.task_id).await?;
    let title = suggestion.title.clone();
    let detail = suggestion.detail.clone();

    let url = match body.target {
        CreateTarget::Internal => {
            // Land it at the bottom of Available, like the manual "create issue".
            let position = queries::max_position_in_column(&state.db, TaskColumn::Available)
                .await?
                .unwrap_or(0.0)
                + 1.0;
            queries::create_internal_task(&state.db, &title, &detail, "open", position).await?;
            None
        }
        CreateTarget::Github => {
            match create_github_issue(&state, task.as_ref(), &title, &detail).await {
                Ok(url) => Some(url),
                Err(message) => return Ok(bad_request(&message)),
            }
        }
        CreateTarget::Jira => match create_jira_issue(&state, task.as_ref(), &title, &detail).await
        {
            Ok(url) => Some(url),
            Err(message) => return Ok(bad_request(&message)),
        },
    };

    // The recommendation has been actioned, so check it off.
    let suggestion = queries::set_suggestion_acknowledged(&state.db, id, true).await?;
    state.notify_board();
    Ok(Json(CreateFromSuggestionResponse { suggestion, url }).into_response())
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

/// Creates the GitHub issue in the originating task's repo, returning its URL or a
/// user-facing reason it can't be created.
async fn create_github_issue(
    state: &AppState,
    task: Option<&Task>,
    title: &str,
    body: &str,
) -> Result<String, String> {
    let Some(repo_id) = task.and_then(|task| task.repo_id) else {
        return Err("This task has no linked repository to open a GitHub issue in.".to_string());
    };
    let repo = queries::get_repository(&state.db, repo_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The linked repository no longer exists.".to_string())?;
    let Some((owner, name)) = repo.full_name.split_once('/') else {
        return Err(format!(
            "Repository '{}' is not owner/repo.",
            repo.full_name
        ));
    };
    let github = state.github().await.map_err(|error| error.to_string())?;
    let issue = git::create_issue(&github, owner, name, title, body)
        .await
        .map_err(|error| error.to_string())?;
    Ok(issue.html_url)
}

/// Creates a Jira ticket (in the task's board project, else the first followed
/// board's project), returning its URL or a user-facing reason it can't be.
async fn create_jira_issue(
    state: &AppState,
    task: Option<&Task>,
    title: &str,
    body: &str,
) -> Result<String, String> {
    let Some(jira) = state.jira().await.map_err(|error| error.to_string())? else {
        return Err("Jira isn't configured. Connect it in Settings first.".to_string());
    };

    // Prefer the project of the task's own Jira board; otherwise the first board.
    let mut project_key = None;
    if let Some(board_id) = task.and_then(|task| task.jira_board_id) {
        project_key = queries::get_jira_board(&state.db, board_id)
            .await
            .map_err(|error| error.to_string())?
            .map(|board| board.project_key);
    }
    if project_key.is_none() {
        project_key = queries::list_jira_boards(&state.db)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
            .map(|board| board.project_key);
    }
    let Some(project_key) = project_key.filter(|key| !key.trim().is_empty()) else {
        return Err("No Jira project to create in. Follow a board in Settings first.".to_string());
    };

    let issue = jira
        .create_issue(&project_key, title, body)
        .await
        .map_err(|error| error.to_string())?;
    Ok(issue.url)
}
