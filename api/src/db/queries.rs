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
    AnswerKind, AutomationRule, AvailabilityWindow, EnvSuggestion, EnvVar, EnvVarWrite,
    HeartAttack, InternalComment, JiraBoard, JiraDeployment, NetworkAccessLevel, PendingQuestion,
    Question, QuestionOption, QuestionStatus, RepoDeletionImpact, Repository, ReviewPolicy,
    Settings, SourceKind, StatsAggregate, Task, TaskColumn, TaskPullRequest, TaskStatus, Turn,
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
     (github_token <> '') AS github_token_set, \
     availability_enabled, availability_timezone, availability_windows, \
     availability_skip_dates, network_access_level, network_access_domains, \
     network_access_include_defaults, usage_limit_pause_enabled, \
     usage_limit_threshold, usage_paused_until, post_thoughts_enabled, \
     close_issue_on_done, \
     jira_enabled, jira_deployment, jira_base_url, jira_email, \
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
    post_thoughts_enabled: Option<bool>,
    jira_enabled: Option<bool>,
    jira_deployment: Option<JiraDeployment>,
    jira_base_url: Option<String>,
    jira_email: Option<String>,
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
         post_thoughts_enabled = COALESCE($17, post_thoughts_enabled), \
         jira_enabled = COALESCE($18, jira_enabled), \
         jira_deployment = COALESCE($19, jira_deployment), \
         jira_base_url = COALESCE($20, jira_base_url), \
         jira_email = COALESCE($21, jira_email), \
         close_issue_on_done = COALESCE($22, close_issue_on_done), \
         attention_sound_enabled = COALESCE($23, attention_sound_enabled), \
         completion_sound_enabled = COALESCE($24, completion_sound_enabled), \
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
    .bind(post_thoughts_enabled)
    .bind(jira_enabled)
    .bind(jira_deployment)
    .bind(jira_base_url)
    .bind(jira_email)
    .bind(close_issue_on_done)
    .bind(attention_sound_enabled)
    .bind(completion_sound_enabled)
    .fetch_one(pool)
    .await
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

pub async fn set_current_session_id(pool: &PgPool, session_id: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET current_session_id = $1, updated_at = now() WHERE id = 1")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

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
         SELECT jira_api_token FROM settings WHERE id = 1 AND jira_api_token <> ''",
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
        "INSERT INTO repositories \
         (full_name, clone_url, default_branch, branch_template, setup_script, instructions, \
          review_policy, enabled, sync_issues, issue_labels) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
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
        "INSERT INTO tasks (source_kind, external_id, repo_id, title, body_snapshot, url, external_state, author_login, author_avatar_url, board_column, position) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'available', $10) \
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
        "INSERT INTO tasks (source_kind, external_id, repo_id, jira_board_id, title, body_snapshot, url, external_state, board_column, position) \
         VALUES ('jira', $1, $2, $3, $4, $5, $6, $7, $8, $9) \
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
pub async fn has_active_blocking_task(pool: &PgPool) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE blocking = TRUE AND board_column = 'in_progress')",
    )
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
/// the given position with a sequential, human-friendly external id.
pub async fn create_internal_task(
    pool: &PgPool,
    title: &str,
    body: &str,
    state: &str,
    initial_position: f64,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "INSERT INTO tasks \
           (source_kind, external_id, title, body_snapshot, url, external_state, board_column, position) \
         VALUES ('internal', nextval('internal_ticket_seq')::text, $1, $2, '', $3, 'available', $4) \
         RETURNING *",
    )
    .bind(title)
    .bind(body)
    .bind(state)
    .bind(initial_position)
    .fetch_one(pool)
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
         ci_fix_attempts = 0, updated_at = now() WHERE board_column = 'in_progress'",
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
pub async fn pick_next_todo(pool: &PgPool) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'todo' AND hold = FALSE \
         AND source_kind = 'github' \
         ORDER BY position ASC LIMIT 1",
    )
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
pub async fn pick_next_ci_fix(pool: &PgPool) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'ci_failing' \
         AND hold = FALSE ORDER BY position ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

/// The next PR whose auto-merge failed on a conflict and that the agent should
/// resolve: top of `In Review` flagged `merge_conflict`, not on hold.
///
/// Unlike [`pick_next_revisit`] this has no cooldown: a fresh conflict is handed
/// back to the agent promptly (and ahead of new To Do work) so a PR that just
/// fell out of mergeability is unblocked rather than left to the idle revisit.
pub async fn pick_next_merge_conflict(pool: &PgPool) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'merge_conflict' \
         AND hold = FALSE ORDER BY position ASC LIMIT 1",
    )
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

/// Resets a task's CI-fix counter (used when the agent circles back to a blocked
/// PR, so the fresh attempt gets the full retry budget again).
pub async fn reset_ci_fix_attempts(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE tasks SET ci_fix_attempts = 0, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The oldest blocked PR worth revisiting while the agent is otherwise idle: in
/// review, `ci_blocked`, not on hold, and untouched for at least `cooldown_secs`
/// (so a genuinely stuck PR is retried periodically, not in a tight loop).
pub async fn pick_next_revisit(pool: &PgPool, cooldown_secs: i64) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'ci_blocked' \
         AND hold = FALSE \
         AND (last_activity_at IS NULL OR last_activity_at < now() - ($1 * interval '1 second')) \
         ORDER BY last_activity_at ASC NULLS FIRST LIMIT 1",
    )
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

/// Every pull request tracked for a task, oldest first.
pub async fn list_task_prs(pool: &PgPool, task_id: Uuid) -> sqlx::Result<Vec<TaskPullRequest>> {
    sqlx::query_as::<_, TaskPullRequest>(
        "SELECT * FROM task_pull_requests WHERE task_id = $1 ORDER BY created_at",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
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

/// When any currently-running turn started, for the global live time ticker.
pub async fn global_running_since(pool: &PgPool) -> sqlx::Result<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT started_at FROM turns WHERE status = 'running' ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
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
    sqlx::query_as::<_, super::models::Event>(
        "SELECT e.* FROM events e JOIN turns t ON e.turn_id = t.id \
         WHERE t.task_id = $1 ORDER BY t.idx, e.seq",
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
    title: &str,
    detail: &str,
) -> sqlx::Result<EnvSuggestion> {
    sqlx::query_as::<_, EnvSuggestion>(
        "INSERT INTO environment_suggestions (task_id, title, detail) \
         VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(task_id)
    .bind(title)
    .bind(detail)
    .fetch_one(pool)
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
pub async fn pick_resume_ready(pool: &PgPool) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT t.* FROM tasks t \
         WHERE t.board_column = 'in_progress' \
           AND EXISTS (SELECT 1 FROM questions q WHERE q.task_id = t.id \
                       AND q.status <> 'pending' AND q.acknowledged = FALSE) \
           AND NOT EXISTS (SELECT 1 FROM questions q WHERE q.task_id = t.id \
                           AND q.status = 'pending') \
         ORDER BY t.position LIMIT 1",
    )
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
