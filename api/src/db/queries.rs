//! Typed queries over the Seraphim schema.
//!
//! Functions take a `&PgPool` and return [`sqlx::Result`]; callers lift errors
//! into the application's `eyre` result with `?`.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    AnswerKind, AutomationRule, AvailabilityWindow, ClaudeUsageCredentials, DependencyCandidate,
    EnvSuggestion, EnvVar, EnvVarWrite, HeartAttack, InternalComment, JiraBoard, JiraDeployment,
    NetworkAccessLevel, PendingPlacement, PendingQuestion, Question, QuestionOption,
    QuestionStatus, Railway, RepoDeletionImpact, RepoSyncError, Repository, ReviewPolicy, Settings,
    SourceKind, StatsAggregate, Task, TaskColumn, TaskPullRequest, TaskScreenshot, TaskStatus,
    Turn,
};
use crate::automation::{RuleAction, RuleGroup, Trigger};

// --- Settings ----------------------------------------------------------------

/// The settings fields exposed to the app, for SELECT/RETURNING reuse. The raw
/// token columns are deliberately excluded; only "is it set" booleans are
/// surfaced so secrets never leave the database via the API.
const SETTINGS_COLUMNS: &str =
    "org_name, global_instructions, default_review_policy, agent_paused, \
     claude_model, workspace_image_tag, base_setup_script, config_repo_url, \
     default_branch_template, config_repo_error, current_session_id, updated_at, \
     (claude_oauth_token <> '') AS claude_token_set, \
     claude_auth_mode, claude_account_email, \
     (claude_usage_refresh_token <> '') AS claude_usage_token_set, \
     (github_token <> '') AS github_token_set, \
     availability_enabled, availability_timezone, availability_windows, \
     availability_skip_dates, network_access_level, network_access_domains, \
     network_access_include_defaults, usage_limit_pause_enabled, \
     usage_limit_threshold, usage_paused_until, railway_idle_timeout_minutes, \
     post_thoughts_enabled, close_issue_on_done, \
     jira_enabled, jira_deployment, jira_base_url, jira_email, \
     jira_assigned_to_me_only, jira_account_id, \
     (jira_api_token <> '') AS jira_token_set, \
     (github_webhook_secret <> '') AS github_webhook_secret_set, \
     (jira_webhook_secret <> '') AS jira_webhook_secret_set, \
     attention_sound_enabled, completion_sound_enabled, \
     (length(attention_sound_audio) > 0) AS attention_sound_custom, \
     (length(completion_sound_audio) > 0) AS completion_sound_custom";

pub async fn get_settings(pool: &PgPool) -> sqlx::Result<Settings> {
    sqlx::query_as::<_, Settings>(&format!(
        "SELECT {SETTINGS_COLUMNS} FROM settings WHERE id = 1"
    ))
    .fetch_one(pool)
    .await
}

/// Patches the settings row; `NULL` arguments leave the existing value intact.
///
/// The four availability arguments travel together: `enabled`, the IANA
/// `timezone`, the weekly `windows`, and the `skip_dates`. As with every other
/// field, passing `None` keeps the stored value.
#[allow(clippy::too_many_arguments)]
pub async fn update_settings(
    pool: &PgPool,
    org_name: Option<String>,
    global_instructions: Option<String>,
    default_review_policy: Option<ReviewPolicy>,
    claude_model: Option<String>,
    base_setup_script: Option<String>,
    config_repo_url: Option<String>,
    default_branch_template: Option<String>,
    availability_enabled: Option<bool>,
    availability_timezone: Option<String>,
    availability_windows: Option<Json<Vec<AvailabilityWindow>>>,
    availability_skip_dates: Option<Json<Vec<NaiveDate>>>,
    network_access_level: Option<NetworkAccessLevel>,
    network_access_domains: Option<Json<Vec<String>>>,
    network_access_include_defaults: Option<bool>,
    usage_limit_pause_enabled: Option<bool>,
    usage_limit_threshold: Option<i32>,
    railway_idle_timeout_minutes: Option<i32>,
    post_thoughts_enabled: Option<bool>,
    jira_enabled: Option<bool>,
    jira_deployment: Option<JiraDeployment>,
    jira_base_url: Option<String>,
    jira_email: Option<String>,
    jira_assigned_to_me_only: Option<bool>,
    close_issue_on_done: Option<bool>,
    attention_sound_enabled: Option<bool>,
    completion_sound_enabled: Option<bool>,
) -> sqlx::Result<Settings> {
    sqlx::query_as::<_, Settings>(&format!(
        "UPDATE settings SET \
         org_name = COALESCE($1, org_name), \
         global_instructions = COALESCE($2, global_instructions), \
         default_review_policy = COALESCE($3, default_review_policy), \
         claude_model = COALESCE($4, claude_model), \
         base_setup_script = COALESCE($5, base_setup_script), \
         config_repo_url = COALESCE($6, config_repo_url), \
         default_branch_template = COALESCE($7, default_branch_template), \
         availability_enabled = COALESCE($8, availability_enabled), \
         availability_timezone = COALESCE($9, availability_timezone), \
         availability_windows = COALESCE($10, availability_windows), \
         availability_skip_dates = COALESCE($11, availability_skip_dates), \
         network_access_level = COALESCE($12, network_access_level), \
         network_access_domains = COALESCE($13, network_access_domains), \
         network_access_include_defaults = \
             COALESCE($14, network_access_include_defaults), \
         usage_limit_pause_enabled = \
             COALESCE($15, usage_limit_pause_enabled), \
         usage_limit_threshold = COALESCE($16, usage_limit_threshold), \
         railway_idle_timeout_minutes = \
             COALESCE($17, railway_idle_timeout_minutes), \
         post_thoughts_enabled = COALESCE($18, post_thoughts_enabled), \
         jira_enabled = COALESCE($19, jira_enabled), \
         jira_deployment = COALESCE($20, jira_deployment), \
         jira_base_url = COALESCE($21, jira_base_url), \
         jira_email = COALESCE($22, jira_email), \
         jira_assigned_to_me_only = COALESCE($23, jira_assigned_to_me_only), \
         close_issue_on_done = COALESCE($24, close_issue_on_done), \
         attention_sound_enabled = COALESCE($25, attention_sound_enabled), \
         completion_sound_enabled = COALESCE($26, completion_sound_enabled), \
         updated_at = now() \
         WHERE id = 1 \
         RETURNING {SETTINGS_COLUMNS}"
    ))
    .bind(org_name)
    .bind(global_instructions)
    .bind(default_review_policy)
    .bind(claude_model)
    .bind(base_setup_script)
    .bind(config_repo_url)
    .bind(default_branch_template)
    .bind(availability_enabled)
    .bind(availability_timezone)
    .bind(availability_windows)
    .bind(availability_skip_dates)
    .bind(network_access_level)
    .bind(network_access_domains)
    .bind(network_access_include_defaults)
    .bind(usage_limit_pause_enabled)
    .bind(usage_limit_threshold)
    .bind(railway_idle_timeout_minutes)
    .bind(post_thoughts_enabled)
    .bind(jira_enabled)
    .bind(jira_deployment)
    .bind(jira_base_url)
    .bind(jira_email)
    .bind(jira_assigned_to_me_only)
    .bind(close_issue_on_done)
    .bind(attention_sound_enabled)
    .bind(completion_sound_enabled)
    .fetch_one(pool)
    .await
}

/// Records the connected Jira account's identifier (Cloud `accountId` / Server
/// username), captured after a successful connection test. Used to filter the
/// realtime webhook path, which cannot run JQL like the poll sync does.
pub async fn set_jira_account_id(pool: &PgPool, account_id: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET jira_account_id = $1, updated_at = now() WHERE id = 1")
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_paused(pool: &PgPool, paused: bool) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET agent_paused = $1, updated_at = now() WHERE id = 1")
        .bind(paused)
        .execute(pool)
        .await?;
    Ok(())
}

/// The global scratchpad shown beside the board.
pub async fn get_notepad(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT notepad FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await
}

/// Saves the global scratchpad. Does not touch `updated_at`: the notepad is a
/// scratchpad saved often and orthogonal to the rest of the settings.
pub async fn set_notepad(pool: &PgPool, content: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET notepad = $1 WHERE id = 1")
        .bind(content)
        .execute(pool)
        .await?;
    Ok(())
}

/// Sets (or clears with `None`) the automatic usage-limit pause: the moment the
/// agent may resume pulling work once the subscription window resets.
pub async fn set_usage_paused_until(
    pool: &PgPool,
    until: Option<chrono::DateTime<chrono::Utc>>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET usage_paused_until = $1, updated_at = now() WHERE id = 1")
        .bind(until)
        .execute(pool)
        .await?;
    Ok(())
}

// NOTE: `settings.current_session_id` is no longer the live agent session. Each
// railway (main included) now owns its session on its own `railways.session_id`
// row (read/written via `orchestrator::railway::{read_session, write_session}`),
// so there is no longer a writer for the settings column. The column is left in
// place for a later migration to drop; `get_settings` still reads it into
// `Settings` only to keep the row's shape and the settings payload stable.

/// Records (or clears with `None`) the config-repo setup error.
pub async fn set_config_repo_error(pool: &PgPool, error: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET config_repo_error = $1, updated_at = now() WHERE id = 1")
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

/// The stored Claude OAuth token (empty string if unset). Internal use only.
pub async fn get_claude_token(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT claude_oauth_token FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await
}

/// The stored GitHub token (empty string if unset). Internal use only.
pub async fn get_github_token(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT github_token FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await
}

/// The stored Jira API token / PAT (empty string if unset). Internal use only.
pub async fn get_jira_token(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT jira_api_token FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await
}

/// The shared secret GitHub signs its issue webhooks with (empty if unset).
pub async fn get_github_webhook_secret(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT github_webhook_secret FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await
}

/// The shared secret presented by Jira's issue webhooks (empty if unset).
pub async fn get_jira_webhook_secret(pool: &PgPool) -> sqlx::Result<String> {
    sqlx::query_scalar("SELECT jira_webhook_secret FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await
}

/// Writes the app tokens and webhook secrets; `None` leaves the existing value
/// untouched (so the UI can update one without resending the others).
pub async fn set_tokens(
    pool: &PgPool,
    claude_oauth_token: Option<String>,
    github_token: Option<String>,
    jira_api_token: Option<String>,
    github_webhook_secret: Option<String>,
    jira_webhook_secret: Option<String>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE settings SET \
         claude_oauth_token = COALESCE($1, claude_oauth_token), \
         github_token = COALESCE($2, github_token), \
         jira_api_token = COALESCE($3, jira_api_token), \
         github_webhook_secret = COALESCE($4, github_webhook_secret), \
         jira_webhook_secret = COALESCE($5, jira_webhook_secret), \
         updated_at = now() WHERE id = 1",
    )
    .bind(claude_oauth_token)
    .bind(github_token)
    .bind(jira_api_token)
    .bind(github_webhook_secret)
    .bind(jira_webhook_secret)
    .execute(pool)
    .await?;
    Ok(())
}

/// The refreshing OAuth credentials from a subscription login (all-empty when none
/// is configured). Drives both the on-demand inference-token refresh and the usage
/// gauge. Internal use only.
pub async fn get_usage_credentials(pool: &PgPool) -> sqlx::Result<ClaudeUsageCredentials> {
    sqlx::query_as::<_, ClaudeUsageCredentials>(
        "SELECT claude_usage_access_token AS access_token, \
         claude_usage_refresh_token AS refresh_token, \
         claude_usage_expires_at AS expires_at, \
         claude_usage_scopes AS scopes FROM settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
}

/// Persists a completed subscription OAuth login: the long-lived inference token
/// the agent runs on, plus the refreshing usage credentials, switching auth mode
/// to `subscription`.
pub async fn set_subscription_credentials(
    pool: &PgPool,
    inference_token: &str,
    access_token: &str,
    refresh_token: &str,
    expires_at: DateTime<Utc>,
    scopes: &str,
    account_email: &str,
) -> sqlx::Result<()> {
    // An empty `account_email` keeps the stored one rather than wiping it, so a
    // response that happens to omit the account never blanks a known email.
    sqlx::query(
        "UPDATE settings SET claude_oauth_token = $1, claude_auth_mode = 'subscription', \
         claude_usage_access_token = $2, claude_usage_refresh_token = $3, \
         claude_usage_expires_at = $4, claude_usage_scopes = $5, \
         claude_account_email = COALESCE(NULLIF($6, ''), claude_account_email), \
         updated_at = now() WHERE id = 1",
    )
    .bind(inference_token)
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at)
    .bind(scopes)
    .bind(account_email)
    .execute(pool)
    .await?;
    Ok(())
}

/// Persists a refreshed subscription token. The new access token becomes both the
/// inference credential the agent runs on (`claude_oauth_token`) and the usage
/// copy, keeping them in lockstep; the rotated refresh token is stored when the
/// endpoint returned one (an empty `refresh_token` keeps the existing one).
pub async fn set_oauth_tokens(
    pool: &PgPool,
    access_token: &str,
    refresh_token: &str,
    expires_at: DateTime<Utc>,
    account_email: &str,
) -> sqlx::Result<()> {
    // Like the refresh token, an empty `account_email` keeps the existing value, so
    // a refresh response that omits the account never blanks a known email. This is
    // also how an install that connected before #269 backfills its email: the first
    // refresh that returns an account populates it without a reconnect.
    sqlx::query(
        "UPDATE settings SET claude_oauth_token = $1, claude_usage_access_token = $1, \
         claude_usage_refresh_token = COALESCE(NULLIF($2, ''), claude_usage_refresh_token), \
         claude_usage_expires_at = $3, \
         claude_account_email = COALESCE(NULLIF($4, ''), claude_account_email), \
         updated_at = now() WHERE id = 1",
    )
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at)
    .bind(account_email)
    .execute(pool)
    .await?;
    Ok(())
}

