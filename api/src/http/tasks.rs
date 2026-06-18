//! Task detail endpoint (card + its full event history).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{
    EnvSuggestion, Event, Question, SourceKind, Task, TaskAttachment, TaskColumn, TaskPullRequest,
    TaskScreenshot,
};
use crate::db::queries;
use crate::git;
use crate::git::{IssueComment, IssueDetail, IssueThread, IssueUser};
use crate::state::AppState;

/// The display login for an internal-comment author ("user" -> the operator,
/// "agent" -> Seraphim).
fn internal_login(author: &str) -> String {
    if author == "agent" { "Seraphim" } else { "You" }.to_string()
}

/// An `IssueUser` with no GitHub backing, for the internal conversation view.
fn internal_user(login: String) -> IssueUser {
    IssueUser {
        login,
        avatar_url: String::new(),
        html_url: String::new(),
    }
}

/// The synthetic issue header for an internal ticket, built from the task row.
fn internal_issue_detail(task: &Task, state: &str) -> IssueDetail {
    IssueDetail {
        number: task.external_id.parse().unwrap_or(0),
        title: task.title.clone(),
        state: state.to_string(),
        user: internal_user("You".to_string()),
        body: Some(task.body_snapshot.clone()),
        created_at: task.created_at.to_rfc3339(),
        author_association: String::new(),
        labels: Vec::new(),
        assignees: Vec::new(),
        milestone: None,
    }
}

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    pub task: Task,
    pub events: Vec<Event>,
    /// Setup recommendations the agent made on this task.
    pub suggestions: Vec<EnvSuggestion>,
    /// Every decision the agent escalated on this task, answered or pending.
    pub questions: Vec<Question>,
    /// Every pull request the task has opened, across all repos it spans. The
    /// review loop gates Done on all of them passing CI and merging.
    pub pull_requests: Vec<TaskPullRequest>,
    /// Screenshots the agent captured during the task (issue #248), newest first,
    /// metadata only; the bytes stream from `/screenshots/:id`.
    pub screenshots: Vec<TaskScreenshot>,
    /// Attachments on the ticket (issue #291): operator uploads and pulled
    /// source-ticket files, oldest first, metadata only; the bytes stream from
    /// `/attachments/:id`.
    pub attachments: Vec<TaskAttachment>,
}

/// `GET /api/v1/tasks/:id` - the card, its conversation events, its environment
/// suggestions, its escalated questions, its pull requests, and its screenshots.
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
    let pull_requests = queries::list_task_prs(&state.db, id).await?;
    let screenshots = queries::list_screenshots_for_task(&state.db, id).await?;
    let attachments = queries::list_attachments_for_task(&state.db, id).await?;
    Ok(Json(TaskDetail {
        task,
        events,
        suggestions,
        questions,
        pull_requests,
        screenshots,
        attachments,
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

/// `POST /api/v1/tasks/:id/reset` - hard-reset a stuck task: stop the agent if it
/// is mid-turn on it, close its PR, delete its branch (remote + workspace), reopen
/// a closed source issue, and return the card to Available. Returns a summary of
/// what was done so the UI can confirm it.
pub async fn hard_reset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<crate::orchestrator::ResetSummary>> {
    let summary = crate::orchestrator::reset_task(&state, id).await?;
    Ok(Json(summary))
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

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// `"open"` (default) or `"closed"`.
    #[serde(default)]
    pub state: Option<String>,
    /// Target repos the ticket affects, in priority order; the first is the
    /// primary one the agent branches in (and that makes it auto-pullable). The
    /// agent is told about all of them but may open a PR in only some. Empty (or
    /// omitted) leaves the ticket tracking-only until repos are assigned.
    #[serde(default)]
    pub repo_ids: Option<Vec<Uuid>>,
    /// Legacy single-repo field, still accepted: treated as a one-entry
    /// `repo_ids` when that newer field is absent.
    #[serde(default)]
    pub repo_id: Option<Uuid>,
}

/// `POST /api/v1/tasks` - create an internal ticket (no external tracker). Lands
/// in `Available` for the operator to triage onto the board.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> ApiResult<Response> {
    let title = body.title.trim();
    if title.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title is required" })),
        )
            .into_response());
    }
    let ticket_state = if body.state.as_deref() == Some("closed") {
        "closed"
    } else {
        "open"
    };
    let position = queries::max_position_in_column(&state.db, TaskColumn::Available)
        .await?
        .unwrap_or(0.0)
        + 1.0;
    // Prefer the multi-repo field; fall back to the legacy single `repo_id`.
    let repo_ids = body
        .repo_ids
        .clone()
        .unwrap_or_else(|| body.repo_id.into_iter().collect());
    let task = queries::create_internal_task(
        &state.db,
        title,
        body.body.trim(),
        ticket_state,
        &repo_ids,
        position,
    )
    .await?;
    state.notify_board();
    Ok(Json(task).into_response())
}

