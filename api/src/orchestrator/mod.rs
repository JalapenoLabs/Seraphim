//! The autonomous agent loop and the background sync/review loops.
//!
//! Three long-lived tasks run for the life of the process:
//! - **sync**: polls each enabled issue source and upserts issues into `Available`.
//! - **agent**: when idle and not paused, pulls the top of `To Do` and works it
//!   end to end through one resumable Claude Code conversation.
//! - **review**: merges `auto_squash_merge` PRs once their checks are green.
//!
//! The agent loop is inherently single-threaded: one task is awaited to
//! completion before the next is considered, so turns never overlap.

mod availability;
mod prompt;
mod provision;

pub use provision::provision_workspace;

use std::time::Duration;

use chrono::Utc;
use eyre::{eyre, Result};
use futures::StreamExt;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::claude::{run_turn, AgentEventKind, TurnArgs};
use crate::db::models::{Repository, ReviewPolicy, SourceKind, Task, TaskColumn, TaskStatus};
use crate::db::queries;
use crate::git;
use crate::secrets::Scrubber;
use crate::state::AppState;

/// How often the agent loop checks for work when idle.
const AGENT_IDLE_POLL: Duration = Duration::from_secs(5);
/// How often the review loop re-checks CI on awaiting PRs.
const REVIEW_POLL: Duration = Duration::from_secs(30);
/// Fallback issue-sync cadence when a source omits its own interval.
const DEFAULT_SYNC_POLL: Duration = Duration::from_secs(120);

/// Launches the background loops and an initial workspace provision. Returns
/// immediately.
pub fn spawn(state: AppState) {
    tokio::spawn(provision_on_startup(state.clone()));
    tokio::spawn(sync_loop(state.clone()));
    tokio::spawn(review_loop(state.clone()));
    tokio::spawn(agent_loop(state));
}

/// Best-effort full provision at boot so the workspace is ready before the first
/// task. Failures (e.g. no token yet) are logged; per-task prep retries anyway.
async fn provision_on_startup(state: AppState) {
    // Mark provisioning in-progress so the agent halts until the config repo is
    // verified this boot (only matters when a config repo is configured).
    if let Ok(settings) = queries::get_settings(&state.db).await {
        if !settings.config_repo_url.trim().is_empty() {
            let _ = queries::set_config_repo_error(
                &state.db,
                Some("workspace provisioning in progress"),
            )
            .await;
        }
    }

    match provision::provision_workspace(&state).await {
        Ok(()) => info!("workspace provisioned"),
        Err(error) => {
            error!(error = %error, "workspace provision failed (agent halted until resolved)");
        }
    }
}

// --- Sync loop ---------------------------------------------------------------

async fn sync_loop(state: AppState) {
    loop {
        if let Err(error) = sync_once(&state).await {
            warn!(error = %error, "issue sync failed");
        }
        sleep(DEFAULT_SYNC_POLL).await;
    }
}

/// Runs one full issue-sync pass across every repo flagged to sync. Also
/// callable from the HTTP layer to power the "Check issues" button.
pub async fn sync_once(state: &AppState) -> Result<()> {
    let repos = queries::list_repositories_to_sync(&state.db).await?;
    let github = state.github().await?;
    let mut changed = false;

    for repo in &repos {
        let Some((owner, name)) = repo.full_name.split_once('/') else {
            warn!(repo = %repo.full_name, "repo full name is not owner/repo");
            continue;
        };

        let issues = match git::list_open_issues(&github, owner, name, &repo.issue_labels).await {
            Ok(issues) => issues,
            Err(error) => {
                warn!(error = %error, repo = %repo.full_name, "failed to list issues");
                continue;
            }
        };

        for issue in issues {
            // New issues land at the end of Available; existing ones refresh.
            let next_position = queries::max_position_in_column(&state.db, TaskColumn::Available)
                .await?
                .unwrap_or(0.0)
                + 1.0;

            queries::upsert_issue_task(
                &state.db,
                SourceKind::Github,
                &issue.number.to_string(),
                Some(repo.id),
                &issue.title,
                &issue.body,
                &issue.url,
                next_position,
            )
            .await?;
            changed = true;
        }
    }

    if changed {
        state.notify_board();
    }
    Ok(())
}

// --- Agent loop --------------------------------------------------------------

async fn agent_loop(state: AppState) {
    loop {
        match next_actionable_task(&state).await {
            Ok(Some(task)) => {
                if let Err(error) = work_task(&state, task).await {
                    error!(error = %error, "task run failed");
                }
                // Immediately look for the next card; only sleep when idle.
            }
            Ok(None) => sleep(AGENT_IDLE_POLL).await,
            Err(error) => {
                warn!(error = %error, "agent loop poll failed");
                sleep(AGENT_IDLE_POLL).await;
            }
        }
    }
}

