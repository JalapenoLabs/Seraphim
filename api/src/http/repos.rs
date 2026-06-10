//! Repository configuration, issue sync, and org import.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{Repository, ReviewPolicy};
use crate::db::queries;
use crate::git;
use crate::state::AppState;

/// `GET /api/v1/repos`
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Repository>>> {
    Ok(Json(queries::list_repositories(&state.db).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpsertRepoRequest {
    pub full_name: String,
    pub clone_url: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    /// Per-repo override of the global branch template; omitted/blank inherits it.
    pub branch_template: Option<String>,
    #[serde(default)]
    pub setup_script: String,
    #[serde(default)]
    pub instructions: String,
    pub review_policy: Option<ReviewPolicy>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub sync_issues: bool,
    #[serde(default)]
    pub issue_labels: Vec<String>,
}

fn default_branch() -> String {
    "main".to_string()
}

/// A blank per-repo template means "inherit the global default", so normalize it
/// to `None` (matching how an omitted field deserializes).
fn branch_template_override(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|template| !template.is_empty())
}

fn default_true() -> bool {
    true
}

/// `POST /api/v1/repos` - create or update a repository by `full_name`.
pub async fn upsert(
    State(state): State<AppState>,
    Json(body): Json<UpsertRepoRequest>,
) -> ApiResult<Json<Repository>> {
    let repo = queries::upsert_repository(
        &state.db,
        &body.full_name,
        &body.clone_url,
        &body.default_branch,
        branch_template_override(&body.branch_template),
        &body.setup_script,
        &body.instructions,
        body.review_policy,
        body.enabled,
        body.sync_issues,
        &body.issue_labels,
    )
    .await?;
    state.notify_board();
    Ok(Json(repo))
}

/// `PUT /api/v1/repos/:id` - update a repository by id (rename-safe, so editing
/// the full name renames the row instead of creating a duplicate).
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpsertRepoRequest>,
) -> ApiResult<Json<Repository>> {
    let repo = queries::update_repository(
        &state.db,
        id,
        &body.full_name,
        &body.clone_url,
        &body.default_branch,
        branch_template_override(&body.branch_template),
        &body.setup_script,
        &body.instructions,
        body.review_policy,
        body.enabled,
        body.sync_issues,
        &body.issue_labels,
    )
    .await?;
    state.notify_board();
    Ok(Json(repo))
}

/// `DELETE /api/v1/repos/:id`
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    queries::delete_repository(&state.db, id).await?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Debug, Deserialize)]
pub struct ImportOrgRequest {
    pub owner: String,
    /// Optional label filter applied to every imported repo's issue sync.
    #[serde(default)]
    pub issue_labels: Vec<String>,
}

/// `POST /api/v1/repos/import-org` - discover every repo under an org/user and
/// add the ones we don't already track (issue-syncing on, your org defaults).
pub async fn import_org(
    State(state): State<AppState>,
    Json(body): Json<ImportOrgRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let github = state.github().await?;
    let discovered = git::list_org_repos(&github, &body.owner).await?;

    let mut imported = 0_usize;
    for repo in &discovered {
        let existed = queries::get_repository_by_full_name(&state.db, &repo.full_name)
            .await?
            .is_some();
        // Newly discovered repos inherit the global branch template (override later).
        queries::create_repository_if_absent(
            &state.db,
            &repo.full_name,
            &repo.clone_url,
            &repo.default_branch,
            None,
            true,
            &body.issue_labels,
        )
        .await?;
        if !existed {
            imported += 1;
        }
    }

    state.notify_board();
    Ok(Json(json!({
        "discovered": discovered.len(),
        "imported": imported,
    })))
}

/// `POST /api/v1/sync` - run an immediate issue sync ("Check issues").
pub async fn sync(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    crate::orchestrator::sync_once(&state).await?;
    Ok(Json(json!({ "status": "synced" })))
}
