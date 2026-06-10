//! Typed queries over the Seraphim schema.
//!
//! Functions take a `&PgPool` and return [`sqlx::Result`]; callers lift errors
//! into the application's `eyre` result with `?`.

use chrono::NaiveDate;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    AvailabilityWindow, Repository, ReviewPolicy, Settings, SourceKind, Task, TaskColumn,
    TaskStatus, Turn,
};

// --- Settings ----------------------------------------------------------------

/// The settings fields exposed to the app, for SELECT/RETURNING reuse. The raw
/// token columns are deliberately excluded; only "is it set" booleans are
/// surfaced so secrets never leave the database via the API.
const SETTINGS_COLUMNS: &str =
    "org_name, global_instructions, default_review_policy, agent_paused, \
     claude_model, workspace_image_tag, base_setup_script, config_repo_url, \
     default_branch_template, current_session_id, updated_at, \
     (claude_oauth_token <> '') AS claude_token_set, \
     (github_token <> '') AS github_token_set, \
     availability_enabled, availability_timezone, availability_windows, \
     availability_skip_dates";

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

pub async fn set_current_session_id(pool: &PgPool, session_id: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE settings SET current_session_id = $1, updated_at = now() WHERE id = 1")
        .bind(session_id)
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

/// Writes the app tokens; `None` leaves the existing value untouched (so the UI
/// can update one without resending the other).
pub async fn set_tokens(
    pool: &PgPool,
    claude_oauth_token: Option<String>,
    github_token: Option<String>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE settings SET \
         claude_oauth_token = COALESCE($1, claude_oauth_token), \
         github_token = COALESCE($2, github_token), \
         updated_at = now() WHERE id = 1",
    )
    .bind(claude_oauth_token)
    .bind(github_token)
    .execute(pool)
    .await?;
    Ok(())
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
    branch_template: &str,
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

/// Like [`upsert_repository`] but only creates a row when one doesn't already
/// exist (used by org import so it never clobbers your manual edits). Returns the
/// existing or newly-created repo.
#[allow(clippy::too_many_arguments)]
pub async fn create_repository_if_absent(
    pool: &PgPool,
    full_name: &str,
    clone_url: &str,
    default_branch: &str,
    branch_template: &str,
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

pub async fn delete_repository(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM repositories WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- Tasks -------------------------------------------------------------------

pub async fn list_tasks(pool: &PgPool) -> sqlx::Result<Vec<Task>> {
    sqlx::query_as::<_, Task>("SELECT * FROM tasks ORDER BY board_column, position")
        .fetch_all(pool)
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
pub async fn upsert_issue_task(
    pool: &PgPool,
    source_kind: SourceKind,
    external_id: &str,
    repo_id: Option<Uuid>,
    title: &str,
    body: &str,
    url: &str,
    initial_position: f64,
) -> sqlx::Result<Task> {
    sqlx::query_as::<_, Task>(
        "INSERT INTO tasks (source_kind, external_id, repo_id, title, body_snapshot, url, board_column, position) \
         VALUES ($1, $2, $3, $4, $5, $6, 'available', $7) \
         ON CONFLICT (repo_id, source_kind, external_id) DO UPDATE SET \
         title = EXCLUDED.title, \
         body_snapshot = EXCLUDED.body_snapshot, \
         url = EXCLUDED.url, \
         updated_at = now() \
         RETURNING *",
    )
    .bind(source_kind)
    .bind(external_id)
    .bind(repo_id)
    .bind(title)
    .bind(body)
    .bind(url)
    .bind(initial_position)
    .fetch_one(pool)
    .await
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

pub async fn move_task(
    pool: &PgPool,
    id: Uuid,
    column: TaskColumn,
    position: f64,
) -> sqlx::Result<Task> {
    // Re-queuing a card (into To Do or Available) clears any prior failure so it
    // starts clean, including after a failed run.
    sqlx::query_as::<_, Task>(
        "UPDATE tasks SET board_column = $2, position = $3, \
         status = CASE WHEN $2 IN ('todo'::task_column, 'available'::task_column) \
                       THEN 'queued'::task_status ELSE status END, \
         error = CASE WHEN $2 IN ('todo'::task_column, 'available'::task_column) \
                      THEN NULL ELSE error END, \
         updated_at = now() \
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(column)
    .bind(position)
    .fetch_one(pool)
    .await
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

/// The next card the agent should work: top of `To Do`, not on hold.
pub async fn pick_next_todo(pool: &PgPool) -> sqlx::Result<Option<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'todo' AND hold = FALSE \
         ORDER BY position ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

/// Tasks sitting in review awaiting an automated merge decision.
pub async fn list_review_candidates(pool: &PgPool) -> sqlx::Result<Vec<Task>> {
    sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE board_column = 'in_review' AND status = 'awaiting_review' \
         ORDER BY position",
    )
    .fetch_all(pool)
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
    session_id: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE turns SET status = $2, result_text = $3, total_cost_usd = $4, \
         session_id = COALESCE($5, session_id), finished_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(result_text)
    .bind(total_cost_usd)
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Events ------------------------------------------------------------------

pub async fn append_event(
    pool: &PgPool,
    turn_id: Uuid,
    seq: i32,
    event_type: &str,
    payload: Value,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO events (turn_id, seq, type, payload) VALUES ($1, $2, $3, $4)")
        .bind(turn_id)
        .bind(seq)
        .bind(event_type)
        .bind(Json(payload))
        .execute(pool)
        .await?;
    Ok(())
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
