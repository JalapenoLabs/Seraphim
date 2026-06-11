//! Task detail endpoint (card + its full event history).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{EnvSuggestion, Event, Question, SourceKind, Task};
use crate::db::queries;
use crate::git;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    pub task: Task,
    pub events: Vec<Event>,
    /// Setup recommendations the agent made on this task.
    pub suggestions: Vec<EnvSuggestion>,
    /// Every decision the agent escalated on this task, answered or pending.
    pub questions: Vec<Question>,
}

/// `GET /api/v1/tasks/:id` - the card, its conversation events, its environment
/// suggestions, and its escalated questions.
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::response::Response> {
    let Some(task) = queries::get_task(&state.db, id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        )
            .into_response());
    };
    let events = queries::list_events_for_task(&state.db, id).await?;
    let suggestions = queries::list_suggestions_for_task(&state.db, id).await?;
    let questions = queries::list_questions_for_task(&state.db, id).await?;
    Ok(Json(TaskDetail {
        task,
        events,
        suggestions,
        questions,
    })
    .into_response())
}

#[derive(Debug, Deserialize)]
pub struct NotesRequest {
    pub notes: String,
}

/// `PUT /api/v1/tasks/:id/notes` - save the operator's private scratchpad for a
/// task. Stored only in our database; never sent to the source ticket. No board
/// notification, since notes are private and change nothing others can see.
pub async fn set_notes(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<NotesRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    queries::set_task_notes(&state.db, id, &body.notes).await?;
    Ok(Json(json!({ "saved": true })))
}

/// Resolves a task to its GitHub `(owner, repo, issue number)`, or an error
/// response when the task isn't a GitHub issue with a linked repository.
async fn issue_coords(
    state: &AppState,
    id: Uuid,
) -> ApiResult<Result<(String, String, String), Response>> {
    let Some(task) = queries::get_task(&state.db, id).await? else {
        return Ok(Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        )
            .into_response()));
    };
    if task.source_kind != SourceKind::Github {
        return Ok(Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "task is not a GitHub issue" })),
        )
            .into_response()));
    }
    let Some(repo_id) = task.repo_id else {
        return Ok(Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "task has no repository" })),
        )
            .into_response()));
    };
    let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
        return Ok(Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "repository not found" })),
        )
            .into_response()));
    };
    let Some((owner, name)) = repo.full_name.split_once('/') else {
        return Ok(Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "repository full name is not owner/repo" })),
        )
            .into_response()));
    };
    Ok(Ok((owner.to_string(), name.to_string(), task.external_id)))
}

/// `GET /api/v1/tasks/:id/issue` - the GitHub issue thread (body + comments +
/// labels/assignees) for the GitHub-style conversation view.
pub async fn get_issue(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let (owner, name, number) = match issue_coords(&state, id).await? {
        Ok(coords) => coords,
        Err(response) => return Ok(response),
    };
    let github = state.github().await?;
    let thread = git::get_issue_thread(&github, &owner, &name, &number).await?;

    // Reconcile the card's cached state with what GitHub reports now; the issue
    // may have changed outside our close/reopen control (e.g. a merged PR that
    // closed it via a keyword). Only refresh the board when it actually moved.
    if queries::reconcile_task_external_state(&state.db, id, &thread.issue.state).await? {
        state.notify_board();
    }

    Ok(Json(thread).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CommentRequest {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct IssueStateRequest {
    /// `"open"` or `"closed"`.
    pub state: String,
    /// GitHub close reason when closing: `"completed"` or `"not_planned"`.
    pub reason: Option<String>,
}

/// `POST /api/v1/tasks/:id/issue/state` - open or close the issue on GitHub.
pub async fn set_issue_state(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<IssueStateRequest>,
) -> ApiResult<Response> {
    if payload.state != "open" && payload.state != "closed" {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "state must be 'open' or 'closed'" })),
        )
            .into_response());
    }
    let (owner, name, number) = match issue_coords(&state, id).await? {
        Ok(coords) => coords,
        Err(response) => return Ok(response),
    };
    let github = state.github().await?;
    let issue = git::set_issue_state(
        &github,
        &owner,
        &name,
        &number,
        &payload.state,
        payload.reason.as_deref(),
    )
    .await?;

    // Mirror the new state onto the task so the board card reflects it without a
    // round-trip to GitHub, and nudge the board to refresh live.
    queries::set_task_external_state(&state.db, id, &payload.state).await?;
    state.notify_board();

    Ok(Json(issue).into_response())
}

/// `POST /api/v1/tasks/:id/comment` - post a comment to the issue on GitHub.
pub async fn add_comment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<CommentRequest>,
) -> ApiResult<Response> {
    if payload.body.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "comment body is empty" })),
        )
            .into_response());
    }
    let (owner, name, number) = match issue_coords(&state, id).await? {
        Ok(coords) => coords,
        Err(response) => return Ok(response),
    };
    let github = state.github().await?;
    let comment = git::add_issue_comment(&github, &owner, &name, &number, &payload.body).await?;
    Ok(Json(comment).into_response())
}
