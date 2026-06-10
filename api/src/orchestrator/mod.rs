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
use crate::db::models::{ReviewPolicy, SourceKind, Task, TaskColumn, TaskStatus};
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

/// How many times we re-check for the agent's freshly opened PR before giving
/// up. GitHub's pull-request list lags a few seconds behind `gh pr create`
/// (read-replica/index propagation), so a single check the instant the turn
/// ends races GitHub's own indexing and spuriously reports "no PR".
const PR_DETECT_ATTEMPTS: u32 = 6;
/// Delay between PR-detection attempts (so detection waits up to ~15s total).
const PR_DETECT_DELAY: Duration = Duration::from_secs(3);

/// How many fix turns the agent spends on a PR's failing CI before leaving it
/// for a human. Bounds thrash when a failure is unfixable or out of scope.
const MAX_CI_FIX_ATTEMPTS: i32 = 3;

/// How long a blocked PR rests before the idle agent circles back to retry it,
/// so a genuinely stuck PR is revisited periodically rather than in a tight loop.
const REVISIT_COOLDOWN: Duration = Duration::from_secs(15 * 60);

/// What kind of work a pulled card needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkMode {
    /// A fresh issue: cut a branch, implement, and open a PR.
    Fresh,
    /// A parked task the user just answered: resume the existing session.
    Resume,
    /// An open PR with failing CI: re-engage on its branch to fix the checks.
    FixCi,
    /// A PR the agent gave up on (CI or merge conflict), retried while idle.
    Revisit,
}

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
    // A turn in flight when the process stopped left its card stuck in In
    // Progress. Return any such card to To Do so the agent reworks it cleanly
    // rather than stranding it.
    match queries::reclaim_orphaned_tasks(&state.db).await {
        Ok(0) => {}
        Ok(count) => warn!(count, "reclaimed tasks stranded in progress by a restart"),
        Err(error) => warn!(error = %error, "failed to reclaim in-progress tasks on startup"),
    }

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
            // Only a config-repo failure halts the agent (tracked separately in
            // settings.config_repo_error). A later step failing here (e.g. a
            // repo's setup script) leaves the agent running on a partially
            // provisioned workspace; per-task prep retries the focus repo.
            error!(error = %error, "workspace provision failed");
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
            Ok(Some((task, mode))) => {
                if let Err(error) = work_task(&state, task, mode).await {
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

/// The next card to work and how, or `None` if paused, halted, outside the
/// availability schedule, or idle.
///
/// Greening an already-open PR takes priority over starting fresh work, so PRs
/// don't linger red while the agent moves on to new issues.
async fn next_actionable_task(state: &AppState) -> Result<Option<(Task, WorkMode)>> {
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
    if let Some(task) = queries::pick_resume_ready(&state.db).await? {
        return Ok(Some((task, WorkMode::Resume)));
    }
    if let Some(task) = queries::pick_next_ci_fix(&state.db).await? {
        return Ok(Some((task, WorkMode::FixCi)));
    }
    if let Some(task) = queries::pick_next_todo(&state.db).await? {
        return Ok(Some((task, WorkMode::Fresh)));
    }
    // Idle: circle back to a PR we gave up on and try once more (cooldown-gated).
    if let Some(task) =
        queries::pick_next_revisit(&state.db, REVISIT_COOLDOWN.as_secs() as i64).await?
    {
        return Ok(Some((task, WorkMode::Revisit)));
    }
    Ok(None)
}

/// Dispatches a pulled card to the right end-to-end flow.
async fn work_task(state: &AppState, task: Task, mode: WorkMode) -> Result<()> {
    match mode {
        WorkMode::Fresh => work_fresh(state, task, false).await,
        WorkMode::Resume => work_fresh(state, task, true).await,
        WorkMode::FixCi => work_ci_fix(state, task, false).await,
        WorkMode::Revisit => work_ci_fix(state, task, true).await,
    }
}

/// Runs a fresh issue end to end: prepare repo, drive Claude, detect PR.
async fn work_fresh(state: &AppState, task: Task, resume: bool) -> Result<()> {
    info!(task_id = %task.id, title = %task.title, resume, "working task");

    let Some(repo_id) = task.repo_id else {
        return fail(state, &task, "no repository is configured for this issue").await;
    };
    let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
        return fail(state, &task, "the linked repository no longer exists").await;
    };

    let settings = queries::get_settings(&state.db).await?;

    // A resumed task already has its branch and working tree; only a fresh task
    // is moved into In Progress and re-cut from the default branch.
    let branch = if resume {
        task.branch
            .clone()
            .unwrap_or_else(|| render_branch(&repo.branch_template, &task))
    } else {
        queries::move_task(&state.db, task.id, TaskColumn::InProgress, task.position).await?;
        queries::set_task_status(&state.db, task.id, TaskStatus::Preparing).await?;
        state.notify_board();

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
        branch
    };

    queries::set_task_status(&state.db, task.id, TaskStatus::Working).await?;
    state.notify_board();

    // On a fresh run the prompt is the task brief; on resume it delivers the
    // user's answers to the question(s) the agent asked.
    let prompt = if resume {
        let answers = queries::list_unacknowledged_answers(&state.db, task.id).await?;
        let prompt = prompt::build_resume(&repo, &task, &branch, &answers);
        queries::acknowledge_answers(&state.db, task.id).await?;
        prompt
    } else {
        prompt::build(&settings, &repo, &task, &branch)
    };
    let outcome = run_agent_turn(state, &settings, &task, prompt).await?;
    persist_session(state, &settings, &outcome).await?;
    // Surface a turn failure (e.g. "Not logged in") on the task itself, instead
    // of letting it fall through to the generic "no pull request" message.
    if let Some(message) = outcome.error {
        return fail(state, &task, &message).await;
    }

    // If the agent asked the user something, park the task until it is answered
    // rather than treating the missing PR as a failure.
    if queries::count_pending_questions(&state.db, task.id).await? > 0 {
        queries::set_task_status(&state.db, task.id, TaskStatus::WaitingForInput).await?;
        state.notify_board();
        info!(task_id = %task.id, "task parked awaiting the user's answer");
        return Ok(());
    }

    // Deterministically detect the PR the agent opened for this branch. Retry to
    // absorb GitHub's brief indexing lag after `gh pr create`.
    let github = state.github().await?;
    let (owner, repo_name) = split_full_name(&repo.full_name)?;
    let Some(pull) = detect_pr(&github, owner, repo_name, &branch).await? else {
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

/// Re-engages the agent on an open PR that's failing CI (`revisit = false`) or
/// one it had given up on and is being retried while idle (`revisit = true`).
///
/// Checks out the PR's existing branch, runs one turn, and decides what happens
/// next by whether the agent pushed: a new commit returns the task to review for
/// a re-check; no new commit means the agent judged it out of scope, so the PR
/// is left for a human and the agent moves on. A revisit also resets the CI-fix
/// counter so the renewed effort gets the full retry budget, and its prompt
/// names merge conflicts (the usual reason auto-merge blocked) as a likely cause.
async fn work_ci_fix(state: &AppState, task: Task, revisit: bool) -> Result<()> {
    info!(task_id = %task.id, attempts = task.ci_fix_attempts, revisit, "fixing pull request");

    let Some(repo_id) = task.repo_id else {
        return block(state, &task, "no repository is configured for this issue").await;
    };
    let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
        return block(state, &task, "the linked repository no longer exists").await;
    };
    let Some(branch) = task.branch.clone() else {
        return block(state, &task, "the task has no branch to fix").await;
    };

    let settings = queries::get_settings(&state.db).await?;

    // A revisit is a fresh effort: clear the exhausted counter so the renewed
    // fix cycle gets the full retry budget again.
    if revisit {
        queries::reset_ci_fix_attempts(&state.db, task.id).await?;
    }

    // While the turn runs the card sits in In Progress, like any actively-worked
    // task, then returns to In Review when it settles below.
    queries::move_task(&state.db, task.id, TaskColumn::InProgress, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::Working).await?;
    state.notify_board();

    if let Err(error) = provision::prepare_existing_branch(state, &settings, &repo, &branch).await {
        return fail(
            state,
            &task,
            &format!("could not check out the PR branch: {error}"),
        )
        .await;
    }

    let github = state.github().await?;
    let (owner, repo_name) = split_full_name(&repo.full_name)?;

    // Snapshot the branch tip so we can later tell whether the agent pushed.
    let before_sha = git::branch_head_sha(&github, owner, repo_name, &branch)
        .await
        .ok();

    // Enumerate the failing checks (best-effort) to focus the agent.
    let failing = match git::find_open_pr_for_branch(&github, owner, repo_name, &branch).await? {
        Some(pull) => match git::ci_status(&github, owner, repo_name, &pull.head_sha).await {
            Ok(git::CiStatus::Failing(checks)) => checks,
            _ => Vec::new(),
        },
        None => Vec::new(),
    };

    let prompt = if revisit {
        prompt::build_revisit(
            &settings,
            &repo,
            &task,
            &branch,
            task.error.as_deref().unwrap_or_default(),
        )
    } else {
        prompt::build_ci_fix(&settings, &repo, &task, &branch, &failing)
    };
    let attempt = queries::bump_ci_fix_attempt(&state.db, task.id).await?;
    let outcome = run_agent_turn(state, &settings, &task, prompt).await?;
    persist_session(state, &settings, &outcome).await?;
    if let Some(message) = outcome.error {
        return fail(state, &task, &message).await;
    }

    // A pushed commit moves the tip; nothing pushed means the agent chose not to
    // act (e.g. the failure is pre-existing or out of scope).
    let after_sha = git::branch_head_sha(&github, owner, repo_name, &branch)
        .await
        .ok();
    let pushed = match (&before_sha, &after_sha) {
        (Some(before), Some(after)) => before != after,
        // If a tip can't be read, assume progress and let the review loop judge.
        _ => true,
    };

    if !pushed {
        return block(
            state,
            &task,
            "The agent made no changes for the failing CI (likely pre-existing or out of scope). \
             Left for human review.",
        )
        .await;
    }

    // Fix pushed: back to review so the loop re-checks CI on the new commit.
    queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
    state.notify_board();
    info!(task_id = %task.id, attempt, "pushed CI fix; awaiting re-check");
    Ok(())
}

/// Persists a turn's session id when it differs from the stored one.
async fn persist_session(
    state: &AppState,
    settings: &crate::db::models::Settings,
    outcome: &TurnOutcome,
) -> Result<()> {
    if let Some(session_id) = &outcome.session_id {
        if settings.current_session_id.as_deref() != Some(session_id.as_str()) {
            queries::set_current_session_id(&state.db, Some(session_id)).await?;
        }
    }
    Ok(())
}

/// The outcome of one Claude turn.
struct TurnOutcome {
    /// Session id reported by the turn (the shared, resumable conversation).
    session_id: Option<String>,
    /// A failure message to surface on the task, if the turn errored.
    error: Option<String>,
}

/// Streams one Claude turn for `prompt`, persisting every event and pushing it
/// to the UI. The caller composes the prompt (fresh work or a CI fix).
async fn run_agent_turn(
    state: &AppState,
    settings: &crate::db::models::Settings,
    task: &Task,
    prompt: String,
) -> Result<TurnOutcome> {
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
        task_id: task.id.to_string(),
        internal_api_url: state.internal_api_url.clone(),
        env,
    };

    // Clear any claude process leaked by a previously aborted turn. The agent is
    // single-threaded, so none should legitimately be running; a leftover would
    // otherwise contend on the same shared session. The `[c]` keeps pkill from
    // matching its own command line. Best-effort.
    let _ = state
        .workspace
        .exec_capture(
            "/workspace",
            vec![
                "bash".to_string(),
                "-lc".to_string(),
                "pkill -9 -f '[c]laude -p' || true".to_string(),
            ],
            vec![],
        )
        .await;

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
        let Some(branch) = &task.branch else { continue };
        let (owner, repo_name) = split_full_name(&repo.full_name)?;
        let Some(pull) = git::find_open_pr_for_branch(&github, owner, repo_name, branch).await?
        else {
            continue; // PR closed or merged externally; nothing to do.
        };

        match git::ci_status(&github, owner, repo_name, &pull.head_sha).await? {
            // Checks still running: re-check next tick.
            git::CiStatus::Pending => {}

            // Red CI: hand it to the agent to fix, or give up once the cap hits.
            // This applies regardless of review policy, so PRs are greened before
            // a human ever looks at them.
            git::CiStatus::Failing(checks) => {
                if task.ci_fix_attempts < MAX_CI_FIX_ATTEMPTS {
                    queries::set_task_status(&state.db, task.id, TaskStatus::CiFailing).await?;
                } else {
                    let note = format!(
                        "CI still failing after {MAX_CI_FIX_ATTEMPTS} fix attempts ({checks}); \
                         needs a human.",
                        checks = checks.join(", "),
                    );
                    queries::block_task_ci(&state.db, task.id, &note).await?;
                }
                state.notify_board();
            }

            // Green CI: auto-merge repos merge now; others wait for a human.
            git::CiStatus::Passing => {
                let policy = repo.review_policy.unwrap_or(settings.default_review_policy);
                if policy != ReviewPolicy::AutoSquashMerge {
                    continue;
                }
                queries::set_task_status(&state.db, task.id, TaskStatus::Merging).await?;
                state.notify_board();

                match git::squash_merge(&github, owner, repo_name, pull.number).await {
                    Ok(()) => {
                        queries::finish_task(
                            &state.db,
                            task.id,
                            TaskColumn::Done,
                            TaskStatus::Done,
                        )
                        .await?;
                        state.notify_board();
                        info!(task_id = %task.id, "auto-merged and marked done");
                    }
                    // A merge can fail for reasons retrying won't fix (conflicts
                    // with the base, restricted merge settings). Record it and
                    // stop auto-retrying so the loop doesn't spin; a human takes
                    // over from the In Review lane.
                    Err(error) => {
                        let note = format!(
                            "Auto-merge failed: {error}. The PR likely conflicts with its base \
                             branch or merging is restricted; resolve it manually.",
                        );
                        block(state, &task, &note).await?;
                    }
                }
            }
        }
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

/// Leaves an open PR in review for a human, recording why the agent stopped on
/// CI. Unlike [`fail`], the card keeps its `In Review` lane and PR; only the
/// status and the note change.
async fn block(state: &AppState, task: &Task, message: &str) -> Result<()> {
    warn!(task_id = %task.id, message, "task CI-blocked");
    let trimmed: String = message.trim().chars().take(800).collect();
    // The card may have been in In Progress while the turn ran; settle it back to
    // In Review for a human.
    queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
    queries::block_task_ci(&state.db, task.id, &trimmed).await?;
    state.notify_board();
    Ok(())
}

/// Detects the open PR for `branch`, retrying to absorb GitHub's indexing lag.
///
/// A freshly created PR can take a few seconds to surface in the head-filtered
/// pulls list, so checking once the instant the turn ends races GitHub. Returns
/// `None` only after [`PR_DETECT_ATTEMPTS`] checks all come back empty.
async fn detect_pr(
    github: &octocrab::Octocrab,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<Option<git::OpenPr>> {
    for attempt in 1..=PR_DETECT_ATTEMPTS {
        if let Some(pull) = git::find_open_pr_for_branch(github, owner, repo, branch).await? {
            return Ok(Some(pull));
        }
        if attempt < PR_DETECT_ATTEMPTS {
            sleep(PR_DETECT_DELAY).await;
        }
    }
    Ok(None)
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
            ci_fix_attempts: 0,
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
