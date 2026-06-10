//! Configuration export/import: back up settings + repos + sources as JSON and
//! restore them on another machine. Secrets are never included (tokens live in
//! the environment, not the database).

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ApiResult;
use crate::db::models::{ReviewPolicy, SourceKind};
use crate::db::queries;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsExport {
    pub org_name: String,
    pub global_instructions: String,
    pub default_review_policy: ReviewPolicy,
    pub claude_model: String,
    pub base_setup_script: String,
    pub config_repo_url: String,
    pub default_branch_template: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoExport {
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: String,
    pub branch_template: String,
    pub setup_script: String,
    pub instructions: String,
    pub review_policy: Option<ReviewPolicy>,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceExport {
    pub kind: SourceKind,
    pub config: Value,
    pub poll_interval_secs: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigBundle {
    pub settings: SettingsExport,
    pub repositories: Vec<RepoExport>,
    pub sources: Vec<SourceExport>,
}

/// `GET /api/v1/export` - the portable config bundle.
pub async fn export(State(state): State<AppState>) -> ApiResult<Json<ConfigBundle>> {
    let settings = queries::get_settings(&state.db).await?;
    let repositories = queries::list_repositories(&state.db).await?;
    let sources = queries::list_issue_sources(&state.db).await?;

    let bundle = ConfigBundle {
        settings: SettingsExport {
            org_name: settings.org_name,
            global_instructions: settings.global_instructions,
            default_review_policy: settings.default_review_policy,
            claude_model: settings.claude_model,
            base_setup_script: settings.base_setup_script,
            config_repo_url: settings.config_repo_url,
            default_branch_template: settings.default_branch_template,
        },
        repositories: repositories
            .into_iter()
            .map(|repo| RepoExport {
                full_name: repo.full_name,
                clone_url: repo.clone_url,
                default_branch: repo.default_branch,
                branch_template: repo.branch_template,
                setup_script: repo.setup_script,
                instructions: repo.instructions,
                review_policy: repo.review_policy,
                enabled: repo.enabled,
            })
            .collect(),
        sources: sources
            .into_iter()
            .map(|source| SourceExport {
                kind: source.kind,
                config: source.config.0,
                poll_interval_secs: source.poll_interval_secs,
            })
            .collect(),
    };

    Ok(Json(bundle))
}

/// `POST /api/v1/import` - restore a bundle: patch settings, upsert repos, and
/// add any sources not already present (deduped by kind + config).
pub async fn import(
    State(state): State<AppState>,
    Json(bundle): Json<ConfigBundle>,
) -> ApiResult<Json<serde_json::Value>> {
    let settings = bundle.settings;
    queries::update_settings(
        &state.db,
        Some(settings.org_name),
        Some(settings.global_instructions),
        Some(settings.default_review_policy),
        Some(settings.claude_model),
        Some(settings.base_setup_script),
        Some(settings.config_repo_url),
        Some(settings.default_branch_template),
    )
    .await?;

    for repo in &bundle.repositories {
        queries::upsert_repository(
            &state.db,
            &repo.full_name,
            &repo.clone_url,
            &repo.default_branch,
            &repo.branch_template,
            &repo.setup_script,
            &repo.instructions,
            repo.review_policy,
            repo.enabled,
        )
        .await?;
    }

    let existing = queries::list_issue_sources(&state.db).await?;
    for source in &bundle.sources {
        let already_present = existing
            .iter()
            .any(|current| current.kind == source.kind && current.config.0 == source.config);
        if !already_present {
            queries::create_issue_source(
                &state.db,
                source.kind,
                source.config.clone(),
                source.poll_interval_secs,
            )
            .await?;
        }
    }

    state.notify_board();
    Ok(Json(serde_json::json!({
        "imported_repositories": bundle.repositories.len(),
        "imported_sources": bundle.sources.len(),
    })))
}
