//! Recommendations the agent makes about a task, and the user acting on them.
//!
//! Two kinds share this pipeline: **environment** setup tips (the `seraphim-suggest`
//! helper) and end-of-task **follow-up work** the agent noticed, e.g. cleanup, tech
//! debt, dead code, or security gaps (the `seraphim-followup` helper, issue #272).
//! Both post to `POST /agent/suggestions`; the task view checks them off
//! (`POST /suggestions/:id/ack`) or one-clicks one into a tracked issue
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
    /// `"follow_up"` for end-of-task follow-up work (issue #272); anything else
    /// (including omitted) is an environment setup recommendation.
    #[serde(default)]
    pub kind: Option<String>,
}

/// `POST /api/v1/agent/suggestions` - the agent records recommendations.
///
/// Called from inside the workspace by `seraphim-suggest` (environment) and
/// `seraphim-followup` (`kind = "follow_up"`). Blank-titled entries are skipped,
/// and the board badge lights up for the task. For follow-up work, anything whose
/// title already matches a task still on the board is dropped, so the agent never
/// recommends work that is already queued (issue #272).
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

    let kind = if body.kind.as_deref() == Some("follow_up") {
        "follow_up"
    } else {
        "environment"
    };
    // The light de-dup applies only to follow-up work (environment tips are not
    // board tasks); fetch the open board's titles once to compare against.
    let queued_titles = if kind == "follow_up" {
        queries::open_task_titles(&state.db).await?
    } else {
        Vec::new()
    };

    let mut ids = Vec::new();
    let mut skipped_duplicates = 0;
    for suggestion in body.suggestions.into_iter().take(MAX_SUGGESTIONS) {
        let title = suggestion.title.trim();
        if title.is_empty() {
            continue;
        }
        if kind == "follow_up" && already_queued(title, &queued_titles) {
            skipped_duplicates += 1;
            continue;
        }
        let created = queries::create_suggestion(
            &state.db,
            body.task_id,
            kind,
            title,
            suggestion.detail.trim(),
        )
        .await?;
        ids.push(created.id);
    }

    // The board badge reflects the new unacknowledged suggestions.
    state.notify_board();

    Ok(
        Json(json!({ "suggestion_ids": ids, "skipped_duplicates": skipped_duplicates }))
            .into_response(),
    )
}

/// Normalizes a title for the light duplicate check: lowercased, with every run of
/// non-alphanumeric characters collapsed to a single space and the ends trimmed. So
/// "Add a security layer!" and "add  a  security layer" compare equal.
fn normalize_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_space = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_space = true;
        }
    }
    out
}

/// Whether a follow-up `title` is already represented by one of the `queued` task
/// titles. Deliberately light and deterministic (issue #272): a hit is normalized
/// equality, or one normalized title fully containing the other when the shorter is
/// substantial, to catch obvious restatements without over-matching on a shared
/// word. Not a fuzzy/semantic match.
fn already_queued(title: &str, queued: &[String]) -> bool {
    // Below this many characters the containment rule is off, so short titles only
    // match exactly (otherwise a common phrase would swallow unrelated work).
    const MIN_CONTAIN_CHARS: usize = 12;
    let needle = normalize_title(title);
    if needle.is_empty() {
        return false;
    }
    queued.iter().any(|other| {
        let hay = normalize_title(other);
        if hay == needle {
            return true;
        }
        let (short, long) = if needle.len() <= hay.len() {
            (needle.as_str(), hay.as_str())
        } else {
            (hay.as_str(), needle.as_str())
        };
        short.len() >= MIN_CONTAIN_CHARS && long.contains(short)
    })
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
            // No target repo: a recommendation is tracking-only until the operator
            // assigns a repo (on the task page), keeping its prior behavior.
            queries::create_internal_task(&state.db, &title, &detail, "open", &[], position)
                .await?;
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

#[cfg(test)]
mod tests {
    use super::{already_queued, normalize_title};

    #[test]
    fn normalize_collapses_case_punctuation_and_spacing() {
        assert_eq!(
            normalize_title("Add a security layer!"),
            "add a security layer"
        );
        assert_eq!(
            normalize_title("  Dead-code   removal  "),
            "dead code removal"
        );
        assert_eq!(normalize_title("!!!"), "");
    }

    #[test]
    fn already_queued_matches_restatements_not_shared_words() {
        let queued = vec![
            "Add a security layer to uploads".to_string(),
            "Photo upload system".to_string(),
        ];
        // Exact restatement (case/punctuation aside) is a duplicate.
        assert!(already_queued("photo upload system", &queued));
        // A substantial restatement contained in a queued title is a duplicate.
        assert!(already_queued("Add a security layer!", &queued));
        // Genuinely new work is not a duplicate, even sharing a word ("upload").
        assert!(!already_queued("Add thumbnail generation", &queued));
        // A short shared phrase must NOT over-match (containment rule is off).
        assert!(!already_queued("add a test", &queued));
    }

    #[test]
    fn already_queued_is_false_against_an_empty_board() {
        assert!(!already_queued("anything at all", &[]));
        assert!(!already_queued("", &["something".to_string()]));
    }
}