/// Switches to API-key auth: stores the key as the inference credential and clears
/// the subscription usage credentials (the gauge does not apply).
pub async fn set_api_key(pool: &PgPool, api_key: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE settings SET claude_oauth_token = $1, claude_auth_mode = 'api_key', \
         claude_usage_access_token = '', claude_usage_refresh_token = '', \
         claude_usage_expires_at = NULL, claude_usage_scopes = '', \
         claude_account_email = '', updated_at = now() \
         WHERE id = 1",
    )
    .bind(api_key)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Notification sounds ------------------------------------------------------
//
// The custom audio clips live on the settings row but are streamed by a dedicated
// endpoint, never in the settings payload (which the board fetches constantly).
// Each getter returns `(bytes, mime)`; an empty `bytes` means "no custom clip, use
// the bundled default". Setting empty bytes clears a clip back to the default.

/// The uploaded attention clip and its MIME type (empty bytes if none).
pub async fn get_attention_sound(pool: &PgPool) -> sqlx::Result<(Vec<u8>, String)> {
    sqlx::query_as::<_, (Vec<u8>, String)>(
        "SELECT attention_sound_audio, attention_sound_mime FROM settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
}

/// The uploaded completion clip and its MIME type (empty bytes if none).
pub async fn get_completion_sound(pool: &PgPool) -> sqlx::Result<(Vec<u8>, String)> {
    sqlx::query_as::<_, (Vec<u8>, String)>(
        "SELECT completion_sound_audio, completion_sound_mime FROM settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
}

/// Stores (or, with empty `audio`, clears) the custom attention clip.
pub async fn set_attention_sound(pool: &PgPool, audio: &[u8], mime: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE settings SET attention_sound_audio = $1, attention_sound_mime = $2, \
         updated_at = now() WHERE id = 1",
    )
    .bind(audio)
    .bind(mime)
    .execute(pool)
    .await?;
    Ok(())
}

/// Stores (or, with empty `audio`, clears) the custom completion clip.
pub async fn set_completion_sound(pool: &PgPool, audio: &[u8], mime: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE settings SET completion_sound_audio = $1, completion_sound_mime = $2, \
         updated_at = now() WHERE id = 1",
    )
    .bind(audio)
    .bind(mime)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Environment variables ---------------------------------------------------

/// All environment variables, ordered by key, with their raw values. Internal
/// use only (injection + secret scrubbing); the HTTP layer masks secrets.
pub async fn list_environment_variables(pool: &PgPool) -> sqlx::Result<Vec<EnvVar>> {
    sqlx::query_as::<_, EnvVar>("SELECT * FROM environment_variables ORDER BY key")
        .fetch_all(pool)
        .await
}

/// Every secret value that should be scrubbed from agent output: the secret
/// environment variables plus the stored Claude, GitHub, and Jira tokens. Empty
/// values are omitted.
pub async fn list_secret_values(pool: &PgPool) -> sqlx::Result<Vec<String>> {
    let values: Vec<String> = sqlx::query_scalar(
        "SELECT value FROM environment_variables WHERE is_secret = TRUE AND value <> '' \
         UNION ALL \
         SELECT claude_oauth_token FROM settings WHERE id = 1 AND claude_oauth_token <> '' \
         UNION ALL \
         SELECT github_token FROM settings WHERE id = 1 AND github_token <> '' \
         UNION ALL \
         SELECT jira_api_token FROM settings WHERE id = 1 AND jira_api_token <> '' \
         UNION ALL \
         SELECT claude_usage_access_token FROM settings WHERE id = 1 AND claude_usage_access_token <> '' \
         UNION ALL \
         SELECT claude_usage_refresh_token FROM settings WHERE id = 1 AND claude_usage_refresh_token <> ''",
    )
    .fetch_all(pool)
    .await?;
    Ok(values)
}

/// Replaces the whole set of environment variables in one transaction.
///
/// Keys absent from `variables` are deleted. For each entry, a `Some` value is
/// written verbatim; a `None` value keeps the currently-stored value (used for
/// secrets the UI never received and so cannot resend). Returns the resulting
/// rows, ordered by key.
pub async fn replace_environment_variables(
    pool: &PgPool,
    variables: &[EnvVarWrite],
) -> sqlx::Result<Vec<EnvVar>> {
    let mut tx = pool.begin().await?;

    // Existing values, so a `None` (unchanged secret) can be preserved.
    let existing: HashMap<String, String> =
        sqlx::query_as::<_, (String, String)>("SELECT key, value FROM environment_variables")
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();

    let mut keys: Vec<String> = Vec::with_capacity(variables.len());
    for variable in variables {
        let key = variable.key.trim();
        if key.is_empty() {
            continue; // Skip blank rows the UI may leave behind.
        }
        let value = match &variable.value {
            Some(value) => value.clone(),
            None => existing.get(key).cloned().unwrap_or_default(),
        };
        sqlx::query(
            "INSERT INTO environment_variables (key, value, is_secret) VALUES ($1, $2, $3) \
             ON CONFLICT (key) DO UPDATE SET \
             value = EXCLUDED.value, is_secret = EXCLUDED.is_secret, updated_at = now()",
        )
        .bind(key)
        .bind(&value)
        .bind(variable.is_secret)
        .execute(&mut *tx)
        .await?;
        keys.push(key.to_string());
    }

    // Drop any variable the UI removed.
    sqlx::query("DELETE FROM environment_variables WHERE key <> ALL($1)")
        .bind(&keys)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    list_environment_variables(pool).await
}

// --- Railways ----------------------------------------------------------------

/// The undeletable `main` railway, which owns everything by default.
///
/// Exactly one row has `is_main` set (a partial unique index enforces it), so the
/// fetch is unambiguous; it is always present after the `0036_railways` migration.
pub async fn get_main_railway(pool: &PgPool) -> sqlx::Result<Railway> {
    sqlx::query_as::<_, Railway>("SELECT * FROM railways WHERE is_main")
        .fetch_one(pool)
        .await
}

/// Every railway, ordered for swimlane display (`main` first, then by rank). The
/// supervisor reconciles its running agent loops against this set each tick.
pub async fn list_railways(pool: &PgPool) -> sqlx::Result<Vec<Railway>> {
    sqlx::query_as::<_, Railway>(
        "SELECT * FROM railways ORDER BY is_main DESC, position, created_at",
    )
    .fetch_all(pool)
    .await
}

/// One railway by id, or `None` if it does not exist. The agent loop re-reads its
/// railway each tick so a runtime pause / session change is picked up promptly.
pub async fn get_railway(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Railway>> {
    sqlx::query_as::<_, Railway>("SELECT * FROM railways WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Persists the railway's long-lived Claude session id (empty string clears it).
///
/// Every railway, `main` included, owns its session on its own row; this row is the
/// single source of truth the orchestrator reads and writes (issue #202).
pub async fn set_railway_session_id(
    pool: &PgPool,
    railway_id: Uuid,
    session_id: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE railways SET session_id = $2, updated_at = now() WHERE id = $1")
        .bind(railway_id)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Clears every railway's session id, so a global hard reset starts every lane's
/// conversation blank. Every railway (including `main`) owns its session here, so
/// this is the single place the reset wipes live sessions.
pub async fn clear_all_railway_sessions(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("UPDATE railways SET session_id = '', updated_at = now() WHERE session_id <> ''")
        .execute(pool)
        .await?;
    Ok(())
}

/// Records a railway's container lifecycle state (issue #203).
///
/// Driven as the per-railway container is created, started, and idle-stopped, so
/// the API and UI can show whether a lane is up. `main` is always reported
/// `running` (its compose container is always on) without going through here.
pub async fn set_railway_lifecycle_state(
    pool: &PgPool,
    railway_id: Uuid,
    state: crate::db::models::RailwayState,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE railways SET lifecycle_state = $2, updated_at = now() WHERE id = $1")
        .bind(railway_id)
        .bind(state)
        .execute(pool)
        .await?;
    Ok(())
}

/// The most recent task activity on a railway, or `None` if it has no tasks with
/// recorded activity. The idle-stop reaper compares this against its timeout to
/// decide whether a non-`main` railway's container can be stopped (issue #203).
pub async fn railway_last_activity(
    pool: &PgPool,
    railway_id: Uuid,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar("SELECT MAX(last_activity_at) FROM tasks WHERE railway_id = $1")
        .bind(railway_id)
        .fetch_one(pool)
        .await
}

/// Whether a railway has a task currently being worked (`in_progress` +
/// `working`/`preparing`). The reaper never stops a railway with a live turn, and
/// the lazy-start path never needs to act on one. Pairs with
/// [`railway_last_activity`] for the idle decision (issue #203).
pub async fn railway_has_running_turn(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tasks \
         WHERE railway_id = $1 AND board_column = 'in_progress' \
         AND status IN ('working', 'preparing'))",
    )
    .bind(railway_id)
    .fetch_one(pool)
    .await
}

/// Whether a repo currently has a task being worked (`in_progress` +
/// `working`/`preparing`). A repo move is blocked while this holds, so the agent
/// is never yanked out from under the lane it is actively coding in. Mirrors
/// [`railway_has_running_turn`] but scoped to a single repo's tasks.
pub async fn repo_has_running_turn(pool: &PgPool, repo_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tasks \
         WHERE repo_id = $1 AND board_column = 'in_progress' \
         AND status IN ('working', 'preparing'))",
    )
    .bind(repo_id)
    .fetch_one(pool)
    .await
}

/// Creates a new railway, ranked after every existing one for swimlane order.
///
/// New railways start `stopped` (their container is created lazily on first
/// work), not paused, and never `main` (the single `main` row is created by the
/// migration and is undeletable). The position is one past the current maximum so
/// a fresh lane lands at the end of the board.
pub async fn create_railway(pool: &PgPool, name: &str, description: &str) -> sqlx::Result<Railway> {
    sqlx::query_as::<_, Railway>(
        "INSERT INTO railways (name, description, position) \
         VALUES ($1, $2, COALESCE((SELECT MAX(position) + 1 FROM railways), 0)) \
         RETURNING *",
    )
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await
}

/// Renames a railway and updates its description, returning the updated row (or
/// `None` if no railway has that id). Leaves the lifecycle, pause, and session
/// untouched; this is the plain "edit name/description" path.
pub async fn update_railway(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    description: &str,
) -> sqlx::Result<Option<Railway>> {
    sqlx::query_as::<_, Railway>(
        "UPDATE railways SET name = $2, description = $3, updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .fetch_optional(pool)
    .await
}

/// Toggles a railway's per-railway pause, returning the updated row (or `None` if
/// it does not exist). This gates work alongside the global master pause on
/// `settings` (issue #202); the two are independent switches.
pub async fn set_railway_paused(
    pool: &PgPool,
    id: Uuid,
    paused: bool,
) -> sqlx::Result<Option<Railway>> {
    sqlx::query_as::<_, Railway>(
        "UPDATE railways SET paused = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(paused)
    .fetch_optional(pool)
    .await
}

/// Hands a deleted railway's repos and tasks back to `main`, the prerequisite for
/// removing it (the `railway_id` foreign keys are `ON DELETE RESTRICT`).
///
/// All of `from`'s repos move to `main`, and so do its tasks; a task's railway
/// always follows its repo, so reassigning both keeps that invariant. Returns how
/// many repos and tasks were moved, for the caller's summary. `main` itself is
/// never a delete target, so this never reassigns onto the row it reads.
pub async fn reassign_railway_to_main(pool: &PgPool, from: Uuid) -> sqlx::Result<(u64, u64)> {
    let mut tx = pool.begin().await?;

    let repos = sqlx::query(
        "UPDATE repositories SET railway_id = (SELECT id FROM railways WHERE is_main), \
           updated_at = now() \
         WHERE railway_id = $1",
    )
    .bind(from)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let tasks = sqlx::query(
        "UPDATE tasks SET railway_id = (SELECT id FROM railways WHERE is_main), \
           updated_at = now() \
         WHERE railway_id = $1",
    )
    .bind(from)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;
    Ok((repos, tasks))
}

/// Deletes a railway by id. The caller must first reassign its repos and tasks to
/// `main` (see [`reassign_railway_to_main`]) and must never pass `main`; the
/// `is_main` guard here is a final backstop so a `main` delete can never slip
/// through. Returns whether a row was removed.
pub async fn delete_railway(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    let removed = sqlx::query("DELETE FROM railways WHERE id = $1 AND NOT is_main")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(removed > 0)
}

/// Moves a repo, and every task that belongs to it, onto `target` railway.
///
/// The railway follows the repo: a repo belongs to exactly one railway, and its
/// tasks always share that railway, so both are reassigned together in one
/// transaction. Returns the moved repo (or `None` if the id does not exist).
pub async fn move_repo_to_railway(
    pool: &PgPool,
    repo_id: Uuid,
    target: Uuid,
) -> sqlx::Result<Option<Repository>> {
    let mut tx = pool.begin().await?;

    let repo = sqlx::query_as::<_, Repository>(
        "UPDATE repositories SET railway_id = $2, updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(repo_id)
    .bind(target)
    .fetch_optional(&mut *tx)
    .await?;

    // Only touch the tasks if the repo actually existed, so a bad id is a clean
    // no-op rather than an orphaned task update.
    if repo.is_some() {
        sqlx::query("UPDATE tasks SET railway_id = $2, updated_at = now() WHERE repo_id = $1")
            .bind(repo_id)
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(repo)
}

// --- Repositories ------------------------------------------------------------

pub async fn list_repositories(pool: &PgPool) -> sqlx::Result<Vec<Repository>> {
    sqlx::query_as::<_, Repository>("SELECT * FROM repositories ORDER BY full_name")
        .fetch_all(pool)
        .await
}

/// Repos the sync loop should poll for issues.
pub async fn list_repositories_to_sync(pool: &PgPool) -> sqlx::Result<Vec<Repository>> {
    sqlx::query_as::<_, Repository>(
        "SELECT * FROM repositories WHERE sync_issues = TRUE AND enabled = TRUE ORDER BY full_name",
    )
    .fetch_all(pool)
    .await
}

/// Records a repo's issue-sync failure (issue #213), stamping the time so the UI
/// can show when it began failing.
pub async fn set_repo_sync_error(pool: &PgPool, id: Uuid, message: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE repositories SET sync_error = $2, sync_error_at = now() WHERE id = $1")
        .bind(id)
        .bind(message)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Clears a repo's recorded issue-sync failure after a successful sync (issue #213).
pub async fn clear_repo_sync_error(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE repositories SET sync_error = NULL, sync_error_at = NULL WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Repos whose last issue sync failed, for the board's persistent banner (issue #213).
pub async fn list_repo_sync_errors(pool: &PgPool) -> sqlx::Result<Vec<RepoSyncError>> {
    sqlx::query_as::<_, RepoSyncError>(
        "SELECT full_name, sync_error, sync_error_at FROM repositories \
         WHERE sync_error IS NOT NULL ORDER BY full_name",
    )
    .fetch_all(pool)
    .await
}

/// Enabled repos, for seeding the activity forest with their tracked files (#216).
/// Every enabled repo is cloned flat under `/workspace`, so this is the set whose
/// `git ls-files` makes up the seeded tree.
pub async fn list_enabled_repositories(pool: &PgPool) -> sqlx::Result<Vec<Repository>> {
    sqlx::query_as::<_, Repository>(
        "SELECT * FROM repositories WHERE enabled = TRUE ORDER BY full_name",
    )
    .fetch_all(pool)
    .await
}

/// Repos assigned to one railway, for provisioning that railway's container
/// (issue #203). A repo belongs to exactly one railway, so this is the exact set
/// to clone into its container. With only `main`, this is every repo.
pub async fn list_repositories_for_railway(
    pool: &PgPool,
    railway_id: Uuid,
) -> sqlx::Result<Vec<Repository>> {
    sqlx::query_as::<_, Repository>(
        "SELECT * FROM repositories WHERE railway_id = $1 ORDER BY full_name",
    )
    .bind(railway_id)
    .fetch_all(pool)
    .await
}

pub async fn get_repository(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Repository>> {
    sqlx::query_as::<_, Repository>("SELECT * FROM repositories WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn get_repository_by_full_name(
    pool: &PgPool,
    full_name: &str,
) -> sqlx::Result<Option<Repository>> {
    sqlx::query_as::<_, Repository>("SELECT * FROM repositories WHERE full_name = $1")
        .bind(full_name)
        .fetch_optional(pool)
        .await
}

/// Inserts a repository or updates it in place, keyed by `full_name`.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_repository(
    pool: &PgPool,
    full_name: &str,
    clone_url: &str,
    default_branch: &str,
    branch_template: Option<&str>,
    setup_script: &str,
    instructions: &str,
    review_policy: Option<ReviewPolicy>,
    enabled: bool,
    sync_issues: bool,
    issue_labels: &[String],
) -> sqlx::Result<Repository> {
    sqlx::query_as::<_, Repository>(
        // New repos default to the `main` railway (issue #201). Set via subquery
        // so the existing bound-parameter numbering is untouched.
        "INSERT INTO repositories \
         (railway_id, full_name, clone_url, default_branch, branch_template, setup_script, \
          instructions, review_policy, enabled, sync_issues, issue_labels) \
         VALUES ((SELECT id FROM railways WHERE is_main), \
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (full_name) DO UPDATE SET \
         clone_url = EXCLUDED.clone_url, \
         default_branch = EXCLUDED.default_branch, \
         branch_template = EXCLUDED.branch_template, \
         setup_script = EXCLUDED.setup_script, \
         instructions = EXCLUDED.instructions, \
         review_policy = EXCLUDED.review_policy, \
         enabled = EXCLUDED.enabled, \
         sync_issues = EXCLUDED.sync_issues, \
         issue_labels = EXCLUDED.issue_labels, \
         updated_at = now() \
         RETURNING *",
    )
    .bind(full_name)
    .bind(clone_url)
    .bind(default_branch)
    .bind(branch_template)
    .bind(setup_script)
    .bind(instructions)
    .bind(review_policy)
    .bind(enabled)
    .bind(sync_issues)
    .bind(issue_labels)
    .fetch_one(pool)
    .await
}

/// Updates a repository in place by `id`, allowing `full_name` to change.
///
/// This is the edit path. [`upsert_repository`] keys on `full_name`, so renaming
/// a repo through it would insert a brand-new row (and leave the old one behind);
/// editing by `id` renames the existing row instead.
#[allow(clippy::too_many_arguments)]
pub async fn update_repository(
    pool: &PgPool,
    id: Uuid,
    full_name: &str,
    clone_url: &str,
    default_branch: &str,
    branch_template: Option<&str>,
    setup_script: &str,
    instructions: &str,
    review_policy: Option<ReviewPolicy>,
    enabled: bool,
    sync_issues: bool,
    issue_labels: &[String],
) -> sqlx::Result<Repository> {
    sqlx::query_as::<_, Repository>(
        "UPDATE repositories SET \
         full_name = $2, clone_url = $3, default_branch = $4, branch_template = $5, \
         setup_script = $6, instructions = $7, review_policy = $8, enabled = $9, \
         sync_issues = $10, issue_labels = $11, updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(full_name)
    .bind(clone_url)
    .bind(default_branch)
    .bind(branch_template)
    .bind(setup_script)
    .bind(instructions)
    .bind(review_policy)
    .bind(enabled)
    .bind(sync_issues)
    .bind(issue_labels)
    .fetch_one(pool)
    .await
}

/// Like [`upsert_repository`] but only creates a row when one doesn't already
/// exist (used by org import so it never clobbers your manual edits). Returns the
/// existing or newly-created repo.
#[allow(clippy::too_many_arguments)]
pub async fn create_repository_if_absent(
    pool: &PgPool,
    full_name: &str,
    clone_url: &str,
    default_branch: &str,
    branch_template: Option<&str>,
    sync_issues: bool,
    issue_labels: &[String],
) -> sqlx::Result<Repository> {
    if let Some(existing) = get_repository_by_full_name(pool, full_name).await? {
        return Ok(existing);
    }
    upsert_repository(
        pool,
        full_name,
        clone_url,
        default_branch,
        branch_template,
        "",
        "",
        None,
        true,
        sync_issues,
        issue_labels,
    )
    .await
}

/// Counts everything a delete of this repo would purge: its tasks and the
/// turns, events, questions, and suggestions that cascade from them.
pub async fn repo_deletion_impact(pool: &PgPool, id: Uuid) -> sqlx::Result<RepoDeletionImpact> {
    sqlx::query_as::<_, RepoDeletionImpact>(
        "SELECT \
           (SELECT COUNT(*) FROM tasks WHERE repo_id = $1) AS tasks, \
           (SELECT COUNT(*) FROM turns t \
              JOIN tasks k ON t.task_id = k.id WHERE k.repo_id = $1) AS turns, \
           (SELECT COUNT(*) FROM events e \
              JOIN turns t ON e.turn_id = t.id \
              JOIN tasks k ON t.task_id = k.id WHERE k.repo_id = $1) AS events, \
           (SELECT COUNT(*) FROM questions q \
              JOIN tasks k ON q.task_id = k.id WHERE k.repo_id = $1) AS questions, \
           (SELECT COUNT(*) FROM environment_suggestions s \
              JOIN tasks k ON s.task_id = k.id WHERE k.repo_id = $1) AS suggestions",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Deletes a repository and everything synced from it. The `tasks.repo_id` FK
/// cascades to the repo's tasks, and tasks cascade to their turns/events/
/// questions/suggestions, so the whole subtree goes in one statement. The Jira
/// board association is a JSON array (no FK), so we strip the repo id from it in
/// the same transaction to avoid a dangling reference on the next sync.
pub async fn delete_repository(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE jira_boards SET repo_ids = repo_ids - $1, updated_at = now() \
         WHERE jsonb_exists(repo_ids, $1)",
    )
    .bind(id.to_string())
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM repositories WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

// --- Tasks -------------------------------------------------------------------

pub async fn list_tasks(pool: &PgPool) -> sqlx::Result<Vec<Task>> {
    sqlx::query_as::<_, Task>("SELECT * FROM tasks ORDER BY board_column, position")
        .fetch_all(pool)
        .await
}

/// Whether the agent is mid-turn (a card sits in the In Progress lane). Used to
/// block a self-update while work is actively running.
pub async fn any_task_in_progress(pool: &PgPool) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE board_column = 'in_progress')",
    )
    .fetch_one(pool)
    .await
}

pub async fn get_task(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Inserts a freshly-synced issue into `Available`, or refreshes the cached
/// title/body/url of one we already track. Never touches the human-curated
/// `board_column`, `position`, or `status`.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_issue_task(
    pool: &PgPool,
    source_kind: SourceKind,
    external_id: &str,
    repo_id: Option<Uuid>,
    title: &str,
    body: &str,
    url: &str,
    external_state: &str,
    author_login: &str,
    author_avatar_url: &str,
    initial_position: f64,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        // A task's railway follows its repo, falling back to `main` (issue #201).
        "INSERT INTO tasks (railway_id, source_kind, external_id, repo_id, title, body_snapshot, url, external_state, author_login, author_avatar_url, board_column, position) \
         VALUES (COALESCE((SELECT railway_id FROM repositories WHERE id = $3), (SELECT id FROM railways WHERE is_main)), \
          $1, $2, $3, $4, $5, $6, $7, $8, $9, 'available', $10) \
         ON CONFLICT (repo_id, source_kind, external_id) DO UPDATE SET \
         title = EXCLUDED.title, \
         body_snapshot = EXCLUDED.body_snapshot, \
         url = EXCLUDED.url, \
         external_state = EXCLUDED.external_state, \
         author_login = EXCLUDED.author_login, \
         author_avatar_url = EXCLUDED.author_avatar_url, \
         updated_at = now() \
         RETURNING *",
    )
    .bind(source_kind)
    .bind(external_id)
    .bind(repo_id)
    .bind(title)
    .bind(body)
    .bind(url)
    .bind(external_state)
    .bind(author_login)
    .bind(author_avatar_url)
    .bind(initial_position)
    .fetch_one(pool)
    .await
}

/// Upserts a Jira ticket as a task, deduped on the issue key. New tickets land in
/// the column their mapped Jira status implies; on conflict we refresh the cached
/// fields and the live Jira status, but never the human-curated `board_column` or
/// `position`.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_jira_task(
    pool: &PgPool,
    external_id: &str,
    repo_id: Option<Uuid>,
    jira_board_id: Uuid,
    title: &str,
    body: &str,
    url: &str,
    status: &str,
    initial_column: TaskColumn,
    initial_position: f64,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        // A task's railway follows its repo, falling back to `main` (issue #201).
        "INSERT INTO tasks (railway_id, source_kind, external_id, repo_id, jira_board_id, title, body_snapshot, url, external_state, board_column, position) \
         VALUES (COALESCE((SELECT railway_id FROM repositories WHERE id = $2), (SELECT id FROM railways WHERE is_main)), \
          'jira', $1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (external_id) WHERE source_kind = 'jira' DO UPDATE SET \
         repo_id = EXCLUDED.repo_id, \
         jira_board_id = EXCLUDED.jira_board_id, \
         title = EXCLUDED.title, \
         body_snapshot = EXCLUDED.body_snapshot, \
         url = EXCLUDED.url, \
         external_state = EXCLUDED.external_state, \
         updated_at = now() \
         RETURNING *",
    )
    .bind(external_id)
    .bind(repo_id)
    .bind(jira_board_id)
    .bind(title)
    .bind(body)
    .bind(url)
    .bind(status)
    .bind(initial_column)
    .bind(initial_position)
    .fetch_one(pool)
    .await
}

/// Upserts a Jira ticket as a task, landing a brand-new one in `initial_column` at
/// `initial_position` on the planner's chosen `railway_id` lane (issue #207).
///
/// Used only when the route planner pre-recorded a placement for this ticket. The
/// railway-follows-repo invariant still wins: a repo-bound ticket lands on its
/// repo's railway, and only a repo-less ticket falls through to the placement's
/// `railway_id` (then `main`), mirroring `create_internal_task_in_todo`. An
/// existing card takes the same conflict path as [`upsert_jira_task`], so its
/// human-curated column / position / railway are never disturbed.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_jira_task_placed(
    pool: &PgPool,
    external_id: &str,
    repo_id: Option<Uuid>,
    jira_board_id: Uuid,
    title: &str,
    body: &str,
    url: &str,
    status: &str,
    initial_column: TaskColumn,
    initial_position: f64,
    railway_id: Option<Uuid>,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        // railway = repo's railway, else the planner's chosen lane, else `main`.
        // Only the railway / column / position differ from `upsert_jira_task`; the
        // conflict path is identical, so an existing card is never disturbed.
        "INSERT INTO tasks (railway_id, source_kind, external_id, repo_id, jira_board_id, title, body_snapshot, url, external_state, board_column, position) \
         VALUES (COALESCE( \
             (SELECT railway_id FROM repositories WHERE id = $2), \
             $10, \
             (SELECT id FROM railways WHERE is_main)), \
          'jira', $1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (external_id) WHERE source_kind = 'jira' DO UPDATE SET \
         repo_id = EXCLUDED.repo_id, \
         jira_board_id = EXCLUDED.jira_board_id, \
         title = EXCLUDED.title, \
         body_snapshot = EXCLUDED.body_snapshot, \
         url = EXCLUDED.url, \
         external_state = EXCLUDED.external_state, \
         updated_at = now() \
         RETURNING *",
    )
    .bind(external_id)
    .bind(repo_id)
    .bind(jira_board_id)
    .bind(title)
    .bind(body)
    .bind(url)
    .bind(status)
    .bind(initial_column)
    .bind(initial_position)
    .bind(railway_id)
    .fetch_one(pool)
    .await
}

/// Upserts a GitHub issue as a task, landing a brand-new one in `initial_column`
/// at `initial_position` instead of the default top of Available.
///
/// Used only when the route planner pre-recorded a placement for this issue (issue
/// #207): a fresh card lands in the planner's lane and order, while an existing
/// task takes the identical conflict path as [`upsert_issue_task`] (refresh cached
/// fields, never the human-curated column / position / railway). A GitHub issue
/// always has a repo, so its railway still follows that repo; the placement only
/// supplies the column and position.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_issue_task_placed(
    pool: &PgPool,
    source_kind: SourceKind,
    external_id: &str,
    repo_id: Option<Uuid>,
    title: &str,
    body: &str,
    url: &str,
    external_state: &str,
    author_login: &str,
    author_avatar_url: &str,
    initial_column: TaskColumn,
    initial_position: f64,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        // A task's railway follows its repo, falling back to `main` (issue #201).
        // Only the landing column / position differ from `upsert_issue_task`; the
        // conflict path is identical, so an existing card is never disturbed.
        "INSERT INTO tasks (railway_id, source_kind, external_id, repo_id, title, body_snapshot, url, external_state, author_login, author_avatar_url, board_column, position) \
         VALUES (COALESCE((SELECT railway_id FROM repositories WHERE id = $3), (SELECT id FROM railways WHERE is_main)), \
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         ON CONFLICT (repo_id, source_kind, external_id) DO UPDATE SET \
         title = EXCLUDED.title, \
         body_snapshot = EXCLUDED.body_snapshot, \
         url = EXCLUDED.url, \
         external_state = EXCLUDED.external_state, \
         author_login = EXCLUDED.author_login, \
         author_avatar_url = EXCLUDED.author_avatar_url, \
         updated_at = now() \
         RETURNING *",
    )
    .bind(source_kind)
    .bind(external_id)
    .bind(repo_id)
    .bind(title)
    .bind(body)
    .bind(url)
    .bind(external_state)
    .bind(author_login)
    .bind(author_avatar_url)
    .bind(initial_column)
    .bind(initial_position)
    .fetch_one(pool)
    .await
}

// --- Jira boards -------------------------------------------------------------

/// Every followed Jira board, ordered by name.
pub async fn list_jira_boards(pool: &PgPool) -> sqlx::Result<Vec<JiraBoard>> {
    sqlx::query_as::<_, JiraBoard>("SELECT * FROM jira_boards ORDER BY name")
        .fetch_all(pool)
        .await
}

/// Followed boards that are flagged to sync.
pub async fn list_jira_boards_to_sync(pool: &PgPool) -> sqlx::Result<Vec<JiraBoard>> {
    sqlx::query_as::<_, JiraBoard>(
        "SELECT * FROM jira_boards WHERE sync_enabled = TRUE ORDER BY name",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_jira_board(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<JiraBoard>> {
    sqlx::query_as::<_, JiraBoard>("SELECT * FROM jira_boards WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Adds a discovered board if we are not already following it; never clobbers an
/// existing board's mapping or repo associations.
pub async fn create_jira_board_if_absent(
    pool: &PgPool,
    board_id: i64,
    name: &str,
    project_key: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO jira_boards (board_id, name, project_key) VALUES ($1, $2, $3) \
         ON CONFLICT (board_id) DO NOTHING",
    )
    .bind(board_id)
    .bind(name)
    .bind(project_key)
    .execute(pool)
    .await?;
    Ok(())
}

/// Updates a followed board's sync flag, status mapping, and repo associations.
pub async fn update_jira_board(
    pool: &PgPool,
    id: Uuid,
    sync_enabled: bool,
    status_map: Json<std::collections::HashMap<String, TaskColumn>>,
    repo_ids: Json<Vec<Uuid>>,
) -> sqlx::Result<JiraBoard> {
    sqlx::query_as::<_, JiraBoard>(
        "UPDATE jira_boards SET sync_enabled = $2, status_map = $3, repo_ids = $4, \
         updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(sync_enabled)
    .bind(status_map)
    .bind(repo_ids)
    .fetch_one(pool)
    .await
}

pub async fn delete_jira_board(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM jira_boards WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Automation rules --------------------------------------------------------

/// Every rule, in evaluation order (first match wins).
pub async fn list_automation_rules(pool: &PgPool) -> sqlx::Result<Vec<AutomationRule>> {
    sqlx::query_as::<_, AutomationRule>(
        "SELECT * FROM automation_rules ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
}

/// The enabled rules only, in evaluation order. Used by the event handlers.
pub async fn list_enabled_automation_rules(pool: &PgPool) -> sqlx::Result<Vec<AutomationRule>> {
    sqlx::query_as::<_, AutomationRule>(
        "SELECT * FROM automation_rules WHERE enabled = TRUE ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
}

/// The bottom rank, so a newly created rule sorts after the others.
pub async fn max_automation_rule_position(pool: &PgPool) -> sqlx::Result<Option<f64>> {
    sqlx::query_scalar::<_, Option<f64>>("SELECT MAX(position) FROM automation_rules")
        .fetch_one(pool)
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn create_automation_rule(
    pool: &PgPool,
    name: &str,
    enabled: bool,
    source_kind: &str,
    triggers: &[Trigger],
    criteria: &RuleGroup,
    action: &RuleAction,
    position: f64,
) -> sqlx::Result<AutomationRule> {
    sqlx::query_as::<_, AutomationRule>(
        "INSERT INTO automation_rules \
         (name, enabled, source_kind, triggers, criteria, action, position) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    )
    .bind(name)
    .bind(enabled)
    .bind(source_kind)
    .bind(Json(triggers))
    .bind(Json(criteria))
    .bind(Json(action))
    .bind(position)
    .fetch_one(pool)
    .await
}

/// Updates a rule's editable fields, preserving its rank. `None` if it's gone.
#[allow(clippy::too_many_arguments)]
pub async fn update_automation_rule(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    enabled: bool,
    source_kind: &str,
    triggers: &[Trigger],
    criteria: &RuleGroup,
    action: &RuleAction,
) -> sqlx::Result<Option<AutomationRule>> {
    sqlx::query_as::<_, AutomationRule>(
        "UPDATE automation_rules SET \
         name = $2, enabled = $3, source_kind = $4, triggers = $5, criteria = $6, \
         action = $7, updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(name)
    .bind(enabled)
    .bind(source_kind)
    .bind(Json(triggers))
    .bind(Json(criteria))
    .bind(Json(action))
    .fetch_optional(pool)
    .await
}

pub async fn delete_automation_rule(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM automation_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Looks up a tracked task by its source identity (the way sync dedupes it).
pub async fn find_issue_task(
    pool: &PgPool,
    source_kind: SourceKind,
    repo_id: Option<Uuid>,
    external_id: &str,
) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks \
         WHERE source_kind = $1 AND repo_id IS NOT DISTINCT FROM $2 AND external_id = $3",
    )
    .bind(source_kind)
    .bind(repo_id)
    .bind(external_id)
    .fetch_optional(pool)
    .await
}

/// Records the source ticket's state on a task (e.g. "open"/"closed" after a
/// close or reopen from the task view). Returns the refreshed task.
pub async fn set_task_external_state(
    pool: &PgPool,
    id: Uuid,
    external_state: &str,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET external_state = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(external_state)
    .fetch_one(pool)
    .await
}

/// Reconciles a task's cached `external_state` with a freshly observed value,
/// writing (and reporting `true`) only when it actually differs. Used when we
/// fetch the live issue thread, so the badge catches state changes made outside
/// Seraphim, e.g. a PR that closed the issue via a "Closes #N" keyword.
pub async fn reconcile_task_external_state(
    pool: &PgPool,
    id: Uuid,
    external_state: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE tasks SET external_state = $2, updated_at = now() \
         WHERE id = $1 AND external_state IS DISTINCT FROM $2",
    )
    .bind(id)
    .bind(external_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn max_position_in_column(
    pool: &PgPool,
    column: TaskColumn,
) -> sqlx::Result<Option<f64>> {
    sqlx::query_scalar::<_, Option<f64>>("SELECT MAX(position) FROM tasks WHERE board_column = $1")
        .bind(column)
        .fetch_one(pool)
        .await
}

/// The lowest (topmost) position currently in a column, so a brand-new card can
/// be placed above everything else with `min - 1`.
pub async fn min_position_in_column(
    pool: &PgPool,
    column: TaskColumn,
) -> sqlx::Result<Option<f64>> {
    sqlx::query_scalar::<_, Option<f64>>("SELECT MIN(position) FROM tasks WHERE board_column = $1")
        .bind(column)
        .fetch_one(pool)
        .await
}

/// Reflects an external state change onto the board: when a tracked issue's
/// source state actually changes (e.g. a human closes or reopens it on
/// GitHub/Jira), record the new state and move the card to `target_column` at
/// `target_position`, applying the same fresh-start resets `move_task` /
/// `finish_task` do for that column.
///
/// Update-only (never inserts), so a closed issue we don't track creates nothing.
/// Idempotent: the `external_state IS DISTINCT FROM` guard makes this a no-op
/// unless the state genuinely transitioned, so steady-state syncs never clobber a
/// human-curated column. A card the agent is actively working (`in_progress`) is
/// left untouched so the move doesn't fight the agent loop; the next sync (once it
/// leaves `in_progress`) reconciles it. Pass `repo_id = None` to match on
/// `external_id` alone (Jira keys are globally unique; GitHub numbers need the
/// repo). Returns whether a row changed.
pub async fn apply_external_state(
    pool: &PgPool,
    source_kind: SourceKind,
    repo_id: Option<Uuid>,
    external_id: &str,
    external_state: &str,
    target_column: TaskColumn,
    target_position: f64,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE tasks SET \
         external_state = $4, \
         board_column = $5, \
         position = CASE WHEN board_column IS DISTINCT FROM $5 THEN $6 ELSE position END, \
         status = CASE \
             WHEN board_column IS NOT DISTINCT FROM $5 THEN status \
             WHEN $5 IN ('available'::task_column, 'todo'::task_column) THEN 'queued'::task_status \
             WHEN $5 = 'done'::task_column THEN 'done'::task_status \
             ELSE status END, \
         error = CASE WHEN board_column IS DISTINCT FROM $5 \
                      AND $5 IN ('available'::task_column, 'todo'::task_column) \
                      THEN NULL ELSE error END, \
         ci_fix_attempts = CASE WHEN board_column IS DISTINCT FROM $5 \
                                AND $5 IN ('available'::task_column, 'todo'::task_column) \
                                THEN 0 ELSE ci_fix_attempts END, \
         finished_at = CASE WHEN board_column IS DISTINCT FROM $5 AND $5 = 'done'::task_column \
                            THEN now() ELSE finished_at END, \
         stats_reset_at = CASE WHEN board_column IS DISTINCT FROM $5 \
                               AND $5 IN ('available'::task_column, 'todo'::task_column) \
                               THEN now() ELSE stats_reset_at END, \
         updated_at = now() \
         WHERE source_kind = $1 AND external_id = $3 \
         AND ($2::uuid IS NULL OR repo_id IS NOT DISTINCT FROM $2::uuid) \
         AND external_state IS DISTINCT FROM $4 \
         AND board_column <> 'in_progress'::task_column",
    )
    .bind(source_kind)
    .bind(repo_id)
    .bind(external_id)
    .bind(external_state)
    .bind(target_column)
    .bind(target_position)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Refreshes the source-ticket state (e.g. `open` -> `closed`) of an already
/// tracked issue, identified the way sync dedupes it. Returns whether a row
/// changed; never inserts, so a webhook for an untracked issue is a no-op.
pub async fn refresh_issue_external_state(
    pool: &PgPool,
    source_kind: SourceKind,
    repo_id: Option<Uuid>,
    external_id: &str,
    external_state: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE tasks SET external_state = $4, updated_at = now() \
         WHERE source_kind = $1 AND repo_id IS NOT DISTINCT FROM $2 AND external_id = $3 \
         AND external_state IS DISTINCT FROM $4",
    )
    .bind(source_kind)
    .bind(repo_id)
    .bind(external_id)
    .bind(external_state)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Removes a tracked issue when its source issue is deleted upstream. Returns
/// whether a row was removed.
pub async fn delete_issue_task(
    pool: &PgPool,
    source_kind: SourceKind,
    repo_id: Option<Uuid>,
    external_id: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "DELETE FROM tasks \
         WHERE source_kind = $1 AND repo_id IS NOT DISTINCT FROM $2 AND external_id = $3",
    )
    .bind(source_kind)
    .bind(repo_id)
    .bind(external_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Finds a tracked Jira ticket by its key alone (the unique Jira dedupe key),
/// regardless of which repo it currently targets. Used to tell a brand-new ticket
/// from one already on the board before consuming a planner placement.
pub async fn find_jira_task(pool: &PgPool, external_id: &str) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE source_kind = 'jira' AND external_id = $1")
        .bind(external_id)
        .fetch_optional(pool)
        .await
}

/// Removes a tracked Jira ticket (deduped on its key alone) when it is deleted
/// upstream. Returns whether a row was removed.
pub async fn delete_jira_task(pool: &PgPool, external_id: &str) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM tasks WHERE source_kind = 'jira' AND external_id = $1")
        .bind(external_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn move_task(
    pool: &PgPool,
    id: Uuid,
    column: TaskColumn,
    position: f64,
) -> sqlx::Result<Task> {
    // Re-queuing a card (into To Do or Available) is a hard reset: it clears any
    // prior failure, resets the CI-fix counter, and stamps the stats reset marker
    // so its time/cost/tokens start fresh, all so the task starts clean.
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET board_column = $2, position = $3, \
         status = CASE WHEN $2 IN ('todo'::task_column, 'available'::task_column) \
                       THEN 'queued'::task_status ELSE status END, \
         error = CASE WHEN $2 IN ('todo'::task_column, 'available'::task_column) \
                      THEN NULL ELSE error END, \
         ci_fix_attempts = CASE WHEN $2 IN ('todo'::task_column, 'available'::task_column) \
                                THEN 0 ELSE ci_fix_attempts END, \
         stats_reset_at = CASE WHEN $2 IN ('todo'::task_column, 'available'::task_column) \
                               THEN now() ELSE stats_reset_at END, \
         updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(column)
    .bind(position)
    .fetch_one(pool)
    .await
}

/// Repositions a task within its column WITHOUT the re-queue side effects of
/// [`move_task`] (issue #274). A bulk sort only changes the manual order, so it
/// must not reset status / error / CI-fix counter / stats the way moving into
/// To Do or Available does. The column is left untouched.
pub async fn set_task_position(pool: &PgPool, id: Uuid, position: f64) -> sqlx::Result<()> {
    sqlx::query("UPDATE tasks SET position = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(position)
        .execute(pool)
        .await?;
    Ok(())
}

/// Hard-resets a single task back to a clean, unstarted state in **Available**:
/// clears its branch, PR link, error, session, and started/finished markers,
/// re-queues it (`queued`), zeroes the CI-fix counter, and restarts its stats.
/// The external cleanup (closing the PR, deleting the branch, reopening the
/// issue) is done by the caller in the orchestrator before this is called.
pub async fn reset_task(pool: &PgPool, id: Uuid, position: f64) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET \
           board_column = 'available', position = $2, status = 'queued', \
           branch = NULL, pr_url = NULL, error = NULL, session_id = NULL, \
           ci_fix_attempts = 0, started_at = NULL, finished_at = NULL, \
           stats_reset_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(position)
    .fetch_one(pool)
    .await
}

/// Drops any still-pending questions a task had escalated, so a reset card stops
/// showing up as needing input. Answered questions are kept as history.
pub async fn delete_pending_questions(pool: &PgPool, task_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM questions WHERE task_id = $1 AND status = 'pending'")
        .bind(task_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn set_task_hold(pool: &PgPool, id: Uuid, hold: bool) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET hold = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(hold)
    .fetch_one(pool)
    .await
}

/// Flags a task as blocking (or clears it). While a blocking task is in progress,
/// the agent pulls no new work.
pub async fn set_task_blocking(pool: &PgPool, id: Uuid, blocking: bool) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET blocking = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(blocking)
    .fetch_one(pool)
    .await
}

// --- Bulk operations ---------------------------------------------------------
//
// Power the board's multi-select bulk edit. Each takes a set of task ids and
// applies one change in a single statement, so a selection of many cards is one
// round-trip rather than N.

/// Loads the tasks for a set of ids (board order), for bulk operations that need
/// each card's source/repo to also reflect the change onto its ticket.
pub async fn list_tasks_by_ids(pool: &PgPool, ids: &[Uuid]) -> sqlx::Result<Vec<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE id = ANY($1) ORDER BY board_column, position",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
}

/// Sets `hold` and/or `blocking` on a set of tasks. A `None` field is left as is
/// (`COALESCE` keeps the existing value), so the caller changes only the fields
/// the user actually picked. Returns how many rows were updated.
pub async fn bulk_set_fields(
    pool: &PgPool,
    ids: &[Uuid],
    hold: Option<bool>,
    blocking: Option<bool>,
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE tasks SET hold = COALESCE($2, hold), blocking = COALESCE($3, blocking), \
         updated_at = now() WHERE id = ANY($1)",
    )
    .bind(ids)
    .bind(hold)
    .bind(blocking)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Permanently deletes a set of tasks. Child rows (turns, events, suggestions,
/// questions, internal comments) cascade; heart attacks keep their snapshot with
/// a nulled task id. Returns how many tasks were removed.
pub async fn delete_tasks(pool: &PgPool, ids: &[Uuid]) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM tasks WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Whether any blocking task is currently in progress. A fresh task that
/// finishes (success or failure) leaves `in_progress`, so a blocking task still
/// sitting here is unfinished, being worked or parked waiting for input, and the
/// agent must not start anything new.
pub async fn has_active_blocking_task(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<bool> {
    // Scoped to the railway (issue #202): each railway serializes only its own
    // queue, so a blocking task on another lane never gates this one. With only the
    // `main` railway every task is on `main`, so this matches the global behavior.
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tasks \
         WHERE blocking = TRUE AND board_column = 'in_progress' AND railway_id = $1)",
    )
    .bind(railway_id)
    .fetch_one(pool)
    .await
}

/// Saves the operator's private scratchpad for a task. Does not touch
/// `updated_at`: notes are orthogonal to the task's lifecycle and saved often.
pub async fn set_task_notes(pool: &PgPool, id: Uuid, notes: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE tasks SET notes = $2 WHERE id = $1")
        .bind(id)
        .bind(notes)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Internal tickets --------------------------------------------------------

/// Creates an internal ticket (no external tracker). It lands in `Available` at
/// the given position with a sequential, human-friendly external id. `repo_ids`
/// are the repos the ticket targets, in priority order; the first becomes the
/// primary `repo_id` the agent branches in. Pass an empty slice to triage the
/// repos later (a tracking-only ticket that is not auto-pulled).
pub async fn create_internal_task(
    pool: &PgPool,
    title: &str,
    body: &str,
    state: &str,
    repo_ids: &[Uuid],
    initial_position: f64,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        // A task's railway follows its primary repo, falling back to `main` (#201).
        "INSERT INTO tasks \
           (railway_id, source_kind, external_id, repo_id, target_repo_ids, title, body_snapshot, url, external_state, board_column, position) \
         VALUES (COALESCE((SELECT railway_id FROM repositories WHERE id = $1), (SELECT id FROM railways WHERE is_main)), \
           'internal', nextval('internal_ticket_seq')::text, $1, $2, $3, $4, '', $5, 'available', $6) \
         RETURNING *",
    )
    .bind(repo_ids.first().copied())
    .bind(Json(repo_ids.to_vec()))
    .bind(title)
    .bind(body)
    .bind(state)
    .bind(initial_position)
    .fetch_one(pool)
    .await
}

/// Creates an internal ticket directly in a railway's **To Do**, at `position`
/// (issue #207's bulk-create from the planner).
///
/// The railway always follows the repo: when `repo_id` is set the card lands on
/// that repo's railway, ignoring `railway_id`; only a repo-less draft uses the
/// explicit `railway_id`, falling back to `main` when that is `None` too. This
/// keeps the "railway follows repo" invariant intact while letting the planner
/// place repo-less drafts on a chosen lane. `position` carries the planner's
/// dependency order so the To Do lane preserves it.
pub async fn create_internal_task_in_todo(
    pool: &PgPool,
    title: &str,
    body: &str,
    repo_id: Option<Uuid>,
    railway_id: Option<Uuid>,
    position: f64,
) -> sqlx::Result<Task> {
    let repo_ids: Vec<Uuid> = repo_id.into_iter().collect();
    sqlx::query_as::<_, Task>(
        // railway = repo's railway, else the chosen railway, else `main`.
        "INSERT INTO tasks \
           (railway_id, source_kind, external_id, repo_id, target_repo_ids, title, body_snapshot, url, external_state, board_column, position) \
         VALUES (COALESCE( \
             (SELECT railway_id FROM repositories WHERE id = $1), \
             $2, \
             (SELECT id FROM railways WHERE is_main)), \
           'internal', nextval('internal_ticket_seq')::text, $1, $3, $4, $5, '', 'open', 'todo', $6) \
         RETURNING *",
    )
    .bind(repo_id)
    .bind(railway_id)
    .bind(Json(repo_ids))
    .bind(title)
    .bind(body)
    .bind(position)
    .fetch_one(pool)
    .await
}

/// Sets the repos an internal ticket targets (priority order; the first becomes
/// the primary `repo_id`), or clears them with an empty slice. Restricted to
/// internal tasks: a GitHub task's repo is its issue's and must not be
/// reassigned. Returns the updated task, or `None` if the id is not internal.
pub async fn set_internal_task_repos(
    pool: &PgPool,
    id: Uuid,
    repo_ids: &[Uuid],
) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET repo_id = $2, target_repo_ids = $3, updated_at = now() \
         WHERE id = $1 AND source_kind = 'internal' RETURNING *",
    )
    .bind(id)
    .bind(repo_ids.first().copied())
    .bind(Json(repo_ids.to_vec()))
    .fetch_optional(pool)
    .await
}

/// An internal ticket's comments, oldest first.
pub async fn list_internal_comments(
    pool: &PgPool,
    task_id: Uuid,
) -> sqlx::Result<Vec<InternalComment>> {
    sqlx::query_as::<_, InternalComment>(
        "SELECT * FROM internal_comments WHERE task_id = $1 ORDER BY created_at",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
}

/// Posts a comment to an internal ticket. `author` is "user" or "agent".
pub async fn add_internal_comment(
    pool: &PgPool,
    task_id: Uuid,
    author: &str,
    body: &str,
) -> sqlx::Result<InternalComment> {
    sqlx::query_as::<_, InternalComment>(
        "INSERT INTO internal_comments (task_id, author, body) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(task_id)
    .bind(author)
    .bind(body)
    .fetch_one(pool)
    .await
}

pub async fn set_task_status(pool: &PgPool, id: Uuid, status: TaskStatus) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET status = $2, last_activity_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(status)
    .fetch_one(pool)
    .await
}

/// Returns tasks stranded in `In Progress` by an interrupted turn (e.g. the API
/// restarting mid-turn) back to `To Do`, reset to a clean queued state, so the
/// agent reworks them instead of leaving them stuck. Returns how many.
pub async fn reclaim_orphaned_tasks(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE tasks SET board_column = 'todo', status = 'queued', error = NULL, \
         ci_fix_attempts = 0, review_fix_attempts = 0, updated_at = now() \
         WHERE board_column = 'in_progress'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Marks any turn left `running` by a previous process (killed mid-turn, e.g. an
/// API restart) as `failed`. On a fresh boot no turn is actually generating, so
/// every `running` row is orphaned; left alone they linger forever and inflate the
/// running-turn count. `finished_at` is set to `started_at` (zero duration), NOT
/// `now()`: an ancient orphan finished at `now()` would add its whole bogus span to
/// `worked_ms` (which sums `finished_at - started_at`), so zero is the safe choice.
/// Returns how many were cleaned.
pub async fn reclaim_orphaned_turns(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE turns SET status = 'failed', finished_at = started_at WHERE status = 'running'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// The next card the agent should work: top of `To Do`, not on hold.
///
/// Restricted to GitHub tasks for now: the agent codes a ticket by branching and
/// opening a PR in one repo, while a Jira ticket may span several (its board's
/// repo set), which is a separate execution model not yet built. Jira tickets
/// still sync in, map to columns, and transition on moves; they just are not
/// auto-pulled to be coded.
pub async fn pick_next_todo(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<Option<Task>> {
    // GitHub issues are always workable; internal tickets are too, but only once
    // the operator has pointed them at a target repo (the agent needs somewhere to
    // branch and open the PR). Jira tickets stay excluded (multi-repo execution is
    // not built yet). An internal ticket with no repo is left for the operator to
    // assign rather than pulled and immediately failed.
    //
    // Scoped to the railway (issue #202) so each lane pulls only its own work; with
    // only `main` every task is on `main`, so the result is unchanged from before.
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'todo' AND hold = FALSE \
         AND railway_id = $1 \
         AND (source_kind = 'github' \
              OR (source_kind = 'internal' AND repo_id IS NOT NULL)) \
         ORDER BY position ASC LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// Tasks sitting in review whose CI the review loop should (re-)evaluate.
///
/// Includes `merging`, so a merge interrupted by a restart or a transient error
/// is reconsidered rather than left stuck in that state forever.
pub async fn list_review_candidates(pool: &PgPool) -> sqlx::Result<Vec<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' \
         AND status IN ('awaiting_review', 'merging') ORDER BY position",
    )
    .fetch_all(pool)
    .await
}

/// The next PR whose failing CI the agent should fix: top of `In Review` flagged
/// `ci_failing`, not on hold.
pub async fn pick_next_ci_fix(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'ci_failing' \
         AND hold = FALSE AND railway_id = $1 ORDER BY position ASC LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// The next PR whose auto-merge failed on a conflict and that the agent should
/// resolve: top of `In Review` flagged `merge_conflict`, not on hold.
///
/// Unlike [`pick_next_revisit`] this has no cooldown: a fresh conflict is handed
/// back to the agent promptly (and ahead of new To Do work) so a PR that just
/// fell out of mergeability is unblocked rather than left to the idle revisit.
pub async fn pick_next_merge_conflict(
    pool: &PgPool,
    railway_id: Uuid,
) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'merge_conflict' \
         AND hold = FALSE AND railway_id = $1 ORDER BY position ASC LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// The next PR whose unresolved review comments the agent should address: top of
/// `In Review` flagged `addressing_review`, not on hold.
///
/// Like [`pick_next_ci_fix`] this re-engages an existing PR, so it takes priority
/// over fresh To Do work; placed after CI fixes and conflict resolution since a
/// red or unmergeable PR is the more urgent block.
pub async fn pick_next_review_address(
    pool: &PgPool,
    railway_id: Uuid,
) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'addressing_review' \
         AND hold = FALSE AND railway_id = $1 ORDER BY position ASC LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// Increments a task's CI-fix attempt counter, returning the new count.
pub async fn bump_ci_fix_attempt(pool: &PgPool, id: Uuid) -> sqlx::Result<i32> {
    sqlx::query_scalar(
        "UPDATE tasks SET ci_fix_attempts = ci_fix_attempts + 1, last_activity_at = now(), \
         updated_at = now() WHERE id = $1 RETURNING ci_fix_attempts",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Increments a task's review-address attempt counter, returning the new count.
pub async fn bump_review_fix_attempt(pool: &PgPool, id: Uuid) -> sqlx::Result<i32> {
    sqlx::query_scalar(
        "UPDATE tasks SET review_fix_attempts = review_fix_attempts + 1, last_activity_at = now(), \
         updated_at = now() WHERE id = $1 RETURNING review_fix_attempts",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Resets a task's CI-fix counter (used when the agent circles back to a blocked
/// PR, so the fresh attempt gets the full retry budget again).
pub async fn reset_ci_fix_attempts(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE tasks SET ci_fix_attempts = 0, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resets a task's review-address counter, so an idle revisit of a review-blocked
/// PR gets the full addressing budget again rather than re-parking immediately.
pub async fn reset_review_fix_attempts(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE tasks SET review_fix_attempts = 0, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The oldest blocked PR worth revisiting while the agent is otherwise idle: in
/// review, `ci_blocked`, not on hold, and untouched for at least `cooldown_secs`
/// (so a genuinely stuck PR is retried periodically, not in a tight loop).
pub async fn pick_next_revisit(
    pool: &PgPool,
    railway_id: Uuid,
    cooldown_secs: i64,
) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'ci_blocked' \
         AND hold = FALSE AND railway_id = $1 \
         AND (last_activity_at IS NULL OR last_activity_at < now() - ($2 * interval '1 second')) \
         ORDER BY last_activity_at ASC NULLS FIRST LIMIT 1",
    )
    .bind(railway_id)
    .bind(cooldown_secs)
    .fetch_optional(pool)
    .await
}

/// Leaves a PR in review for a human, recording why the agent stopped on CI. The
/// card keeps its `in_review` lane; only the status and error note change.
pub async fn block_task_ci(pool: &PgPool, id: Uuid, note: &str) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET status = $2, error = $3, finished_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(TaskStatus::CiBlocked)
    .bind(note)
    .fetch_one(pool)
    .await
}

/// Flags a PR whose auto-merge failed (typically a conflict with the base) for
/// the agent to resolve. The card keeps its `in_review` lane and PR; the status
/// and note change so the agent picks it up proactively.
///
/// Unlike [`block_task_ci`], this does not set `finished_at`: the task is not
/// finished, just handed back. `last_activity_at` is refreshed so the card sorts
/// as freshly touched and any later idle-revisit cooldown is measured from now.
pub async fn flag_merge_conflict(pool: &PgPool, id: Uuid, note: &str) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET status = $2, error = $3, last_activity_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(TaskStatus::MergeConflict)
    .bind(note)
    .fetch_one(pool)
    .await
}

/// Flags a green, (auto-)approved PR that still has unresolved review threads for
/// the agent to address before the merge. The card keeps its `in_review` lane and
/// PR; only the status changes so the agent picks it up proactively.
///
/// Like [`flag_merge_conflict`] this does not set `finished_at` (the task is not
/// finished, just handed back). It clears `error` because this is a normal step,
/// not a failure, so a stale CI/conflict note doesn't linger on the card.
pub async fn flag_review_addressing(pool: &PgPool, id: Uuid) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET status = $2, error = NULL, last_activity_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(TaskStatus::AddressingReview)
    .fetch_one(pool)
    .await
}

/// Records the branch and session a task is being worked under, and stamps it
/// as started.
pub async fn mark_task_started(
    pool: &PgPool,
    id: Uuid,
    branch: &str,
    session_id: Option<&str>,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET branch = $2, session_id = $3, started_at = now(), \
         last_activity_at = now(), updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(branch)
    .bind(session_id)
    .fetch_one(pool)
    .await
}

pub async fn set_task_pr(pool: &PgPool, id: Uuid, pr_url: &str) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET pr_url = $2, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(pr_url)
    .fetch_one(pool)
    .await
}

/// In-flight tasks on a railway that have at least one open PR and a branch, as
/// candidate dependencies for a new ticket (issue #256). Excludes `exclude_id`
/// (the ticket being started). One row per task (a task with several open PRs
/// collapses to one). The caller matches the new ticket's `Depends on:`
/// references against these and resolves the matches to their PR branches.
pub async fn list_open_dependency_candidates(
    pool: &PgPool,
    railway_id: Uuid,
    exclude_id: Uuid,
) -> sqlx::Result<Vec<DependencyCandidate>> {
    sqlx::query_as::<_, DependencyCandidate>(
        "SELECT DISTINCT t.id, t.source_kind, t.external_id, t.title, t.branch \
         FROM tasks t \
         JOIN task_pull_requests p ON p.task_id = t.id AND p.pr_state = 'open' \
         WHERE t.railway_id = $1 AND t.id <> $2 AND t.branch IS NOT NULL",
    )
    .bind(railway_id)
    .bind(exclude_id)
    .fetch_all(pool)
    .await
}

// --- Agent screenshots (issue #248) ------------------------------------------

/// The screenshot metadata columns, every column EXCEPT the `image` bytea, so a
/// `SELECT` for a list or RETURNING never drags the bytes into a JSON payload.
const SCREENSHOT_COLUMNS: &str =
    "id, task_id, turn_id, mime, width, height, route, caption, created_at";

/// Stores one captured screenshot and returns its metadata (never the bytes).
/// `turn_id` best-effort associates it with the turn that captured it; `width` /
/// `height` are `None` when the uploader could not determine them.
#[allow(clippy::too_many_arguments)]
pub async fn create_screenshot(
    pool: &PgPool,
    task_id: Uuid,
    turn_id: Option<Uuid>,
    image: &[u8],
    mime: &str,
    width: Option<i32>,
    height: Option<i32>,
    route: &str,
    caption: &str,
) -> sqlx::Result<TaskScreenshot> {
    sqlx::query_as::<_, TaskScreenshot>(&format!(
        "INSERT INTO task_screenshots \
         (task_id, turn_id, image, mime, width, height, route, caption) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {SCREENSHOT_COLUMNS}"
    ))
    .bind(task_id)
    .bind(turn_id)
    .bind(image)
    .bind(mime)
    .bind(width)
    .bind(height)
    .bind(route)
    .bind(caption)
    .fetch_one(pool)
    .await
}

/// A task's screenshots, newest first, metadata only (the bytes are streamed by id).
pub async fn list_screenshots_for_task(
    pool: &PgPool,
    task_id: Uuid,
) -> sqlx::Result<Vec<TaskScreenshot>> {
    sqlx::query_as::<_, TaskScreenshot>(&format!(
        "SELECT {SCREENSHOT_COLUMNS} FROM task_screenshots \
         WHERE task_id = $1 ORDER BY created_at DESC"
    ))
    .bind(task_id)
    .fetch_all(pool)
    .await
}

/// The raw bytes and MIME of one screenshot, for the streaming endpoint. `None`
/// when no screenshot has that id.
pub async fn get_screenshot_image(
    pool: &PgPool,
    id: Uuid,
) -> sqlx::Result<Option<(Vec<u8>, String)>> {
    sqlx::query_as::<_, (Vec<u8>, String)>("SELECT image, mime FROM task_screenshots WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// The id of a task's most recent turn, to associate an uploaded screenshot with
/// the turn that captured it. `None` for a task that has no turns yet.
pub async fn latest_turn_id(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM turns WHERE task_id = $1 ORDER BY idx DESC LIMIT 1")
        .bind(task_id)
        .fetch_optional(pool)
        .await
}

/// Every pull request tracked for a task, oldest first.
pub async fn list_task_prs(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Vec<TaskPullRequest>> {
    sqlx::query_as::<_, TaskPullRequest>(
        "SELECT * FROM task_pull_requests WHERE task_id = $1 ORDER BY created_at",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
}

/// Every open pull request across all tasks, for the CI-step watcher. Merged /
/// closed PRs are settled, so the watcher ignores them.
pub async fn list_open_task_prs(pool: &PgPool) -> sqlx::Result<Vec<TaskPullRequest>> {
    sqlx::query_as::<_, TaskPullRequest>(
        "SELECT * FROM task_pull_requests WHERE pr_state = 'open' ORDER BY task_id, created_at",
    )
    .fetch_all(pool)
    .await
}

/// Prompt sentinel marking a synthetic turn that holds CI-step activity rather
/// than a real Claude invocation. The watcher appends its events here so they
/// flow through the same activity-log query as agent events, without a schema
/// change. No `prompt` event is recorded for it, so it shows nothing of itself.
const CI_TURN_PROMPT: &str = "[CI activity]";

/// Finds the turn the CI watcher should append to, or creates one. Reuses the
/// latest turn only when it is already a CI turn, so CI events that arrive after
/// a fresh agent turn (e.g. a CI-fix turn) open a new CI turn and stay in
/// chronological order in the activity log.
pub async fn get_or_create_ci_turn(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Turn> {
    let latest = sqlx::query_as::<_, Turn>(
        "SELECT * FROM turns WHERE task_id = $1 ORDER BY idx DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;
    if let Some(turn) = latest {
        if turn.prompt == CI_TURN_PROMPT {
            return Ok(turn);
        }
    }
    let idx = next_turn_idx(pool, task_id).await?;
    create_turn(pool, task_id, idx, CI_TURN_PROMPT, None).await
}

/// The next event sequence number within a turn (max + 1, or 0 for the first).
pub async fn next_event_seq(pool: &PgPool, turn_id: Uuid) -> sqlx::Result<i32> {
    let max: Option<i32> = sqlx::query_scalar("SELECT MAX(seq) FROM events WHERE turn_id = $1")
        .bind(turn_id)
        .fetch_one(pool)
        .await?;
    Ok(max.map_or(0, |value| value + 1))
}

/// Records (or refreshes) a task's pull request, keyed by `(task, repo, number)`.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_task_pr(
    pool: &PgPool,
    task_id: Uuid,
    repo_id: Option<Uuid>,
    repo_full_name: &str,
    pr_number: i64,
    pr_url: &str,
    head_sha: &str,
    ci_state: &str,
    pr_state: &str,
) -> sqlx::Result<TaskPullRequest> {
    sqlx::query_as::<_, TaskPullRequest>(
        "INSERT INTO task_pull_requests \
           (task_id, repo_id, repo_full_name, pr_number, pr_url, head_sha, ci_state, pr_state) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (task_id, repo_full_name, pr_number) DO UPDATE SET \
           repo_id = EXCLUDED.repo_id, pr_url = EXCLUDED.pr_url, head_sha = EXCLUDED.head_sha, \
           ci_state = EXCLUDED.ci_state, pr_state = EXCLUDED.pr_state, updated_at = now() \
         RETURNING *",
    )
    .bind(task_id)
    .bind(repo_id)
    .bind(repo_full_name)
    .bind(pr_number)
    .bind(pr_url)
    .bind(head_sha)
    .bind(ci_state)
    .bind(pr_state)
    .fetch_one(pool)
    .await
}

pub async fn set_task_error(pool: &PgPool, id: Uuid, error: &str) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET status = 'failed', error = $2, finished_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(error)
    .fetch_one(pool)
    .await
}

pub async fn finish_task(
    pool: &PgPool,
    id: Uuid,
    column: TaskColumn,
    status: TaskStatus,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET board_column = $2, status = $3, finished_at = now(), updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(column)
    .bind(status)
    .fetch_one(pool)
    .await
}

// --- Turns -------------------------------------------------------------------

pub async fn next_turn_idx(pool: &PgPool, task_id: Uuid) -> sqlx::Result<i32> {
    let max: Option<i32> = sqlx::query_scalar("SELECT MAX(idx) FROM turns WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(pool)
        .await?;
    Ok(max.map_or(0, |value| value + 1))
}

pub async fn create_turn(
    pool: &PgPool,
    task_id: Uuid,
    idx: i32,
    prompt: &str,
    session_id: Option<&str>,
) -> sqlx::Result<Turn> {
    sqlx::query_as::<_, Turn>(
        "INSERT INTO turns (task_id, idx, prompt, session_id) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(task_id)
    .bind(idx)
    .bind(prompt)
    .bind(session_id)
    .fetch_one(pool)
    .await
}

pub async fn finish_turn(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    result_text: Option<&str>,
    total_cost_usd: Option<f64>,
    token_usage: Option<Value>,
    session_id: Option<&str>,
) -> sqlx::Result<()> {
    // Strip NUL bytes; Postgres TEXT can't store them either.
    let result_text = result_text.map(|text| text.replace('\0', ""));
    sqlx::query(
        "UPDATE turns SET status = $2, result_text = $3, total_cost_usd = $4, \
         token_usage = COALESCE($5, token_usage), \
         session_id = COALESCE($6, session_id), finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(result_text.as_deref())
    .bind(total_cost_usd)
    .bind(token_usage.map(Json))
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Statistics --------------------------------------------------------------

/// The SELECT list shared by the task and global aggregates. Token fields read
/// out of the per-turn `token_usage` JSON; `worked_ms` sums elapsed turn time.
const STATS_SELECT: &str = "\
    COALESCE(SUM(total_cost_usd), 0)::float8 AS cost_usd, \
    COALESCE(SUM((token_usage->>'input_tokens')::bigint), 0)::bigint AS input_tokens, \
    COALESCE(SUM((token_usage->>'output_tokens')::bigint), 0)::bigint AS output_tokens, \
    COALESCE(SUM((token_usage->>'cache_creation_input_tokens')::bigint), 0)::bigint AS cache_creation_tokens, \
    COALESCE(SUM((token_usage->>'cache_read_input_tokens')::bigint), 0)::bigint AS cache_read_tokens, \
    COALESCE(SUM(EXTRACT(EPOCH FROM (finished_at - started_at)) * 1000), 0)::bigint AS worked_ms, \
    COUNT(*)::bigint AS turns";

/// Aggregated usage for one task, over turns since its stats reset.
pub async fn task_stats(pool: &PgPool, task_id: Uuid) -> sqlx::Result<StatsAggregate> {
    sqlx::query_as::<_, StatsAggregate>(&format!(
        "SELECT {STATS_SELECT} FROM turns \
         WHERE task_id = $1 \
         AND started_at > COALESCE((SELECT stats_reset_at FROM tasks WHERE id = $1), 'epoch'::timestamptz)"
    ))
    .bind(task_id)
    .fetch_one(pool)
    .await
}

/// Aggregated usage across all tasks, over turns since the global stats reset.
pub async fn global_stats(pool: &PgPool) -> sqlx::Result<StatsAggregate> {
    sqlx::query_as::<_, StatsAggregate>(&format!(
        "SELECT {STATS_SELECT} FROM turns \
         WHERE started_at > COALESCE((SELECT stats_reset_at FROM settings WHERE id = 1), 'epoch'::timestamptz)"
    ))
    .fetch_one(pool)
    .await
}

/// Aggregated usage for one railway, over turns of its tasks since the global
/// stats reset. A turn belongs to a railway through its task's `railway_id`, so
/// this mirrors [`global_stats`] but scoped to the lane's cards.
pub async fn railway_stats(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<StatsAggregate> {
    // Filter by railway via a subquery rather than a JOIN to `tasks`: `tasks`
    // also has `started_at`/`finished_at`, so joining it would make the unqualified
    // columns in the shared STATS_SELECT ambiguous. With only `turns` in scope they
    // resolve cleanly, and STATS_SELECT stays usable by the non-joined callers.
    sqlx::query_as::<_, StatsAggregate>(&format!(
        "SELECT {STATS_SELECT} FROM turns \
         WHERE turns.task_id IN (SELECT id FROM tasks WHERE railway_id = $1) \
         AND turns.started_at > COALESCE((SELECT stats_reset_at FROM settings WHERE id = 1), 'epoch'::timestamptz)"
    ))
    .bind(railway_id)
    .fetch_one(pool)
    .await
}

/// When any currently-running turn on this railway started, for its live ticker.
pub async fn railway_running_since(
    pool: &PgPool,
    railway_id: Uuid,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT turns.started_at FROM turns \
         JOIN tasks ON tasks.id = turns.task_id \
         WHERE tasks.railway_id = $1 AND turns.status = 'running' \
         ORDER BY turns.started_at DESC LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// The latest turn's token usage on this railway, approximating its context fill.
pub async fn railway_latest_usage(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<Option<Value>> {
    let row: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT turns.token_usage FROM turns \
         JOIN tasks ON tasks.id = turns.task_id \
         WHERE tasks.railway_id = $1 AND turns.token_usage IS NOT NULL \
         ORDER BY turns.started_at DESC LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|json| json.0))
}

/// When the currently-running turn for a task started, for the live time ticker.
pub async fn task_running_since(
    pool: &PgPool,
    task_id: Uuid,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT started_at FROM turns WHERE task_id = $1 AND status = 'running' \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
}

/// The start time of every currently-running turn, across all railways.
///
/// Railways run turns in parallel, so the global worked-time tick must account for
/// each running turn's elapsed time, not just one. The handler folds these into the
/// reported `worked_ms` (persisted total plus each turn's elapsed so far) and the
/// `running_turns` count the client uses to keep ticking at the combined rate.
pub async fn global_running_turns(pool: &PgPool) -> sqlx::Result<Vec<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT started_at FROM turns WHERE status = 'running' ORDER BY started_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// The latest turn's token usage for a task, approximating its current context.
pub async fn task_latest_usage(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Option<Value>> {
    let row: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT token_usage FROM turns WHERE task_id = $1 AND token_usage IS NOT NULL \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|json| json.0))
}

/// The latest turn's token usage overall, approximating the session context.
pub async fn global_latest_usage(pool: &PgPool) -> sqlx::Result<Option<Value>> {
    let row: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT token_usage FROM turns WHERE token_usage IS NOT NULL \
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|json| json.0))
}

/// The most recent rate-limit notice payload (its `rate_limit_info` carries the
/// subscription utilization), for the usage-limit gauge.
pub async fn latest_rate_limit(pool: &PgPool) -> sqlx::Result<Option<Value>> {
    let row: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT payload FROM events WHERE type = 'rate_limit' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|json| json.0))
}

/// Resets global statistics (non-destructive): future reads only count turns
/// started after now.
pub async fn reset_global_stats(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET stats_reset_at = now() WHERE id = 1")
        .execute(pool)
        .await?;
    Ok(())
}

/// Purges all conversation history: every turn and its events (events cascade
/// from turns). Used by the hard reset to wipe the agent's recorded history,
/// which also zeroes the turn-derived statistics.
pub async fn purge_history(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM turns").execute(pool).await?;
    Ok(result.rows_affected())
}

// --- Events ------------------------------------------------------------------

pub async fn append_event(
    pool: &PgPool,
    turn_id: Uuid,
    seq: i32,
    event_type: &str,
    mut payload: Value,
) -> sqlx::Result<()> {
    // Postgres JSONB rejects the NUL escape ( ), so a tool result carrying a
    // null byte (e.g. binary output) would otherwise fail the insert and abort
    // the whole turn. Strip NULs from every string first.
    strip_nul(&mut payload);
    sqlx::query("INSERT INTO events (turn_id, seq, type, payload) VALUES ($1, $2, $3, $4)")
        .bind(turn_id)
        .bind(seq)
        .bind(event_type)
        .bind(Json(payload))
        .execute(pool)
        .await?;
    Ok(())
}

/// Removes NUL (` `) characters from every string in a JSON value.
///
/// Postgres JSONB (and TEXT) cannot store NUL, and Claude's tool output can
/// carry one (binary reads, odd command output). Dropping them keeps the event
/// persistable without otherwise altering the content.
fn strip_nul(value: &mut Value) {
    match value {
        Value::String(text) => {
            if text.contains('\0') {
                text.retain(|character| character != '\0');
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_nul),
        Value::Object(map) => map.values_mut().for_each(strip_nul),
        _ => {}
    }
}

/// All events for a task in order, joined across its turns. Powers the task
/// detail view's chat history.
pub async fn list_events_for_task(
    pool: &PgPool,
    task_id: Uuid,
) -> sqlx::Result<Vec<super::models::Event>> {
    // `rate_limit` events are persisted only to feed the usage gauge's fallback
    // (`latest_rate_limit`); they are not activity, so they are omitted from the
    // activity log the task view renders (issue #182).
    sqlx::query_as::<_, super::models::Event>(
        "SELECT e.* FROM events e JOIN turns t ON e.turn_id = t.id \
         WHERE t.task_id = $1 AND e.type <> 'rate_limit' ORDER BY t.idx, e.seq",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
}

// --- Environment suggestions -------------------------------------------------

/// Records one setup recommendation the agent made for a task.
pub async fn create_suggestion(
    pool: &PgPool,
    task_id: Uuid,
    kind: &str,
    title: &str,
    detail: &str,
) -> sqlx::Result<EnvSuggestion> {
    sqlx::query_as::<_, EnvSuggestion>(
        "INSERT INTO environment_suggestions (task_id, kind, title, detail) \
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(task_id)
    .bind(kind)
    .bind(title)
    .bind(detail)
    .fetch_one(pool)
    .await
}

/// Titles of every task that is not finished, i.e. still on the board as work
/// (Available / To Do / In Progress / In Review). Used as the light de-dup check
/// (issue #272) so the agent never recommends follow-up work already queued.
pub async fn open_task_titles(pool: &PgPool) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar("SELECT title FROM tasks WHERE board_column NOT IN ('done', 'ignored')")
        .fetch_all(pool)
        .await
}

/// One suggestion by id (to act on it, e.g. create an issue from it).
pub async fn get_suggestion(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<EnvSuggestion>> {
    sqlx::query_as::<_, EnvSuggestion>("SELECT * FROM environment_suggestions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Every suggestion on a task, oldest first, for the task detail view.
pub async fn list_suggestions_for_task(
    pool: &PgPool,
    task_id: Uuid,
) -> sqlx::Result<Vec<EnvSuggestion>> {
    sqlx::query_as::<_, EnvSuggestion>(
        "SELECT * FROM environment_suggestions WHERE task_id = $1 ORDER BY created_at",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
}

/// Checks or unchecks a suggestion, stamping the time when it is acknowledged.
pub async fn set_suggestion_acknowledged(
    pool: &PgPool,
    id: Uuid,
    acknowledged: bool,
) -> sqlx::Result<EnvSuggestion> {
    sqlx::query_as::<_, EnvSuggestion>(
        "UPDATE environment_suggestions \
         SET acknowledged = $2, acknowledged_at = CASE WHEN $2 THEN now() ELSE NULL END \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(acknowledged)
    .fetch_one(pool)
    .await
}

/// Unacknowledged-suggestion counts per task, for the loud board badges. Tasks
/// with no open suggestions are omitted.
pub async fn unacknowledged_suggestion_counts(pool: &PgPool) -> sqlx::Result<Vec<(Uuid, i64)>> {
    sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT task_id, COUNT(*) FROM environment_suggestions \
         WHERE acknowledged = FALSE GROUP BY task_id",
    )
    .fetch_all(pool)
    .await
}

// --- Heart attacks (dead-agent management) -----------------------------------

/// Records a heart attack: a turn that died mid-flight. The detail is the
/// diagnosis kept for later patching; `recovery` is what the defibrillator did.
pub async fn create_heart_attack(
    pool: &PgPool,
    task_id: Option<Uuid>,
    task_title: &str,
    status_label: &str,
    detail: &str,
    recovery: &str,
) -> sqlx::Result<HeartAttack> {
    sqlx::query_as::<_, HeartAttack>(
        "INSERT INTO heart_attacks (task_id, task_title, status_label, detail, recovery) \
         VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(task_id)
    .bind(task_title)
    .bind(status_label)
    .bind(detail)
    .bind(recovery)
    .fetch_one(pool)
    .await
}

/// The unacknowledged heart attacks, newest first, for the board's alert banner.
/// Bounded so a storm of incidents can't bloat the board payload.
pub async fn list_unacknowledged_heart_attacks(pool: &PgPool) -> sqlx::Result<Vec<HeartAttack>> {
    sqlx::query_as::<_, HeartAttack>(
        "SELECT * FROM heart_attacks WHERE acknowledged = FALSE \
         ORDER BY created_at DESC LIMIT 20",
    )
    .fetch_all(pool)
    .await
}

/// Clears a heart attack once the operator has seen it.
pub async fn acknowledge_heart_attack(pool: &PgPool, id: Uuid) -> sqlx::Result<HeartAttack> {
    sqlx::query_as::<_, HeartAttack>(
        "UPDATE heart_attacks SET acknowledged = TRUE, acknowledged_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// How many heart attacks a task has suffered. Bounds how many times the
/// defibrillator revives the same task before leaving it for a human.
pub async fn count_heart_attacks_for_task(pool: &PgPool, task_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM heart_attacks WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(pool)
        .await
}

/// The task that has been mid-turn (`working`) with no activity for at least
/// `stale_secs`, if any. The defibrillator watchdog uses this to catch a turn
/// that hung or stranded the card without the in-turn heartbeat noticing. Picks
/// the most stale first.
pub async fn find_stranded_task(pool: &PgPool, stale_secs: i64) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks \
         WHERE board_column = 'in_progress' AND status = 'working' \
         AND last_activity_at IS NOT NULL \
         AND last_activity_at < now() - ($1 * interval '1 second') \
         ORDER BY last_activity_at ASC LIMIT 1",
    )
    .bind(stale_secs)
    .fetch_optional(pool)
    .await
}

// --- Questions ---------------------------------------------------------------

/// Records one question the agent is asking the user, in `pending` state.
pub async fn create_question(
    pool: &PgPool,
    task_id: Uuid,
    prompt: &str,
    options: &[QuestionOption],
) -> sqlx::Result<Question> {
    sqlx::query_as::<_, Question>(
        "INSERT INTO questions (task_id, prompt, options) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(task_id)
    .bind(prompt)
    .bind(Json(options))
    .fetch_one(pool)
    .await
}

pub async fn get_question(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Question>> {
    sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Every question on a task, oldest first; powers the decision history on the
/// task detail view.
pub async fn list_questions_for_task(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Vec<Question>> {
    sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE task_id = $1 ORDER BY created_at")
        .bind(task_id)
        .fetch_all(pool)
        .await
}

/// All unanswered questions across every task, for the notifications sidebar.
pub async fn list_pending_questions(pool: &PgPool) -> sqlx::Result<Vec<PendingQuestion>> {
    sqlx::query_as::<_, PendingQuestion>(
        "SELECT q.id, q.task_id, t.title AS task_title, q.prompt, q.options, q.created_at \
         FROM questions q JOIN tasks t ON q.task_id = t.id \
         WHERE q.status = 'pending' ORDER BY q.created_at",
    )
    .fetch_all(pool)
    .await
}

/// Records the user's answer to a question and stamps it answered.
pub async fn answer_question(
    pool: &PgPool,
    id: Uuid,
    status: QuestionStatus,
    answer_kind: AnswerKind,
    answer: &str,
) -> sqlx::Result<Question> {
    sqlx::query_as::<_, Question>(
        "UPDATE questions SET status = $2, answer_kind = $3, answer = $4, answered_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(status)
    .bind(answer_kind)
    .bind(answer)
    .fetch_one(pool)
    .await
}

/// How many questions on a task are still awaiting an answer.
pub async fn count_pending_questions(pool: &PgPool, task_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM questions WHERE task_id = $1 AND status = 'pending'",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
}

/// A task that is parked on a question whose answer has arrived but not yet been
/// delivered to the agent: in progress, nothing pending, at least one answered
/// question still unacknowledged. This is what the agent loop resumes.
pub async fn pick_resume_ready(pool: &PgPool, railway_id: Uuid) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT t.* FROM tasks t \
         WHERE t.board_column = 'in_progress' AND t.railway_id = $1 \
           AND EXISTS (SELECT 1 FROM questions q WHERE q.task_id = t.id \
                       AND q.status <> 'pending' AND q.acknowledged = FALSE) \
           AND NOT EXISTS (SELECT 1 FROM questions q WHERE q.task_id = t.id \
                           AND q.status = 'pending') \
         ORDER BY t.position LIMIT 1",
    )
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// The answered-but-undelivered questions for a task, oldest first, that a resume
/// turn should hand back to the agent.
pub async fn list_unacknowledged_answers(
    pool: &PgPool,
    task_id: Uuid,
) -> sqlx::Result<Vec<Question>> {
    sqlx::query_as::<_, Question>(
        "SELECT * FROM questions WHERE task_id = $1 AND status <> 'pending' \
         AND acknowledged = FALSE ORDER BY created_at",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
}

/// Marks delivered answers acknowledged so a later resume does not repeat them.
pub async fn acknowledge_answers(pool: &PgPool, task_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE questions SET acknowledged = TRUE \
         WHERE task_id = $1 AND status <> 'pending' AND acknowledged = FALSE",
    )
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Compose assistant (issue #181) ------------------------------------------
//
// A second, on-demand Claude session with its own conversation and statistics,
// stored in dedicated tables so nothing here touches the board, the agent loop,
// or the main shared session.

/// The session id of the most recent compose turn, to resume the conversation.
/// `None` when there is no history yet (a fresh conversation), e.g. after a reset.
pub async fn latest_compose_session_id(pool: &PgPool) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar(
        "SELECT session_id FROM compose_turns WHERE session_id IS NOT NULL \
         ORDER BY idx DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}

/// Whether a compose turn is currently running (so a second is rejected).
pub async fn compose_turn_running(pool: &PgPool) -> sqlx::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM compose_turns WHERE status = 'running'")
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

/// Opens a new compose turn (the next index), returning its id.
pub async fn create_compose_turn(
    pool: &PgPool,
    prompt: &str,
    session_id: Option<&str>,
) -> sqlx::Result<Uuid> {
    let next_idx: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(idx) + 1, 0) FROM compose_turns")
        .fetch_one(pool)
        .await?;
    sqlx::query_scalar(
        "INSERT INTO compose_turns (idx, prompt, session_id) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(next_idx)
    .bind(prompt)
    .bind(session_id)
    .fetch_one(pool)
    .await
}

/// Finalizes a compose turn with its outcome (mirrors [`finish_turn`]).
pub async fn finish_compose_turn(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    result_text: Option<&str>,
    total_cost_usd: Option<f64>,
    token_usage: Option<Value>,
    session_id: Option<&str>,
) -> sqlx::Result<()> {
    let result_text = result_text.map(|text| text.replace('\0', ""));
    sqlx::query(
        "UPDATE compose_turns SET status = $2, result_text = $3, total_cost_usd = $4, \
         token_usage = COALESCE($5, token_usage), \
         session_id = COALESCE($6, session_id), finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(result_text.as_deref())
    .bind(total_cost_usd)
    .bind(token_usage.map(Json))
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Appends one parsed event to a compose turn's transcript.
pub async fn append_compose_event(
    pool: &PgPool,
    turn_id: Uuid,
    seq: i32,
    event_type: &str,
    mut payload: Value,
) -> sqlx::Result<()> {
    strip_nul(&mut payload);
    sqlx::query("INSERT INTO compose_events (turn_id, seq, type, payload) VALUES ($1, $2, $3, $4)")
        .bind(turn_id)
        .bind(seq)
        .bind(event_type)
        .bind(Json(payload))
        .execute(pool)
        .await?;
    Ok(())
}

/// The whole compose transcript in order, for the chat history on reload.
pub async fn list_compose_events(pool: &PgPool) -> sqlx::Result<Vec<super::models::Event>> {
    sqlx::query_as::<_, super::models::Event>(
        "SELECT e.* FROM compose_events e JOIN compose_turns t ON e.turn_id = t.id \
         WHERE e.type <> 'rate_limit' ORDER BY t.idx, e.seq",
    )
    .fetch_all(pool)
    .await
}

/// Aggregated usage for the compose assistant (mirrors [`global_stats`]).
pub async fn compose_stats(pool: &PgPool) -> sqlx::Result<StatsAggregate> {
    sqlx::query_as::<_, StatsAggregate>(&format!("SELECT {STATS_SELECT} FROM compose_turns"))
        .fetch_one(pool)
        .await
}

/// When the running compose turn started, for the live time ticker.
pub async fn compose_running_since(pool: &PgPool) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT started_at FROM compose_turns WHERE status = 'running' \
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

/// The latest compose turn's token usage, approximating its context fill.
pub async fn compose_latest_usage(pool: &PgPool) -> sqlx::Result<Option<Value>> {
    let row: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT token_usage FROM compose_turns WHERE token_usage IS NOT NULL \
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|json| json.0))
}

/// Wipes the compose conversation (turns + events cascade). Used by reset.
pub async fn clear_compose_history(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM compose_turns")
        .execute(pool)
        .await?;
    Ok(())
}

/// Every draft issue, in display order.
pub async fn list_drafts(pool: &PgPool) -> sqlx::Result<Vec<super::models::IssueDraft>> {
    sqlx::query_as::<_, super::models::IssueDraft>(
        "SELECT * FROM issue_drafts ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
}

/// Replaces the entire draft set with `drafts` (`title`, `body`, optional
/// `repo_id`, optional `railway_id`), positioned in the given order. The compose
/// agent always sends the full desired list, so a replace keeps its view and ours
/// in sync. The `seraphim-draft` helper does not set a railway, so a replace
/// preserves the operator's per-draft railway choice by re-applying it to the
/// matching kept draft (matched by title) when the incoming entry has none.
pub async fn replace_drafts(
    pool: &PgPool,
    drafts: &[(String, String, Option<Uuid>, Option<Uuid>)],
) -> sqlx::Result<Vec<super::models::IssueDraft>> {
    // Snapshot the operator's railway choices before the wipe so the agent's
    // repo/body-only replace does not silently reset the lane assignments.
    let existing = list_drafts(pool).await?;

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM issue_drafts")
        .execute(&mut *tx)
        .await?;
    for (index, (title, body, repo_id, railway_id)) in drafts.iter().enumerate() {
        let railway_id = railway_id.or_else(|| {
            existing
                .iter()
                .find(|draft| draft.title == *title)
                .and_then(|draft| draft.railway_id)
        });
        sqlx::query(
            "INSERT INTO issue_drafts (title, body, repo_id, railway_id, position) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(title)
        .bind(body)
        .bind(repo_id)
        .bind(railway_id)
        .bind(index as f64)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    list_drafts(pool).await
}

/// Edits one draft's title, body, target repo, and target railway. Returns `None`
/// if it's gone.
pub async fn update_draft(
    pool: &PgPool,
    id: Uuid,
    title: &str,
    body: &str,
    repo_id: Option<Uuid>,
    railway_id: Option<Uuid>,
) -> sqlx::Result<Option<super::models::IssueDraft>> {
    sqlx::query_as::<_, super::models::IssueDraft>(
        "UPDATE issue_drafts \
         SET title = $2, body = $3, repo_id = $4, railway_id = $5, updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(title)
    .bind(body)
    .bind(repo_id)
    .bind(railway_id)
    .fetch_optional(pool)
    .await
}

/// Reorders the drafts to exactly `ordered_ids` (the planner's dependency
/// sequence), rewriting each draft's `position` to its index. Ids not present are
/// left after the ordered run, preserving their relative order, so a stale id in
/// the request never drops a draft.
pub async fn reorder_drafts(
    pool: &PgPool,
    ordered_ids: &[Uuid],
) -> sqlx::Result<Vec<super::models::IssueDraft>> {
    let mut tx = pool.begin().await?;
    for (index, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE issue_drafts SET position = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(index as f64)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    list_drafts(pool).await
}

/// Deletes one draft.
pub async fn delete_draft(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM issue_drafts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes every draft (used by reset and after a successful bulk-create).
pub async fn clear_drafts(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM issue_drafts")
        .execute(pool)
        .await?;
    Ok(())
}

// --- Pending placements ------------------------------------------------------

/// Records (or replaces) the planner's intended board placement for an issue it
/// bulk-created on an external tracker, so the first sync that brings the issue in
/// can land its card in the chosen lane and order instead of the top of Available.
///
/// Keyed by the issue's identity (`source_kind`, `repo_id`, `external_id`); a
/// re-created draft for the same issue overwrites the prior intent. The stored
/// `railway_id` is the planner's chosen lane, only authoritative for a repo-less
/// issue (a repo-bound issue follows its repo's railway when consumed).
pub async fn create_pending_placement(
    pool: &PgPool,
    source_kind: SourceKind,
    repo_id: Option<Uuid>,
    external_id: &str,
    position: f64,
    railway_id: Option<Uuid>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO pending_placements (source_kind, repo_id, external_id, position, railway_id) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (source_kind, repo_id, external_id) DO UPDATE SET \
         position = EXCLUDED.position, \
         railway_id = EXCLUDED.railway_id, \
         created_at = now()",
    )
    .bind(source_kind)
    .bind(repo_id)
    .bind(external_id)
    .bind(position)
    .bind(railway_id)
    .fetch_optional(pool)
    .await?;
    Ok(())
}

/// Fetches and deletes the pending placement for an issue identity, if one exists.
///
/// Returning the deleted row makes the read-and-consume a single atomic statement,
/// so a placement is never applied twice. Matches Jira's NULL `repo_id` via `IS
/// NOT DISTINCT FROM`. Returns `None` (the overwhelmingly common case) when the
/// issue was not planner-created, leaving the caller's default placement intact.
pub async fn take_pending_placement(
    pool: &PgPool,
    source_kind: SourceKind,
    repo_id: Option<Uuid>,
    external_id: &str,
) -> sqlx::Result<Option<PendingPlacement>> {
    sqlx::query_as::<_, PendingPlacement>(
        "DELETE FROM pending_placements \
         WHERE source_kind = $1 AND repo_id IS NOT DISTINCT FROM $2 AND external_id = $3 \
         RETURNING *",
    )
    .bind(source_kind)
    .bind(repo_id)
    .bind(external_id)
    .fetch_optional(pool)
    .await
}

/// Deletes pending placements older than `max_age_days`, returning how many were
/// removed. A placement is only consumed when its issue first syncs in, so one
/// whose issue is deleted on the tracker before it ever syncs would otherwise
/// linger forever; this best-effort sweep keeps the table bounded.
pub async fn prune_stale_pending_placements(pool: &PgPool, max_age_days: i32) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM pending_placements \
         WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(max_age_days)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