#[derive(Debug, Deserialize)]
pub struct SetTaskRepoRequest {
    /// The target repos in priority order, or an empty list to clear them (back
    /// to tracking-only). The first becomes the primary repo the agent branches in.
    #[serde(default)]
    pub repo_ids: Option<Vec<Uuid>>,
    /// Legacy single-repo field, still accepted when `repo_ids` is absent.
    #[serde(default)]
    pub repo_id: Option<Uuid>,
}

/// `POST /api/v1/tasks/:id/repo` - set the repos an internal ticket targets (or
/// clear them). Only valid for internal tickets; a GitHub task's repo is its
/// issue's and is never reassigned.
pub async fn set_repo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<SetTaskRepoRequest>,
) -> ApiResult<Response> {
    // Prefer the multi-repo field; fall back to the legacy single `repo_id`.
    let repo_ids = payload
        .repo_ids
        .clone()
        .unwrap_or_else(|| payload.repo_id.into_iter().collect());
    match queries::set_internal_task_repos(&state.db, id, &repo_ids).await? {
        Some(task) => {
            state.notify_board();
            Ok(Json(task).into_response())
        }
        None => Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "not an internal ticket" })),
        )
            .into_response()),
    }
}

/// `GET /api/v1/tasks/:id/issue` - the conversation view: a real GitHub issue
/// thread, or a synthetic one (body + DB comments + state) for an internal ticket.
pub async fn get_issue(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    if let Some(task) = queries::get_task(&state.db, id).await? {
        if task.source_kind == SourceKind::Internal {
            let comments = queries::list_internal_comments(&state.db, id)
                .await?
                .into_iter()
                .map(|comment| IssueComment {
                    user: internal_user(internal_login(&comment.author)),
                    body: Some(comment.body),
                    created_at: comment.created_at.to_rfc3339(),
                    author_association: String::new(),
                })
                .collect();
            let state_str = task.external_state.clone().unwrap_or_else(|| "open".into());
            let thread = IssueThread {
                issue: internal_issue_detail(&task, &state_str),
                comments,
            };
            return Ok(Json(thread).into_response());
        }
    }

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
    /// For internal tickets: "user" (default) or "agent". Ignored for GitHub.
    #[serde(default)]
    pub author: Option<String>,
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

    // Internal tickets have no external service; toggle the state in our DB.
    if let Some(task) = queries::get_task(&state.db, id).await? {
        if task.source_kind == SourceKind::Internal {
            queries::set_task_external_state(&state.db, id, &payload.state).await?;
            state.notify_board();
            return Ok(Json(internal_issue_detail(&task, &payload.state)).into_response());
        }
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

    // Internal tickets store comments in our DB rather than on a service.
    if let Some(task) = queries::get_task(&state.db, id).await? {
        if task.source_kind == SourceKind::Internal {
            let author = match payload.author.as_deref() {
                Some("agent") => "agent",
                _ => "user",
            };
            let comment =
                queries::add_internal_comment(&state.db, id, author, &payload.body).await?;
            let view = IssueComment {
                user: internal_user(internal_login(&comment.author)),
                body: Some(comment.body),
                created_at: comment.created_at.to_rfc3339(),
                author_association: String::new(),
            };
            return Ok(Json(view).into_response());
        }
    }

    let (owner, name, number) = match issue_coords(&state, id).await? {
        Ok(coords) => coords,
        Err(response) => return Ok(response),
    };
    let github = state.github().await?;
    let comment = git::add_issue_comment(&github, &owner, &name, &number, &payload.body).await?;
    Ok(Json(comment).into_response())
}
