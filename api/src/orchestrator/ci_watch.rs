//! Mirrors a watched pull request's GitHub Actions progress into the task's
//! activity log, so the operator sees CI's per-step progress alongside the
//! agent's own events (issue #185).
//!
//! For every open PR a task tracks, the loop polls the commit's workflow runs,
//! then each run's jobs and steps, and emits one activity event per transition:
//!
//! - `step_running`: a step started (live only, never persisted, so the history isn't cluttered).
//! - `step_passed`: a step succeeded (persisted: "CI: <step> complete").
//! - `step_failed`: a step failed (persisted, with the tail of the job log so the error is visible inline).
//! - `job_passed`: a whole job succeeded (persisted: "CI: All <job> passed").
//!
//! These flow through the same `events` table + task SSE stream as agent events,
//! so both the per-task view and the global watch view render them with no
//! special transport. Persisted events carry a stable `key` so a process restart
//! reseeds from the database and never re-writes a line it already recorded.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use eyre::Result;
use serde_json::json;
use tokio::time::sleep;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::db::models::TaskPullRequest;
use crate::db::queries;
use crate::git;
use crate::state::AppState;

/// How often the watcher polls GitHub Actions for the open PRs it tracks. Faster
/// than the 30s review loop so a step reads as live, but coarse enough that a
/// handful of open PRs stay well within the API budget.
const CI_POLL: Duration = Duration::from_secs(12);

/// Lines of a failed step's job log to surface inline in the activity log.
const FAIL_LOG_LINES: usize = 25;

/// The phase we last emitted for a step (or job), so each transition is emitted
/// exactly once. `Running` is live-only; `Done` is persisted to history.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    Done,
}

/// This process's watch memory for one task: the phase last emitted per step/job
/// key, and whether the already-persisted keys have been seeded from the DB yet
/// (so a restart never re-writes history that is already there).
#[derive(Default)]
struct TaskWatch {
    seeded: bool,
    phases: HashMap<String, Phase>,
}

/// Background loop: poll the open PRs' CI and emit step activity. Never exits.
pub async fn ci_watch_loop(state: AppState) {
    let mut watch: HashMap<Uuid, TaskWatch> = HashMap::new();
    loop {
        if let Err(error) = watch_once(&state, &mut watch).await {
            warn!(error = %error, "CI activity watch failed");
        }
        sleep(CI_POLL).await;
    }
}

async fn watch_once(state: &AppState, watch: &mut HashMap<Uuid, TaskWatch>) -> Result<()> {
    // No GitHub auth configured means nothing to watch (and unauthenticated
    // Actions calls would just 401); skip quietly.
    let token = queries::get_github_token(&state.db).await?;
    if token.is_empty() {
        return Ok(());
    }

    let prs = queries::list_open_task_prs(&state.db).await?;
    // Forget watch memory for tasks whose PRs have all settled, so it can't grow
    // without bound over the process's lifetime.
    let open_task_ids: HashSet<Uuid> = prs.iter().map(|pr| pr.task_id).collect();
    watch.retain(|task_id, _| open_task_ids.contains(task_id));
    if prs.is_empty() {
        return Ok(());
    }

    let github = state.github().await?;
    for pr in prs {
        let Some((owner, name)) = pr.repo_full_name.split_once('/') else {
            continue;
        };
        if pr.head_sha.is_empty() {
            continue;
        }

        let entry = watch.entry(pr.task_id).or_default();
        if !entry.seeded {
            if let Err(error) = seed_seen(state, pr.task_id, entry).await {
                debug!(error = %error, task_id = %pr.task_id, "failed to seed CI watch state");
            } else {
                entry.seeded = true;
            }
        }

        if let Err(error) = watch_pr(state, &github, &token, &pr, owner, name, entry).await {
            debug!(error = %error, task_id = %pr.task_id, repo = %pr.repo_full_name, "CI watch for PR failed");
        }
    }
    Ok(())
}

/// Seeds a task's already-recorded CI keys from the database so a restart resumes
/// where it left off instead of re-writing the same lines.
async fn seed_seen(state: &AppState, task_id: Uuid, watch: &mut TaskWatch) -> Result<()> {
    for event in queries::list_events_for_task(&state.db, task_id).await? {
        if event.event_type != "ci" {
            continue;
        }
        if let Some(key) = event.payload.0.get("key").and_then(|value| value.as_str()) {
            watch.phases.insert(key.to_string(), Phase::Done);
        }
    }
    Ok(())
}

