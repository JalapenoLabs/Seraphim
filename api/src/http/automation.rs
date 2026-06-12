//! Automation rules CRUD. The rule's triggers, condition group, and action are
//! validated against the typed `automation` structs as they deserialize, so a
//! malformed rule is rejected at the boundary.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::ApiResult;
use crate::automation::{RuleAction, RuleGroup, Trigger};
use crate::db::models::AutomationRule;
use crate::db::queries;
use crate::state::AppState;

/// `GET /api/v1/automation/rules` - every rule, in evaluation order.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<AutomationRule>>> {
    Ok(Json(queries::list_automation_rules(&state.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct RuleRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_source")]
    pub source_kind: String,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    pub criteria: RuleGroup,
    pub action: RuleAction,
}

fn default_true() -> bool {
    true
}

fn default_source() -> String {
    "github".to_string()
}

/// `POST /api/v1/automation/rules` - create a rule, appended after the others.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<RuleRequest>,
) -> ApiResult<Json<AutomationRule>> {
    let position = queries::max_automation_rule_position(&state.db)
        .await?
        .unwrap_or(0.0)
        + 1.0;
    let rule = queries::create_automation_rule(
        &state.db,
        &body.name,
        body.enabled,
        &body.source_kind,
        &body.triggers,
        &body.criteria,
        &body.action,
        position,
    )
    .await?;
    Ok(Json(rule))
}

/// `PUT /api/v1/automation/rules/:id` - replace a rule's editable fields.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<RuleRequest>,
) -> ApiResult<Response> {
    let updated = queries::update_automation_rule(
        &state.db,
        id,
        &body.name,
        body.enabled,
        &body.source_kind,
        &body.triggers,
        &body.criteria,
        &body.action,
    )
    .await?;
    match updated {
        Some(rule) => Ok(Json(rule).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

/// `DELETE /api/v1/automation/rules/:id`
pub async fn delete(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    if queries::delete_automation_rule(&state.db, id).await? {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok(StatusCode::NOT_FOUND.into_response())
    }
}
