//! Configuration export/import: back up settings + repos + sources as JSON and
//! restore them on another machine. Secrets are never included (tokens live in
//! the environment, not the database).

use axum::extract::State;
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::types::Json as SqlxJson;

use super::ApiResult;
use crate::db::models::{AvailabilityWindow, JiraDeployment, NetworkAccessLevel, ReviewPolicy};
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
    // Default so bundles exported before the schedule feature still import.
    #[serde(default)]
    pub availability_enabled: bool,
    #[serde(default = "default_timezone")]
    pub availability_timezone: String,
    #[serde(default)]
    pub availability_windows: Vec<AvailabilityWindow>,
    #[serde(default)]
    pub availability_skip_dates: Vec<NaiveDate>,
    // Default so bundles exported before the network-access feature still import.
    #[serde(default = "default_network_level")]
    pub network_access_level: NetworkAccessLevel,
    #[serde(default)]
    pub network_access_domains: Vec<String>,
    #[serde(default = "default_true")]
    pub network_access_include_defaults: bool,
    // Default so bundles exported before the usage-limit feature still import.
    #[serde(default = "default_true")]
    pub usage_limit_pause_enabled: bool,
    #[serde(default = "default_usage_threshold")]
    pub usage_limit_threshold: i32,
    // Default so bundles exported before the railway idle-timeout setting still
    // import; matches the database default (30 minutes).
    #[serde(default = "default_railway_idle_timeout")]
    pub railway_idle_timeout_minutes: i32,
    #[serde(default)]
    pub post_thoughts_enabled: bool,
    // Matches the database default (TRUE) so older bundles keep closing issues.
    #[serde(default = "default_true")]
    pub close_issue_on_done: bool,
    // Notification sound toggles (default on, like the DB). The custom audio clips
    // themselves are machine-local and not part of the bundle.
    #[serde(default = "default_true")]
    pub attention_sound_enabled: bool,
    #[serde(default = "default_true")]
    pub completion_sound_enabled: bool,
    // Jira connection (non-secret parts only; the API token is never exported,
    // like the Claude/GitHub tokens). Followed boards are machine-specific (they
    // reference repo ids), so they are not part of the bundle.
    #[serde(default)]
    pub jira_enabled: bool,
    #[serde(default = "default_jira_deployment")]
    pub jira_deployment: JiraDeployment,
    #[serde(default)]
    pub jira_base_url: String,
    #[serde(default)]
    pub jira_email: String,
    // Matches the database default (TRUE); older bundles import with the filter on.
    #[serde(default = "default_true")]
    pub jira_assigned_to_me_only: bool,
}

/// Matches the database default so an older bundle imports as plain UTC.
fn default_timezone() -> String {
    "UTC".to_string()
}

/// Matches the database default (`full`) so older bundles import unrestricted.
fn default_network_level() -> NetworkAccessLevel {
    NetworkAccessLevel::Full
}

fn default_true() -> bool {
    true
}

/// Matches the database default usage-limit threshold (80%).
fn default_usage_threshold() -> i32 {
    80
}

/// Matches the database default railway idle-stop timeout (30 minutes).
fn default_railway_idle_timeout() -> i32 {
    30
}

/// Matches the database default (`cloud`) so older bundles import sensibly.
fn default_jira_deployment() -> JiraDeployment {
    JiraDeployment::Cloud
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoExport {
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: String,
    // None / omitted inherits the global default_branch_template.
    #[serde(default)]
    pub branch_template: Option<String>,
    pub setup_script: String,
    pub instructions: String,
    pub review_policy: Option<ReviewPolicy>,
    pub enabled: bool,
    pub sync_issues: bool,
    pub issue_labels: Vec<String>,
    // Re-run the setup script before every task (issue #275); omitted = off.
    #[serde(default)]
    pub setup_script_always_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigBundle {
    pub settings: SettingsExport,
    pub repositories: Vec<RepoExport>,
}

/// `GET /api/v1/export` - the portable config bundle.
pub async fn export(State(state): State<AppState>) -> ApiResult<Json<ConfigBundle>> {
    let settings = queries::get_settings(&state.db).await?;
    let repositories = queries::list_repositories(&state.db).await?;

    let bundle = ConfigBundle {
        settings: SettingsExport {
            org_name: settings.org_name,
            global_instructions: settings.global_instructions,
            default_review_policy: settings.default_review_policy,
            claude_model: settings.claude_model,
            base_setup_script: settings.base_setup_script,
            config_repo_url: settings.config_repo_url,
            default_branch_template: settings.default_branch_template,
            availability_enabled: settings.availability_enabled,
            availability_timezone: settings.availability_timezone,
            availability_windows: settings.availability_windows.0,
            availability_skip_dates: settings.availability_skip_dates.0,
            network_access_level: settings.network_access_level,
            network_access_domains: settings.network_access_domains.0,
            network_access_include_defaults: settings.network_access_include_defaults,
            usage_limit_pause_enabled: settings.usage_limit_pause_enabled,
            usage_limit_threshold: settings.usage_limit_threshold,
            railway_idle_timeout_minutes: settings.railway_idle_timeout_minutes,
            post_thoughts_enabled: settings.post_thoughts_enabled,
            close_issue_on_done: settings.close_issue_on_done,
            attention_sound_enabled: settings.attention_sound_enabled,
            completion_sound_enabled: settings.completion_sound_enabled,
            jira_enabled: settings.jira_enabled,
            jira_deployment: settings.jira_deployment,
            jira_base_url: settings.jira_base_url,
            jira_email: settings.jira_email,
            jira_assigned_to_me_only: settings.jira_assigned_to_me_only,
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
                sync_issues: repo.sync_issues,
                issue_labels: repo.issue_labels,
                setup_script_always_run: repo.setup_script_always_run,
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
        Some(settings.availability_enabled),
        Some(settings.availability_timezone),
        Some(SqlxJson(settings.availability_windows)),
        Some(SqlxJson(settings.availability_skip_dates)),
        Some(settings.network_access_level),
        Some(SqlxJson(settings.network_access_domains)),
        Some(settings.network_access_include_defaults),
        Some(settings.usage_limit_pause_enabled),
        Some(settings.usage_limit_threshold),
        Some(settings.railway_idle_timeout_minutes),
        Some(settings.post_thoughts_enabled),
        Some(settings.jira_enabled),
        Some(settings.jira_deployment),
        Some(settings.jira_base_url),
        Some(settings.jira_email),
        Some(settings.jira_assigned_to_me_only),
        Some(settings.close_issue_on_done),
        Some(settings.attention_sound_enabled),
        Some(settings.completion_sound_enabled),
    )
    .await?;

    for repo in &bundle.repositories {
        queries::upsert_repository(
            &state.db,
            &repo.full_name,
            &repo.clone_url,
            &repo.default_branch,
            repo.branch_template.as_deref(),
            &repo.setup_script,
            &repo.instructions,
            repo.review_policy,
            repo.enabled,
            repo.sync_issues,
            &repo.issue_labels,
            repo.setup_script_always_run,
        )
        .await?;
    }

    state.notify_board();
    Ok(Json(serde_json::json!({
        "imported_repositories": bundle.repositories.len(),
    })))
}