async fn watch_pr(
    state: &AppState,
    github: &octocrab::Octocrab,
    token: &str,
    pr: &TaskPullRequest,
    owner: &str,
    name: &str,
    watch: &mut TaskWatch,
) -> Result<()> {
    for run in git::list_runs_for_sha(github, owner, name, &pr.head_sha).await? {
        for job in git::list_run_jobs(github, owner, name, run.id).await? {
            for step in &job.steps {
                let key = format!(
                    "{}#{}#{}#{}",
                    pr.repo_full_name, run.id, job.id, step.number
                );
                process_step(state, token, pr, owner, name, &job, step, &key, watch).await?;
            }

            // A whole job finishing green gets a summary line, mirroring the
            // issue's "All <job> has passed".
            if job.status == "completed" && job.conclusion.as_deref() == Some("success") {
                let key = format!("{}#{}#{}#job", pr.repo_full_name, run.id, job.id);
                if watch.phases.get(&key) != Some(&Phase::Done) {
                    emit_persisted(
                        state,
                        pr.task_id,
                        "job_passed",
                        format!("CI: All {} passed", job.name),
                        None,
                        &key,
                    )
                    .await?;
                    watch.phases.insert(key, Phase::Done);
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_step(
    state: &AppState,
    token: &str,
    pr: &TaskPullRequest,
    owner: &str,
    name: &str,
    job: &git::WorkflowJob,
    step: &git::WorkflowStep,
    key: &str,
    watch: &mut TaskWatch,
) -> Result<()> {
    match step.status.as_str() {
        // First time we see a step running, announce it live (not persisted).
        "in_progress" => {
            if !watch.phases.contains_key(key) {
                emit_live(
                    state,
                    pr.task_id,
                    "step_running",
                    &format!("CI: Running {}...", step.name),
                    key,
                );
                watch.phases.insert(key.to_string(), Phase::Running);
            }
        }
        "completed" => {
            if watch.phases.get(key) == Some(&Phase::Done) {
                return Ok(());
            }
            match step.conclusion.as_deref() {
                Some("success") => {
                    emit_persisted(
                        state,
                        pr.task_id,
                        "step_passed",
                        format!("CI: {} complete", step.name),
                        None,
                        key,
                    )
                    .await?;
                }
                // Skipped steps (and the odd null conclusion) aren't worth a line.
                Some("skipped") | None => {}
                Some(_failed) => {
                    let log =
                        git::fetch_job_log_tail(token, owner, name, job.id, FAIL_LOG_LINES).await;
                    emit_persisted(
                        state,
                        pr.task_id,
                        "step_failed",
                        format!("CI: {} failed", step.name),
                        log,
                        key,
                    )
                    .await?;
                }
            }
            watch.phases.insert(key.to_string(), Phase::Done);
        }
        // queued / unknown: nothing to show yet.
        _ => {}
    }
    Ok(())
}

/// Pushes a live CI event onto the task's stream without persisting it, so an
/// in-progress step shows immediately but doesn't clutter the saved history.
fn emit_live(state: &AppState, task_id: Uuid, status: &str, text: &str, key: &str) {
    let payload = json!({ "status": status, "text": text, "key": key });
    state.notify_task(
        task_id,
        json!({ "type": "ci", "payload": payload, "created_at": Utc::now() }),
    );
}

/// Persists a CI event to the task's activity history and streams it live. An
/// optional `log` (a failed step's log tail) rides along, with `has_ansi` set so
/// the UI knows whether to honor embedded ANSI color.
async fn emit_persisted(
    state: &AppState,
    task_id: Uuid,
    status: &str,
    text: String,
    log: Option<String>,
    key: &str,
) -> Result<()> {
    let turn = queries::get_or_create_ci_turn(&state.db, task_id).await?;
    let seq = queries::next_event_seq(&state.db, turn.id).await?;

    let mut payload = json!({ "status": status, "text": text, "key": key });
    if let Some(log) = log {
        payload["has_ansi"] = json!(log.contains('\u{1b}'));
        payload["log"] = json!(log);
    }

    queries::append_event(&state.db, turn.id, seq, "ci", payload.clone()).await?;
    state.notify_task(
        task_id,
        json!({ "type": "ci", "payload": payload, "created_at": Utc::now() }),
    );
    Ok(())
}