/// The next card to work, or `None` if paused, halted, outside the availability
/// schedule, or the queue is empty.
async fn next_actionable_task(state: &AppState) -> Result<Option<Task>> {
    let settings = queries::get_settings(&state.db).await?;
    if settings.agent_paused {
        return Ok(None);
    }
    // Hard halt: a configured config repo that failed to set up means the agent
    // is missing its instructions/skills. Refuse to pull work until it's fixed.
    // Bypassed only when no config repo is configured (blank url).
    if !settings.config_repo_url.trim().is_empty() && settings.config_repo_error.is_some() {
        return Ok(None);
    }
    // Optional availability schedule (hours/days/skip-dates in the user's zone).
    if !availability::is_available(&settings, Utc::now()) {
        return Ok(None);
    }
    queries::pick_next_todo(&state.db).await.map_err(Into::into)
}

/// Runs one task end to end: prepare repo, drive Claude, detect PR, apply policy.
async fn work_task(state: &AppState, task: Task) -> Result<()> {
    info!(task_id = %task.id, title = %task.title, "starting task");

    // Move the card into In Progress and mark it preparing.
    queries::move_task(&state.db, task.id, TaskColumn::InProgress, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::Preparing).await?;
    state.notify_board();

    let Some(repo_id) = task.repo_id else {
        return fail(state, &task, "no repository is configured for this issue").await;
    };
    let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
        return fail(state, &task, "the linked repository no longer exists").await;
    };

    let settings = queries::get_settings(&state.db).await?;
    let branch = render_branch(&repo.branch_template, &task);
    if let Err(error) = provision::prepare_branch(state, &settings, &repo, &branch).await {
        return fail(state, &task, &format!("repo preparation failed: {error}")).await;
    }

    queries::mark_task_started(
        &state.db,
        task.id,
        &branch,
        settings.current_session_id.as_deref(),
    )
    .await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::Working).await?;
    state.notify_board();

    // Drive the Claude turn and capture the (possibly new) session id.
    let outcome = run_agent_turn(state, &settings, &repo, &task, &branch).await?;
    if let Some(session_id) = &outcome.session_id {
        if settings.current_session_id.as_deref() != Some(session_id.as_str()) {
            queries::set_current_session_id(&state.db, Some(session_id)).await?;
        }
    }
    // Surface a turn failure (e.g. "Not logged in") on the task itself, instead
    // of letting it fall through to the generic "no pull request" message.
    if let Some(message) = outcome.error {
        return fail(state, &task, &message).await;
    }

    // Deterministically detect the PR the agent opened for this branch.
    let github = state.github().await?;
    let (owner, repo_name) = split_full_name(&repo.full_name)?;
    let pull = git::find_open_pr_for_branch(&github, owner, repo_name, &branch).await?;
    let Some(pull) = pull else {
        return fail(
            state,
            &task,
            "the agent finished without opening a pull request",
        )
        .await;
    };

    queries::set_task_pr(&state.db, task.id, &pull.html_url).await?;
    queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
    state.notify_board();

    info!(task_id = %task.id, pr = %pull.html_url, "task moved to review");
    Ok(())
}

/// The outcome of one Claude turn.
struct TurnOutcome {
    /// Session id reported by the turn (the shared, resumable conversation).
    session_id: Option<String>,
    /// A failure message to surface on the task, if the turn errored.
    error: Option<String>,
}

/// Streams one Claude turn, persisting every event and pushing it to the UI.
async fn run_agent_turn(
    state: &AppState,
    settings: &crate::db::models::Settings,
    repo: &Repository,
    task: &Task,
    branch: &str,
) -> Result<TurnOutcome> {
    let prompt = prompt::build(settings, repo, task, branch);
    // Claude runs at the workspace root so it can work across all cloned repos.
    let working_dir = "/workspace".to_string();

    let idx = queries::next_turn_idx(&state.db, task.id).await?;
    let turn = queries::create_turn(
        &state.db,
        task.id,
        idx,
        &prompt,
        settings.current_session_id.as_deref(),
    )
    .await?;

    // User-defined environment variables are injected into the agent's exec.
    let env = queries::list_environment_variables(&state.db)
        .await?
        .into_iter()
        .map(|variable| (variable.key, variable.value))
        .collect();
    // Every secret (env vars + tokens) is scrubbed from output before it is
    // persisted or streamed, so a secret the agent echoes never leaks.
    let scrubber = Scrubber::new(queries::list_secret_values(&state.db).await?);

    let args = TurnArgs {
        container: state.workspace.container().to_string(),
        working_dir,
        prompt,
        resume_session_id: settings.current_session_id.clone(),
        model: settings.claude_model.clone(),
        oauth_token: queries::get_claude_token(&state.db).await?,
        github_token: queries::get_github_token(&state.db).await?,
        env,
    };

    let mut stream = Box::pin(run_turn(state.workspace.docker(), args));
    let mut seq = 0_i32;
    let mut session_id = settings.current_session_id.clone();
    let mut result_text: Option<String> = None;
    let mut total_cost: Option<f64> = None;
    let mut error_message: Option<String> = None;

    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(event) => event,
            Err(error) => {
                warn!(error = %error, "claude stream error");
                error_message = Some(format!("Claude stream error: {error}"));
                break;
            }
        };

        if let Some(found) = &event.session_id {
            session_id = Some(found.clone());
        }
        if let AgentEventKind::Result {
            total_cost_usd,
            result_text: text,
            is_error,
        } = &event.kind
        {
            total_cost = *total_cost_usd;
            result_text = text.as_deref().map(|text| scrubber.scrub_text(text));
            if *is_error {
                let message = text
                    .clone()
                    .unwrap_or_else(|| "the agent reported an error".to_string());
                error_message = Some(scrubber.scrub_text(&message));
            }
        }

        let label = event.type_label();
        // Scrub secrets out of the payload before it touches the DB or the stream.
        let mut payload = event.raw.clone();
        scrubber.scrub_value(&mut payload);
        queries::append_event(&state.db, turn.id, seq, label, payload.clone()).await?;
        state.notify_task(
            task.id,
            serde_json::json!({ "type": label, "payload": payload }),
        );
        queries::set_task_status(&state.db, task.id, TaskStatus::Working)
            .await
            .ok();
        seq += 1;
    }

    let status = if error_message.is_some() {
        "failed"
    } else {
        "completed"
    };
    queries::finish_turn(
        &state.db,
        turn.id,
        status,
        result_text.as_deref(),
        total_cost,
        session_id.as_deref(),
    )
    .await?;

    Ok(TurnOutcome {
        session_id,
        error: error_message,
    })
}

