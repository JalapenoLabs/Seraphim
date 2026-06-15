//! Jira admin endpoints: test the stored connection, discover boards, and manage
//! the followed boards (their status->column mapping and repo associations).

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{JiraBoard, TaskColumn};
use crate::db::queries;
use crate::state::AppState;

/// `POST /api/v1/jira/test` - verify the stored connection. Always 200; the body
/// reports success or the failure reason so the UI can show it inline.
pub async fn test(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let Some(client) = state.jira().await? else {
        return Ok(Json(
            json!({ "ok": false, "error": "Jira is not enabled or not fully configured" }),
        ));
    };
    match client.verify().await {
        Ok(identity) => {
            // Capture the account id so the realtime webhook path can tell which
            // tickets are the operator's (it cannot run JQL like the poll does).
            if !identity.account_id.is_empty() {
                queries::set_jira_account_id(&state.db, &identity.account_id).await?;
            }
            Ok(Json(json!({ "ok": true, "user": identity.display_name })))
        }
        Err(error) => Ok(Json(json!({ "ok": false, "error": error.to_string() }))),
    }
}

/// `GET /api/v1/jira/boards` - the boards we follow.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<JiraBoard>>> {
    Ok(Json(queries::list_jira_boards(&state.db).await?))
}

/// `POST /api/v1/jira/discover` - pull boards from Jira and start following any we
/// are not already (without clobbering existing mappings). Returns the full list.
pub async fn discover(State(state): State<AppState>) -> ApiResult<Json<Vec<JiraBoard>>> {
    if let Some(client) = state.jira().await? {
        for board in client.list_boards().await? {
            queries::create_jira_board_if_absent(
                &state.db,
                board.board_id,
                &board.name,
                &board.project_key,
            )
            .await?;
        }
    }
    Ok(Json(queries::list_jira_boards(&state.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBoardRequest {
    pub sync_enabled: bool,
    /// Jira status name -> our kanban column.
    pub status_map: HashMap<String, TaskColumn>,
    /// Repositories a ticket from this board may target.
    pub repo_ids: Vec<Uuid>,
}

/// `PUT /api/v1/jira/boards/:id` - update a board's sync flag, status mapping, and
/// repo associations.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBoardRequest>,
) -> ApiResult<Json<JiraBoard>> {
    let board = queries::update_jira_board(
        &state.db,
        id,
        body.sync_enabled,
        SqlxJson(body.status_map),
        SqlxJson(body.repo_ids),
    )
    .await?;
    Ok(Json(board))
}

/// `DELETE /api/v1/jira/boards/:id` - stop following a board.
pub async fn delete(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    queries::delete_jira_board(&state.db, id).await?;
    Ok(Json(json!({ "deleted": true })))
}
