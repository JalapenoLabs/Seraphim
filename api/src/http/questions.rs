//! Questions the agent escalates to the user, and the user's answers.
//!
//! Three audiences share this module:
//! - the agent's `seraphim-ask` helper posts questions (`POST /agent/questions`);
//! - the notifications sidebar lists what is pending (`GET /questions/pending`);
//! - the task view submits answers (`POST /questions/:id/answer`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{AnswerKind, QuestionOption, QuestionStatus, TaskStatus};
use crate::db::queries;
use crate::state::AppState;

/// The most suggested answers a single question may carry.
const MAX_OPTIONS: usize = 3;

#[derive(Debug, Deserialize)]
pub struct AskQuestion {
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Deserialize)]
pub struct AskRequest {
    pub task_id: Uuid,
    pub questions: Vec<AskQuestion>,
}

/// `POST /api/v1/agent/questions` - the agent escalates one or more questions.
///
/// Called from inside the workspace by `seraphim-ask`. The task is parked in
/// `waiting_for_input` and the user is notified; the orchestrator resumes the
/// agent once every question is answered.
pub async fn ask(
    State(state): State<AppState>,
    Json(body): Json<AskRequest>,
) -> ApiResult<axum::response::Response> {
    let Some(task) = queries::get_task(&state.db, body.task_id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        )
            .into_response());
    };
    if body.questions.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no questions provided" })),
        )
            .into_response());
    }

    let mut ids = Vec::with_capacity(body.questions.len());
    for question in body.questions {
        // Keep at most the first few options; the UI always adds its own
        // "something else" and "decline" choices.
        let options: Vec<QuestionOption> = question.options.into_iter().take(MAX_OPTIONS).collect();
        let created =
            queries::create_question(&state.db, task.id, &question.prompt, &options).await?;
        state.notify_question(task.id, task.title.clone(), created.prompt.clone());
        ids.push(created.id);
    }

    queries::set_task_status(&state.db, task.id, TaskStatus::WaitingForInput).await?;
    state.notify_board();

    Ok(Json(json!({ "question_ids": ids })).into_response())
}

/// `GET /api/v1/questions/pending` - everything awaiting an answer, for the
/// notifications sidebar.
pub async fn pending(State(state): State<AppState>) -> ApiResult<axum::response::Response> {
    let questions = queries::list_pending_questions(&state.db).await?;
    Ok(Json(json!({ "questions": questions })).into_response())
}

#[derive(Debug, Deserialize)]
pub struct AnswerRequest {
    pub kind: AnswerKind,
    /// The chosen option's title, the custom text, or a note when declining.
    #[serde(default)]
    pub text: String,
}

/// `POST /api/v1/questions/:id/answer` - the user answers a pending question.
pub async fn answer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AnswerRequest>,
) -> ApiResult<axum::response::Response> {
    let Some(question) = queries::get_question(&state.db, id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "question not found" })),
        )
            .into_response());
    };
    if question.status != QuestionStatus::Pending {
        // Already answered; return it unchanged so a double-submit is harmless.
        return Ok(Json(question).into_response());
    }

    let status = match body.kind {
        AnswerKind::Declined => QuestionStatus::Declined,
        AnswerKind::Option | AnswerKind::Custom => QuestionStatus::Answered,
    };
    let answered = queries::answer_question(&state.db, id, status, body.kind, &body.text).await?;

    // The board reflects the status, and once nothing is pending the agent loop
    // picks the task up to resume (see `queries::pick_resume_ready`).
    state.notify_board();

    Ok(Json(answered).into_response())
}