// --- Review loop -------------------------------------------------------------

async fn review_loop(state: AppState) {
    loop {
        if let Err(error) = review_once(&state).await {
            warn!(error = %error, "review loop failed");
        }
        sleep(REVIEW_POLL).await;
    }
}

async fn review_once(state: &AppState) -> Result<()> {
    let settings = queries::get_settings(&state.db).await?;
    let candidates = queries::list_review_candidates(&state.db).await?;
    if candidates.is_empty() {
        return Ok(());
    }
    let github = state.github().await?;

    for task in candidates {
        let Some(repo_id) = task.repo_id else {
            continue;
        };
        let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
            continue;
        };

        let policy = repo.review_policy.unwrap_or(settings.default_review_policy);
        if policy != ReviewPolicy::AutoSquashMerge {
            continue; // Human-reviewed repos wait for a person.
        }

        let Some(branch) = &task.branch else { continue };
        let (owner, repo_name) = split_full_name(&repo.full_name)?;
        let Some(pull) = git::find_open_pr_for_branch(&github, owner, repo_name, branch).await?
        else {
            continue;
        };

        if !git::checks_green(&github, owner, repo_name, &pull.head_sha).await? {
            continue; // CI still running or red; try again next tick.
        }

        queries::set_task_status(&state.db, task.id, TaskStatus::Merging).await?;
        state.notify_board();

        git::squash_merge(&github, owner, repo_name, pull.number).await?;
        queries::finish_task(&state.db, task.id, TaskColumn::Done, TaskStatus::Done).await?;
        state.notify_board();
        info!(task_id = %task.id, "auto-merged and marked done");
    }

    Ok(())
}

// --- Helpers -----------------------------------------------------------------

/// Records a task failure: captures the message and surfaces it in `In Review`.
async fn fail(state: &AppState, task: &Task, message: &str) -> Result<()> {
    warn!(task_id = %task.id, message, "task failed");
    // Keep card-level errors readable; full detail lives in the event stream.
    let trimmed: String = message.trim().chars().take(800).collect();
    queries::set_task_error(&state.db, task.id, &trimmed).await?;
    queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
    state.notify_board();
    Ok(())
}

/// Splits `owner/repo` into its parts.
fn split_full_name(full_name: &str) -> Result<(&str, &str)> {
    full_name
        .split_once('/')
        .ok_or_else(|| eyre!("repository full name '{full_name}' is not owner/repo"))
}

/// Renders a branch template, substituting `{number}` and `{slug}`.
fn render_branch(template: &str, task: &Task) -> String {
    template
        .replace("{number}", &task.external_id)
        .replace("{slug}", &slugify(&task.title))
}

/// A filesystem/git-safe slug: lowercase alphanumerics joined by single dashes.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for character in title.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
        // Keep branch names tidy.
        if slug.len() >= 40 {
            break;
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_is_branch_safe() {
        assert_eq!(slugify("Fix the Login Bug!"), "fix-the-login-bug");
        assert_eq!(slugify("   spaces   "), "spaces");
    }

    #[test]
    fn render_branch_substitutes_placeholders() {
        let mut task = sample_task();
        task.external_id = "42".to_string();
        task.title = "Add Dark Mode".to_string();
        assert_eq!(
            render_branch("seraphim/issue-{number}-{slug}", &task),
            "seraphim/issue-42-add-dark-mode"
        );
    }

    fn sample_task() -> Task {
        Task {
            id: uuid::Uuid::nil(),
            source_kind: crate::db::models::SourceKind::Github,
            external_id: String::new(),
            repo_id: None,
            title: String::new(),
            body_snapshot: String::new(),
            url: String::new(),
            board_column: TaskColumn::Todo,
            position: 0.0,
            status: TaskStatus::Queued,
            branch: None,
            pr_url: None,
            error: None,
            hold: false,
            session_id: None,
            started_at: None,
            finished_at: None,
            last_activity_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}
