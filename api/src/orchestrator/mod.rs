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
mod network;
mod prompt;
mod provision;
mod review;
mod subscription;
mod thoughts;
mod usage;

pub use provision::provision_workspace;

use std::time::{Duration, Instant};

use chrono::Utc;
use eyre::{eyre, Result};
use futures::StreamExt;
use tokio::time::{sleep, timeout};
use tracing::{error, info, warn};

use crate::automation::{self, QueuePosition, RuleAction, RuleContext};
use crate::claude::{run_turn, AgentEventKind, TurnArgs};
use crate::db::models::{
    AutomationRule, Repository, ReviewPolicy, SourceKind, Task, TaskColumn, TaskPullRequest,
    TaskStatus,
};
use crate::db::queries;
use crate::git;
use crate::secrets::Scrubber;
use crate::state::AppState;
use review::{PrCi, PrReview, ReviewDecision};

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
const PR_DETECT_ATTEMPTS: u32 = 8;
/// Delay between PR-detection attempts (so detection waits up to ~24s total).
const PR_DETECT_DELAY: Duration = Duration::from_secs(3);
/// How long the review loop keeps re-detecting a freshly opened PR before it
/// concludes the agent genuinely opened none and fails the task. Generously above
/// any plausible GitHub list-indexing lag, so a transient miss after the turn ends
/// is recovered rather than turned into a permanent false failure.
const PR_DETECT_GRACE: Duration = Duration::from_secs(15 * 60);

/// How many fix turns the agent spends on a PR's failing CI before leaving it
/// for a human. Bounds thrash when a failure is unfixable or out of scope.
const MAX_CI_FIX_ATTEMPTS: i32 = 3;

/// How long a blocked PR rests before the idle agent circles back to retry it,
/// so a genuinely stuck PR is revisited periodically rather than in a tight loop.
const REVISIT_COOLDOWN: Duration = Duration::from_secs(15 * 60);

/// How long the agent waits after a transient (server-side) rate limit before
/// retrying the same turn. Anthropic's "temporarily limiting requests" throttle
/// clears within a few seconds, so this mirrors the human reflex of waiting a
/// moment and resending; short enough that work resumes promptly.
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(8);
/// How many times a single turn is retried through the cooldown before the
/// transient rate limit is treated as a real failure and surfaced on the card.
/// Bounds the wait at roughly `RATE_LIMIT_COOLDOWN * RATE_LIMIT_RETRY_MAX`.
const RATE_LIMIT_RETRY_MAX: u32 = 5;
/// Minimum spacing between live token-usage SSE ticks during a turn. The partial
/// stream updates the in-memory counter on every chunk; this throttles only the
/// "refetch the gauges" nudge so a smooth-but-not-flooding ~3 ticks/second reach
/// the UI.
const LIVE_USAGE_TICK: Duration = Duration::from_millis(350);

/// How long a turn may go without emitting a single event before it is presumed
/// dead (a "heart attack"). A healthy turn streams partial-message usage events
/// continuously while generating and a tool event around each call, so the only
/// legitimate silence is one long-running tool (a build or test). This is set
/// well above the longest realistic silent step so a slow build is never mistaken
/// for a hang, while still bounding how long a genuinely dead turn wastes the
/// single-threaded agent before the defibrillator steps in.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// How often the defibrillator watchdog scans for a stranded turn.
const DEFIB_POLL: Duration = Duration::from_secs(60);

/// How long a task may sit `working` with no activity before the watchdog treats
/// it as stranded. Strictly greater than [`HEARTBEAT_TIMEOUT`] so a live turn
/// always self-terminates through the in-turn heartbeat first; only a turn the
/// in-turn path could not catch (an aborted loop, a wedged non-stream await) is
/// ever reaped here, so the watchdog never races a healthy turn.
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(25 * 60);

/// How many heart attacks one task may suffer before the defibrillator stops
/// reviving it and leaves it for a human. Bounds a task that dies every run from
/// looping forever.
const MAX_DEFIBRILLATIONS: i64 = 3;

/// What kind of work a pulled card needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkMode {
    /// A fresh issue: cut a branch, implement, and open a PR.
    Fresh,
    /// A parked task the user just answered: resume the existing session.
    Resume,
    /// An open PR with failing CI: re-engage on its branch to fix the checks.
    FixCi,
    /// A PR whose auto-merge failed on a conflict: re-engage to merge the base in
    /// and resolve it, then let the review loop re-merge.
    ResolveConflict,
    /// A PR the agent gave up on (CI or merge conflict), retried while idle.
    Revisit,
}

/// Launches the background loops and an initial workspace provision. Returns
/// immediately.
pub fn spawn(state: AppState) {
    tokio::spawn(provision_on_startup(state.clone()));
    tokio::spawn(sync_loop(state.clone()));
    tokio::spawn(review_loop(state.clone()));
    tokio::spawn(defibrillator_loop(state.clone()));
    tokio::spawn(subscription::token_loop(state.clone()));
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

        // GitHub lists newest first; insert oldest first so the newest issue ends
        // up at the very top of Available (each new card is placed above the last).
        for issue in issues.into_iter().rev() {
            // Sync only ever lists open issues, so any issue we see here is open.
            upsert_github_issue(state, repo.id, &issue, "open").await?;
            changed = true;
        }

        // The open list above can't reveal an issue closed outside Seraphim (it
        // simply drops out), so reconcile recently-closed issues separately and
        // move any we still track to Done.
        match git::list_recently_closed_issues(&github, owner, name, &repo.issue_labels).await {
            Ok(numbers) => {
                for number in numbers {
                    if reflect_closed_github_issue(state, repo.id, &number.to_string()).await? {
                        changed = true;
                    }
                }
            }
            Err(error) => {
                warn!(error = %error, repo = %repo.full_name, "failed to list closed issues");
            }
        }
    }

    // Jira: pull tickets from each followed board, mapping the Jira status to one
    // of our columns. New tickets land in the mapped column; existing ones refresh
    // their cached fields and live status but keep the human-set column.
    if let Some(jira) = state.jira().await? {
        for board in queries::list_jira_boards_to_sync(&state.db).await? {
            let issues = match jira.list_board_issues(board.board_id).await {
                Ok(issues) => issues,
                Err(error) => {
                    warn!(error = %error, board = %board.name, "failed to list Jira issues");
                    continue;
                }
            };
            for issue in issues.into_iter().rev() {
                upsert_jira_issue(state, &board, &issue).await?;
                changed = true;
            }
        }
    }

    if changed {
        state.notify_board();
    }
    Ok(())
}

/// The position that places a brand-new card at the top of `column` (just above
/// the current topmost). New issues go to the top so the freshest work leads.
async fn top_of_column_position(state: &AppState, column: TaskColumn) -> Result<f64> {
    Ok(queries::min_position_in_column(&state.db, column)
        .await?
        .unwrap_or(0.0)
        - 1.0)
}

/// The position that places a card at the bottom of `column` (just below the
/// current lowest).
async fn bottom_of_column_position(state: &AppState, column: TaskColumn) -> Result<f64> {
    Ok(queries::max_position_in_column(&state.db, column)
        .await?
        .unwrap_or(0.0)
        + 1.0)
}

// --- Automation rules --------------------------------------------------------

/// The short source name a rule's `source_kind` is matched against.
fn source_name(source: SourceKind) -> &'static str {
    match source {
        SourceKind::Github => "github",
        SourceKind::Jira => "jira",
        SourceKind::Internal => "internal",
    }
}

/// The action of the first enabled rule (in order) whose source, trigger, and
/// conditions all match the event, or `None` if nothing matches.
fn first_matching_action(
    rules: &[AutomationRule],
    source: SourceKind,
    ctx: &RuleContext,
) -> Option<QueuePosition> {
    let name = source_name(source);
    rules
        .iter()
        .filter(|rule| rule.source_kind == "any" || rule.source_kind == name)
        .filter(|rule| rule.triggers.0.contains(&ctx.trigger))
        .find(|rule| rule.criteria.0.matches(ctx))
        .map(|rule| match &rule.action.0 {
            RuleAction::MoveToTodo { position } => *position,
        })
}

/// Evaluates the automation rules for a GitHub issue event. If a rule matches,
/// the issue is ensured-tracked and moved to To Do (top or bottom of the queue),
/// even if the repo's label filter would otherwise exclude it. Returns whether
/// the board changed. Only meaningful for open issues.
#[allow(clippy::too_many_arguments)]
pub async fn run_github_automation(
    state: &AppState,
    repo: &Repository,
    issue: &git::OpenIssue,
    labels: &[String],
    issue_state: &str,
    trigger: automation::Trigger,
    comment: &str,
    comment_author: &str,
) -> Result<bool> {
    let rules = queries::list_enabled_automation_rules(&state.db).await?;
    if rules.is_empty() {
        return Ok(false);
    }

    let ctx = RuleContext {
        trigger,
        repo: &repo.full_name,
        author: &issue.author_login,
        labels,
        title: &issue.title,
        body: &issue.body,
        state: issue_state,
        comment,
        comment_author,
    };
    let Some(position) = first_matching_action(&rules, SourceKind::Github, &ctx) else {
        return Ok(false);
    };

    // A rule matched: ensure the issue is tracked, then move its card to To Do.
    upsert_github_issue(state, repo.id, issue, "open").await?;
    let external_id = issue.number.to_string();
    let Some(task) =
        queries::find_issue_task(&state.db, SourceKind::Github, Some(repo.id), &external_id)
            .await?
    else {
        return Ok(false);
    };
    let target = match position {
        QueuePosition::Top => top_of_column_position(state, TaskColumn::Todo).await?,
        QueuePosition::Bottom => bottom_of_column_position(state, TaskColumn::Todo).await?,
    };
    queries::move_task(&state.db, task.id, TaskColumn::Todo, target).await?;
    info!(task_id = %task.id, ?position, "automation matched: moved issue to To Do");
    Ok(true)
}

/// Upserts a GitHub issue as a task: a brand-new one lands at the top of
/// Available; an existing one only refreshes its cached fields, keeping its
/// human-curated column and position. Shared by the poll sync and the realtime
/// webhook so both place and dedupe issues identically.
pub async fn upsert_github_issue(
    state: &AppState,
    repo_id: uuid::Uuid,
    issue: &git::OpenIssue,
    external_state: &str,
) -> Result<()> {
    let external_id = issue.number.to_string();
    let position = top_of_column_position(state, TaskColumn::Available).await?;

    // Reflect an external reopen (the issue flipped back to open) by returning the
    // card to Available, *before* the upsert refreshes the cached state. Update-
    // only and a no-op unless the state actually changed, so it never disturbs a
    // steady open issue or its human-curated column.
    queries::apply_external_state(
        &state.db,
        SourceKind::Github,
        Some(repo_id),
        &external_id,
        external_state,
        TaskColumn::Available,
        position,
    )
    .await?;

    queries::upsert_issue_task(
        &state.db,
        SourceKind::Github,
        &external_id,
        Some(repo_id),
        &issue.title,
        &issue.body,
        &issue.url,
        external_state,
        &issue.author_login,
        &issue.author_avatar_url,
        position,
    )
    .await?;
    Ok(())
}

/// Reflects an issue closed outside Seraphim by moving its tracked task to Done.
/// Update-only and idempotent (see [`queries::apply_external_state`]); does
/// nothing for an issue we don't track. Returns whether the board changed.
pub async fn reflect_closed_github_issue(
    state: &AppState,
    repo_id: uuid::Uuid,
    external_id: &str,
) -> Result<bool> {
    let position = top_of_column_position(state, TaskColumn::Done).await?;
    queries::apply_external_state(
        &state.db,
        SourceKind::Github,
        Some(repo_id),
        external_id,
        "closed",
        TaskColumn::Done,
        position,
    )
    .await
    .map_err(Into::into)
}

/// Upserts a Jira ticket as a task, placing a brand-new one at the top of the
/// column its mapped status implies. Shared by the poll sync and the webhook.
pub async fn upsert_jira_issue(
    state: &AppState,
    board: &crate::db::models::JiraBoard,
    issue: &crate::jira::JiraIssue,
) -> Result<()> {
    let column = crate::jira::column_for_status(&board.status_map.0, &issue.status);
    let position = top_of_column_position(state, column).await?;

    // Reflect an external Jira status change by moving the card to the column its
    // new status maps to, before the upsert refreshes the cached status. Jira keys
    // are globally unique, so match on the key alone (repo_id = None). Update-only
    // and a no-op unless the status actually changed.
    queries::apply_external_state(
        &state.db,
        SourceKind::Jira,
        None,
        &issue.key,
        &issue.status,
        column,
        position,
    )
    .await?;

    // A ticket can target several repos; the first is the primary one the agent
    // would branch in (multi-repo execution is a follow-up).
    let primary_repo = board.repo_ids.0.first().copied();
    queries::upsert_jira_task(
        &state.db,
        &issue.key,
        primary_repo,
        board.id,
        &issue.summary,
        &issue.description,
        &issue.url,
        &issue.status,
        column,
        position,
    )
    .await?;
    Ok(())
}

/// Hard-resets the agent to a clean slate: stops any running turn, wipes the
/// conversation history and the persisted Claude session, requeues whatever task
/// was being worked, and (when `purge_memories`) deletes the agent's memory files.
/// The next turn the loop runs then spawns a brand-new, context-free session.
pub async fn hard_reset(state: &AppState, purge_memories: bool) -> Result<()> {
    info!(purge_memories, "hard reset requested");

    // Bump first, so an in-flight turn (about to be killed) abandons its post-turn
    // handling and never revives the session or its task after we wipe them.
    state.bump_reset_epoch();

    // Stop the running Claude process and wipe its on-disk session (and memories,
    // when asked). Best-effort: workspace cleanup must not abort the reset.
    let mut script = String::from(
        ": \"${CLAUDE_CONFIG_DIR:=/workspace/.claude}\"\n\
         pkill -9 -f '[c]laude -p' || true\n\
         find \"$CLAUDE_CONFIG_DIR/projects\" -type f -name '*.jsonl' -delete 2>/dev/null || true\n",
    );
    if purge_memories {
        script.push_str(
            "find \"$CLAUDE_CONFIG_DIR/projects\" -type d -name memory -exec rm -rf {} + 2>/dev/null || true\n\
             find \"$CLAUDE_CONFIG_DIR/projects\" -type f -name 'MEMORY.md' -delete 2>/dev/null || true\n",
        );
    }
    if let Err(error) = state
        .workspace
        .exec_capture(
            "/workspace",
            vec!["bash".to_string(), "-lc".to_string(), script],
            vec![],
        )
        .await
    {
        warn!(error = %error, "hard reset: workspace cleanup failed (continuing)");
    }

    // Clear the shared session so the next turn starts blank, purge the recorded
    // history (which also zeroes the turn-derived stats), and requeue the task the
    // agent was mid-work on so a fresh session can redo it cleanly.
    queries::set_current_session_id(&state.db, None).await?;
    let turns_purged = queries::purge_history(&state.db).await?;
    let tasks_requeued = queries::reclaim_orphaned_tasks(&state.db).await?;

    // Drop the ephemeral in-memory signals so the UI doesn't show stale gauges.
    state.set_live_usage(None);
    state.set_cooldown_until(None);

    state.notify_board();
    info!(turns_purged, tasks_requeued, "agent hard reset complete");
    Ok(())
}

// --- Per-task hard reset -----------------------------------------------------

/// What a per-task hard reset did, returned to the UI so it can confirm exactly
/// which side effects happened.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a flat result DTO of four independent, best-effort reset outcomes"
)]
#[derive(Debug, Default, serde::Serialize)]
pub struct ResetSummary {
    /// The agent's in-flight turn on this task was stopped.
    pub interrupted_agent: bool,
    /// An open pull request was closed.
    pub pr_closed: bool,
    /// The branch was deleted from the remote.
    pub branch_deleted: bool,
    /// A closed source issue was reopened.
    pub issue_reopened: bool,
}

/// Hard-resets a single stuck task to a clean slate (issue #72): if the agent is
/// mid-turn on it, that turn is stopped; its pull request is closed, its branch
/// deleted from the remote and the workspace, a closed source issue is reopened,
/// and the card returns to **Available**, queued and unstarted.
///
/// The external GitHub steps are best-effort and independently logged, so a
/// network hiccup on any one of them never prevents the card from being reset
/// locally. The returned [`ResetSummary`] reports what actually happened.
pub async fn reset_task(state: &AppState, task_id: uuid::Uuid) -> Result<ResetSummary> {
    let Some(task) = queries::get_task(&state.db, task_id).await? else {
        return Err(eyre!("task {task_id} not found"));
    };
    info!(task_id = %task.id, title = %task.title, "hard reset of a task requested");

    let mut summary = ResetSummary::default();

    // Stop the agent only if it is *actively* running a turn on THIS task. The
    // loop is single-threaded, so the live turn is unique; a task merely parked
    // awaiting input, or sitting in review, is not it and must not be disturbed.
    // Interrupting the live turn bumps the reset epoch (so the turn abandons its
    // post-turn handling rather than re-moving the card), kills the Claude
    // process, and clears the shared session, since a turn killed mid-stream can
    // leave the resumable conversation inconsistent for the next task.
    let actively_working = task.board_column == TaskColumn::InProgress
        && matches!(task.status, TaskStatus::Working | TaskStatus::Preparing);
    if actively_working {
        state.bump_reset_epoch();
        kill_agent_process(state).await;
        queries::set_current_session_id(&state.db, None).await?;
        state.set_live_usage(None);
        summary.interrupted_agent = true;
        info!(task_id = %task.id, "stopped the agent's in-flight turn for the reset");
    }

    // Best-effort external cleanup for a GitHub-sourced task with a known repo.
    if task.source_kind == SourceKind::Github {
        if let Some(repo_id) = task.repo_id {
            if let Some(repo) = queries::get_repository(&state.db, repo_id).await? {
                reset_github_side(state, &task, &repo, &mut summary).await;
            }
        }
    }

    // Local reset: clear the branch/PR/error/session and return the card to
    // Available, queued and unstarted, then drop any pending questions so it
    // stops asking for input.
    let position = top_of_column_position(state, TaskColumn::Available).await?;
    queries::reset_task(&state.db, task.id, position).await?;
    if let Err(error) = queries::delete_pending_questions(&state.db, task.id).await {
        warn!(error = %error, task_id = %task.id, "failed to clear pending questions on reset");
    }

    state.notify_board();
    info!(task_id = %task.id, ?summary, "task hard reset complete");
    Ok(summary)
}

/// The GitHub-side cleanup of a reset: close the open PR, delete its branch from
/// the remote and the workspace, and reopen a closed source issue. Every step is
/// best-effort and updates `summary` with what succeeded.
async fn reset_github_side(
    state: &AppState,
    task: &Task,
    repo: &Repository,
    summary: &mut ResetSummary,
) {
    let Ok((owner, name)) = split_full_name(&repo.full_name) else {
        warn!(repo = %repo.full_name, "reset: repository is not owner/repo; skipping GitHub cleanup");
        return;
    };
    let github = match state.github().await {
        Ok(github) => github,
        Err(error) => {
            warn!(error = %error, "reset: GitHub client unavailable; skipping remote cleanup");
            return;
        }
    };

    if let Some(branch) = task.branch.as_deref() {
        // Close any open PR on the branch first, so the close is explicit even
        // though deleting the head branch would also close it.
        match git::find_open_pr_for_branch(&github, owner, name, branch).await {
            Ok(Some(pull)) => {
                match git::close_pull_request(&github, owner, name, pull.number).await {
                    Ok(()) => {
                        summary.pr_closed = true;
                        info!(task_id = %task.id, pr = pull.number, "reset: closed the pull request");
                    }
                    Err(error) => {
                        warn!(error = %error, task_id = %task.id, "reset: failed to close the pull request");
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                warn!(error = %error, task_id = %task.id, "reset: failed to look up the pull request");
            }
        }

        // Delete the branch from the remote, then from the workspace clone.
        match git::delete_remote_branch(&github, owner, name, branch).await {
            Ok(()) => {
                summary.branch_deleted = true;
                info!(task_id = %task.id, branch, "reset: deleted the remote branch");
            }
            Err(error) => {
                warn!(error = %error, task_id = %task.id, "reset: failed to delete the remote branch (already gone?)");
            }
        }
        delete_local_branch(state, repo, branch).await;
    }

    // Reopen the source issue if Seraphim has it recorded as closed.
    if !task.external_id.trim().is_empty() && task.external_state.as_deref() == Some("closed") {
        match git::set_issue_state(&github, owner, name, &task.external_id, "open", None).await {
            Ok(_) => {
                summary.issue_reopened = true;
                info!(task_id = %task.id, issue = %task.external_id, "reset: reopened the issue");
            }
            Err(error) => {
                warn!(error = %error, task_id = %task.id, "reset: failed to reopen the issue");
            }
        }
    }
}

/// Deletes the task's branch from the workspace clone, switching off it first
/// (a checked-out branch can't be deleted). Best-effort: a missing repo dir or
/// branch is fine, since the authoritative copy was already deleted on the remote.
async fn delete_local_branch(state: &AppState, repo: &Repository, branch: &str) {
    let dir = format!("/workspace/{}", provision::repo_dir_name(&repo.full_name));
    let script = format!(
        "cd \"{dir}\" 2>/dev/null || exit 0\n\
         git checkout \"{default}\" 2>/dev/null || true\n\
         git branch -D \"{branch}\" 2>/dev/null || true\n",
        default = repo.default_branch,
    );
    if let Err(error) = state
        .workspace
        .exec_capture(
            "/workspace",
            vec!["bash".to_string(), "-lc".to_string(), script],
            vec![],
        )
        .await
    {
        warn!(error = %error, branch, "reset: failed to delete the local branch (continuing)");
    }
}

// --- Agent loop --------------------------------------------------------------

async fn agent_loop(state: AppState) {
    loop {
        match next_actionable_task(&state).await {
            Ok(Some((task, mode))) => {
                // Keep a snapshot: if the turn aborts (a `?` propagated, leaving
                // the card stranded and possibly an orphaned process), the
                // defibrillator needs the task to record and recover it.
                let snapshot = task.clone();
                if let Err(error) = work_task(&state, task, mode).await {
                    error!(error = %error, task_id = %snapshot.id, "task run aborted; defibrillating");
                    let detail = format!("The turn aborted with an error: {error}");
                    if let Err(defib_error) =
                        defibrillate(&state, &snapshot, "working", &detail).await
                    {
                        error!(error = %defib_error, task_id = %snapshot.id, "defibrillator failed after a turn abort");
                    }
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
    // Automatic usage-limit pause: hold all new work until the subscription
    // window resets, then clear the pause and resume pulling.
    if let Some(until) = settings.usage_paused_until {
        if Utc::now() < until {
            return Ok(None);
        }
        queries::set_usage_paused_until(&state.db, None).await?;
        state.notify_board();
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
    // Resolving a conflict that blocked auto-merge also takes priority over fresh
    // work, so a PR that just lost mergeability is unblocked promptly rather than
    // abandoned while the agent moves on to the next issue.
    if let Some(task) = queries::pick_next_merge_conflict(&state.db).await? {
        return Ok(Some((task, WorkMode::ResolveConflict)));
    }
    // A blocking task in progress (being worked or parked waiting for input)
    // serializes the queue: pull no new To Do work until it finishes. Resumes
    // and CI fixes above continue existing in-flight work and are not gated.
    if !queries::has_active_blocking_task(&state.db).await? {
        if let Some(task) = queries::pick_next_todo(&state.db).await? {
            return Ok(Some((task, WorkMode::Fresh)));
        }
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
        // Every "re-engage on an existing PR" mode shares one flow; the mode only
        // chooses the prompt and whether the attempt budget is reset.
        WorkMode::FixCi | WorkMode::ResolveConflict | WorkMode::Revisit => {
            work_pr_fix(state, task, mode).await
        }
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

    // The per-repo branch template is an optional override of the global default.
    let branch_template = repo
        .branch_template
        .as_deref()
        .filter(|template| !template.trim().is_empty())
        .unwrap_or(&settings.default_branch_template);

    // A resumed task already has its branch and working tree; only a fresh task
    // is moved into In Progress and re-cut from the default branch.
    let branch = if resume {
        task.branch
            .clone()
            .unwrap_or_else(|| render_branch(branch_template, &task))
    } else {
        queries::move_task(&state.db, task.id, TaskColumn::InProgress, task.position).await?;
        queries::set_task_status(&state.db, task.id, TaskStatus::Preparing).await?;
        state.notify_board();

        let branch = render_branch(branch_template, &task);
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
        let comments = fetch_issue_comments(state, &repo, &task).await;
        prompt::build(&settings, &repo, &task, &branch, &comments)
    };
    let outcome = run_agent_turn(state, &settings, &task, prompt).await?;
    persist_session(state, &settings, &outcome).await?;
    // A hard reset during the turn has already reclaimed this task and wiped the
    // session; don't fail it or move it, just yield to the reset.
    if state.reset_epoch() != outcome.epoch {
        info!(task_id = %task.id, "hard reset during turn; leaving the task to the reset");
        return Ok(());
    }
    // The turn died (hung or its stream broke), not merely reported a problem:
    // hand it to the defibrillator (kill the orphan, revive, alert) rather than a
    // plain failure.
    if outcome.heart_attack {
        let detail = outcome
            .error
            .as_deref()
            .unwrap_or("the agent stopped responding");
        return defibrillate(state, &task, "working", detail).await;
    }
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

    // Deterministically detect every PR the agent opened on this branch, across
    // all enabled repos (a task may span more than one). Retry to absorb GitHub's
    // brief indexing lag after `gh pr create`.
    let github = state.github().await?;
    let mut detected = 0;
    for attempt in 1..=PR_DETECT_ATTEMPTS {
        detected = detect_task_prs(state, &github, &task).await?;
        if detected > 0 {
            break;
        }
        if attempt < PR_DETECT_ATTEMPTS {
            sleep(PR_DETECT_DELAY).await;
        }
    }
    if detected == 0 {
        // The turn finished cleanly but no PR is visible yet. GitHub's PR-list
        // endpoint lags behind `gh pr create`, so rather than fail outright we move
        // the task into review as awaiting; the review loop keeps re-detecting the
        // branch and only fails it if no PR appears within `PR_DETECT_GRACE`.
        info!(task_id = %task.id, "no PR detected yet; awaiting GitHub indexing via the review loop");
        queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
        queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
        state.notify_board();
        return Ok(());
    }

    set_primary_pr(state, task.id, &repo.full_name).await?;
    queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
    state.notify_board();

    info!(task_id = %task.id, prs = detected, "task moved to review");
    Ok(())
}

/// Re-engages the agent on an open PR that needs another turn: failing CI
/// ([`WorkMode::FixCi`]), a conflict that blocked auto-merge
/// ([`WorkMode::ResolveConflict`]), or a PR it had given up on, retried while idle
/// ([`WorkMode::Revisit`]).
///
/// Checks out the PR's existing branch, runs one turn with the prompt for `mode`,
/// and decides what happens next by whether the agent pushed: a new commit returns
/// the task to review for a re-check (or a re-merge); no new commit means the agent
/// judged it out of scope, so the PR is left for a human and the agent moves on. A
/// revisit also resets the fix-attempt counter so the renewed effort gets the full
/// budget again.
async fn work_pr_fix(state: &AppState, task: Task, mode: WorkMode) -> Result<()> {
    info!(task_id = %task.id, attempts = task.ci_fix_attempts, ?mode, "fixing pull request");

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
    if mode == WorkMode::Revisit {
        queries::reset_ci_fix_attempts(&state.db, task.id).await?;
    }

    // While the turn runs the card sits in In Progress, like any actively-worked
    // task, then returns to In Review when it settles below.
    queries::move_task(&state.db, task.id, TaskColumn::InProgress, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::Working).await?;
    state.notify_board();

    let github = state.github().await?;

    // A task can have PRs in several repos; check out the branch in each so the
    // agent can fix whichever repo is red (or resolve a conflict there). The focus
    // repo comes first, so its prompt context is checked out even on the first fix
    // (before any PR is tracked).
    let repos = task_branch_repos(state, &task, &repo).await?;
    for branch_repo in &repos {
        if let Err(error) =
            provision::prepare_existing_branch(state, &settings, branch_repo, &branch).await
        {
            return fail(
                state,
                &task,
                &format!(
                    "could not check out the PR branch in {}: {error}",
                    branch_repo.full_name,
                ),
            )
            .await;
        }
    }

    // Snapshot each repo's branch tip so we can later tell whether the agent
    // pushed to any of them.
    let mut before_shas = Vec::with_capacity(repos.len());
    for branch_repo in &repos {
        let (owner, name) = split_full_name(&branch_repo.full_name)?;
        before_shas.push(
            git::branch_head_sha(&github, owner, name, &branch)
                .await
                .ok(),
        );
    }

    let comments = fetch_issue_comments(state, &repo, &task).await;
    let prompt = match mode {
        WorkMode::FixCi => {
            // Enumerate the failing checks across every tracked open PR (best-
            // effort), tagging each with its repo when the task spans more than one.
            let failing = collect_failing_checks(state, &github, &task, repos.len() > 1).await;
            prompt::build_ci_fix(&settings, &repo, &task, &branch, &failing, &comments)
        }
        WorkMode::ResolveConflict => prompt::build_merge_conflict(
            &settings,
            &repo,
            &task,
            &branch,
            task.error.as_deref().unwrap_or_default(),
            &comments,
        ),
        WorkMode::Revisit => prompt::build_revisit(
            &settings,
            &repo,
            &task,
            &branch,
            task.error.as_deref().unwrap_or_default(),
            &comments,
        ),
        // work_task only routes the three PR-fix modes here.
        WorkMode::Fresh | WorkMode::Resume => {
            unreachable!("work_pr_fix is only called for PR-fix modes")
        }
    };
    let attempt = queries::bump_ci_fix_attempt(&state.db, task.id).await?;
    let outcome = run_agent_turn(state, &settings, &task, prompt).await?;
    persist_session(state, &settings, &outcome).await?;
    // A hard reset during the turn owns this task now; yield to it.
    if state.reset_epoch() != outcome.epoch {
        info!(task_id = %task.id, "hard reset during turn; leaving the task to the reset");
        return Ok(());
    }
    // A turn that died mid-fix goes to the defibrillator: it already has a PR, so
    // recovery returns it to review rather than re-queuing it as fresh work.
    if outcome.heart_attack {
        let detail = outcome
            .error
            .as_deref()
            .unwrap_or("the agent stopped responding");
        return defibrillate(state, &task, "working", detail).await;
    }
    if let Some(message) = outcome.error {
        return fail(state, &task, &message).await;
    }

    // A pushed commit moves a tip in some repo; nothing pushed in any means the
    // agent chose not to act (e.g. the failure is pre-existing or out of scope).
    let mut pushed = false;
    for (branch_repo, before_sha) in repos.iter().zip(&before_shas) {
        let (owner, name) = split_full_name(&branch_repo.full_name)?;
        let after_sha = git::branch_head_sha(&github, owner, name, &branch)
            .await
            .ok();
        let repo_pushed = match (before_sha, &after_sha) {
            (Some(before), Some(after)) => before != after,
            // If a tip can't be read, assume progress and let the review loop judge.
            _ => true,
        };
        if repo_pushed {
            pushed = true;
            break;
        }
    }

    if !pushed {
        // Nothing pushed means the agent judged there was nothing it could (or
        // should) do, so the PR is left for a human with a mode-appropriate note.
        let note = match mode {
            WorkMode::ResolveConflict => {
                "The agent could not resolve the merge conflict on its own (it may need a \
                 human decision or be out of scope). Left for human review."
            }
            _ => {
                "The agent made no changes for the failing CI (likely pre-existing or out of \
                 scope). Left for human review."
            }
        };
        return block(state, &task, note).await;
    }

    // Pushed: back to review so the loop re-checks CI and re-attempts the merge on
    // the new commit.
    queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
    queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
    state.notify_board();
    info!(task_id = %task.id, attempt, "pushed a fix; awaiting re-check");
    Ok(())
}

/// Persists a turn's session id when it differs from the stored one. Skipped if a
/// hard reset happened during the turn, so the just-cleared session isn't revived.
async fn persist_session(
    state: &AppState,
    settings: &crate::db::models::Settings,
    outcome: &TurnOutcome,
) -> Result<()> {
    if state.reset_epoch() != outcome.epoch {
        return Ok(());
    }
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
    /// Whether the turn *died* rather than merely reporting a problem: it hung with
    /// no output past the heartbeat, or its stream broke. These are routed to the
    /// defibrillator (kill the orphan, revive, alert) instead of a plain failure.
    heart_attack: bool,
    /// The hard-reset epoch captured when the turn started, so the caller can tell
    /// whether a reset interrupted it.
    epoch: u64,
}

/// Runs one Claude turn, transparently retrying through a brief cooldown when it
/// fails with a transient (server-side) rate limit.
///
/// Anthropic occasionally throttles requests ("Server is temporarily limiting
/// requests", distinct from the subscription usage limit); the throttle clears
/// within seconds. Rather than fail the task, the agent waits
/// [`RATE_LIMIT_COOLDOWN`] and resends the same turn, up to
/// [`RATE_LIMIT_RETRY_MAX`] times, surfacing the cooldown in the navbar while it
/// waits. Any other outcome (success, a different error) returns immediately.
async fn run_agent_turn(
    state: &AppState,
    settings: &crate::db::models::Settings,
    task: &Task,
    prompt: String,
) -> Result<TurnOutcome> {
    let mut attempt = 0_u32;
    loop {
        attempt += 1;
        let outcome = stream_turn(state, settings, task, prompt.clone()).await?;

        let throttled = outcome
            .error
            .as_deref()
            .is_some_and(is_transient_rate_limit);
        if throttled && attempt < RATE_LIMIT_RETRY_MAX {
            let resume_at = Utc::now()
                + chrono::Duration::from_std(RATE_LIMIT_COOLDOWN)
                    .expect("cooldown is a small, valid duration");
            state.set_cooldown_until(Some(resume_at));
            state.notify_board();
            warn!(
                task_id = %task.id,
                attempt,
                "transient rate limit; cooling down before retrying the turn"
            );
            sleep(RATE_LIMIT_COOLDOWN).await;
            continue;
        }

        // Settled (succeeded, or failed for some other reason): clear any
        // cooldown we raised so the navbar stops showing it.
        if state.cooldown_until().is_some() {
            state.set_cooldown_until(None);
            state.notify_board();
        }
        return Ok(outcome);
    }
}

/// Whether a turn's error message is Anthropic's transient, server-side request
/// throttle rather than a genuine failure or the subscription usage limit.
///
/// Claude Code surfaces it as e.g. "API Error: Server is temporarily limiting
/// requests (not your usage limit) · Rate limited". The subscription usage limit
/// is handled separately (via `rate_limit_event` notices), so it is deliberately
/// excluded here.
fn is_transient_rate_limit(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("temporarily limiting requests")
        || lower.contains("overloaded")
        || lower.contains("rate_limit_error")
        || (lower.contains("rate limited") && lower.contains("api error"))
}

/// Streams one Claude turn for `prompt`, persisting every event and pushing it
/// to the UI. The caller composes the prompt (fresh work or a CI fix).
async fn stream_turn(
    state: &AppState,
    settings: &crate::db::models::Settings,
    task: &Task,
    prompt: String,
) -> Result<TurnOutcome> {
    // Snapshot the hard-reset epoch so the caller can tell if a reset lands while
    // this turn runs (and then skip reviving its session / moving its task).
    let reset_epoch = state.reset_epoch();

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

    // Record the exact brief we hand Claude as the first event of the turn, so the
    // activity log shows our own instructions (secrets scrubbed) right alongside
    // the agent's response, for full transparency.
    let prompt_event = serde_json::json!({ "text": scrubber.scrub_text(&prompt) });
    queries::append_event(&state.db, turn.id, 0, "prompt", prompt_event.clone()).await?;
    state.notify_task(
        task.id,
        serde_json::json!({ "type": "prompt", "payload": prompt_event, "created_at": Utc::now() }),
    );

    let args = TurnArgs {
        container: state.workspace.container().to_string(),
        working_dir,
        prompt,
        resume_session_id: settings.current_session_id.clone(),
        model: settings.claude_model.clone(),
        auth_mode: settings.claude_auth_mode,
        // Refresh the subscription token if it is near expiry, so a turn never runs
        // on an expired token, even the first turn after a long downtime.
        oauth_token: subscription::fresh_inference_token(state).await?,
        github_token: queries::get_github_token(&state.db).await?,
        task_id: task.id.to_string(),
        internal_api_url: state.internal_api_url.clone(),
        env,
    };

    // Clear any claude process leaked by a previously aborted turn. The agent is
    // single-threaded, so none should legitimately be running; a leftover would
    // otherwise contend on the same shared session. Best-effort.
    kill_agent_process(state).await;

    let mut stream = Box::pin(run_turn(state.workspace.docker(), args));
    // seq 0 is the prompt event recorded above; the stream's events follow.
    let mut seq = 1_i32;
    let mut session_id = settings.current_session_id.clone();
    let mut result_text: Option<String> = None;
    let mut total_cost: Option<f64> = None;
    // The terminal `result` event's `usage` block (input/output/cache tokens),
    // persisted on the turn so the stats endpoints can aggregate it.
    let mut token_usage: Option<serde_json::Value> = None;
    let mut error_message: Option<String> = None;
    // Set when the turn *died* (hung past the heartbeat, or its stream broke), as
    // opposed to the agent merely reporting a problem; routes to the defibrillator.
    let mut heart_attack = false;
    // The reset time of the last usage pause we applied this turn, so repeated
    // rate-limit notices don't re-write the same value.
    let mut usage_pause_reset: Option<i64> = None;
    // The agent's non-JSON "thoughts" this turn (its reasoning and prose),
    // collected for an optional summary comment on the source issue.
    let mut thoughts: Vec<String> = Vec::new();

    // Live token usage from the partial-message stream: the in-memory counter is
    // updated on every chunk, but the SSE tick that refetches the gauges is
    // throttled to `LIVE_USAGE_TICK` so the UI stays smooth without flooding.
    let mut usage = crate::claude::UsageTracker::default();
    let mut last_usage_tick: Option<Instant> = None;
    // A stale overlay from a prior turn would otherwise show until the first tick.
    state.set_live_usage(None);

    loop {
        // Bound each wait for the next event by the heartbeat: a turn that goes
        // silent past it is presumed dead (a heart attack), not merely slow.
        let item = match timeout(HEARTBEAT_TIMEOUT, stream.next()).await {
            Ok(Some(item)) => item,
            // The stream ended: the claude process exited. Normal completion.
            Ok(None) => break,
            Err(_elapsed) => {
                let minutes = HEARTBEAT_TIMEOUT.as_secs() / 60;
                warn!(task_id = %task.id, minutes, "agent heartbeat timed out; presumed hung");
                error_message = Some(format!(
                    "No output from the agent for {minutes} minutes; presumed hung."
                ));
                heart_attack = true;
                // Stop the stalled process now so it can't keep spinning orphaned
                // and so the dropped stream's exec is reaped promptly.
                kill_agent_process(state).await;
                break;
            }
        };
        let event = match item {
            Ok(event) => event,
            Err(error) => {
                // A broken stream means the docker exec/process died under us: a
                // heart attack, not an agent-reported failure.
                warn!(error = %error, "claude stream error");
                error_message = Some(format!("Claude stream error: {error}"));
                heart_attack = true;
                break;
            }
        };

        // Live token usage from a partial-message event. This is a firehose, so it
        // is never persisted or streamed verbatim: it only updates the in-memory
        // live counter, with a throttled SSE tick to refetch the gauges.
        if let AgentEventKind::Usage {
            input_tokens,
            output_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        } = &event.kind
        {
            usage.apply(
                *input_tokens,
                *output_tokens,
                *cache_read_input_tokens,
                *cache_creation_input_tokens,
            );
            state.set_live_usage(Some(crate::state::LiveUsage {
                task_id: task.id,
                output_tokens: usage.output_tokens(),
                context_tokens: usage.context_tokens(),
            }));
            let now = Instant::now();
            if last_usage_tick.is_none_or(|last| now.duration_since(last) >= LIVE_USAGE_TICK) {
                state.notify_usage(task.id);
                last_usage_tick = Some(now);
            }
            continue;
        }

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
            token_usage = event.raw.get("usage").cloned();
            result_text = text.as_deref().map(|text| scrubber.scrub_text(text));
            if *is_error {
                let message = text
                    .clone()
                    .unwrap_or_else(|| "the agent reported an error".to_string());
                error_message = Some(scrubber.scrub_text(&message));
            }
        }

        // Collect the agent's non-JSON thoughts (reasoning + prose), scrubbed, in
        // case we summarize them back onto the issue once the turn ends.
        if let AgentEventKind::Thinking { text } | AgentEventKind::AssistantText { text } =
            &event.kind
        {
            let scrubbed = scrubber.scrub_text(text);
            if !scrubbed.trim().is_empty() {
                thoughts.push(scrubbed);
            }
        }

        // Watch the periodic `rate_limit_event` notices: once a usage window is
        // (nearly) exhausted, park new work until it resets. This never aborts the
        // current task - the agent loop only consults the pause before the *next*
        // pull - so the running task always finishes first.
        if settings.usage_limit_pause_enabled
            && event.raw.get("type").and_then(serde_json::Value::as_str) == Some("rate_limit_event")
        {
            if let Some(info) = event.raw.get("rate_limit_info") {
                if let Some(reset) = usage::pause_until(info, settings.usage_limit_threshold) {
                    if usage_pause_reset != Some(reset) {
                        if let Some(until) = chrono::DateTime::from_timestamp(reset, 0) {
                            queries::set_usage_paused_until(&state.db, Some(until)).await?;
                            state.notify_board();
                            usage_pause_reset = Some(reset);
                            warn!(
                                resets_at = %until,
                                "subscription usage limit reached; pausing new work until reset"
                            );
                        }
                    }
                }
            }
        }

        let label = event.type_label();
        // Scrub secrets out of the payload before it touches the DB or the stream.
        let mut payload = event.raw.clone();
        scrubber.scrub_value(&mut payload);
        queries::append_event(&state.db, turn.id, seq, label, payload.clone()).await?;
        state.notify_task(
            task.id,
            serde_json::json!({ "type": label, "payload": payload, "created_at": Utc::now() }),
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
        token_usage,
        session_id.as_deref(),
    )
    .await?;

    // The turn's usage is now persisted (the source of truth). Drop the live
    // overlay and tick once more so the gauges settle on the final total.
    state.set_live_usage(None);
    state.notify_usage(task.id);

    // Optionally summarize this turn's reasoning back onto the source issue.
    // Best-effort: a failure here never affects the task's own outcome.
    if let Err(error) = thoughts::post_turn_thoughts(state, settings, task, &thoughts).await {
        warn!(error = %error, task = %task.id, "failed to post reasoning summary to the issue");
    }

    Ok(TurnOutcome {
        session_id,
        error: error_message,
        heart_attack,
        epoch: reset_epoch,
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
        if let Err(error) = review_task(state, &settings, &github, &task).await {
            warn!(error = %error, task_id = %task.id, "review of task failed");
        }
    }
    Ok(())
}

/// Reviews one task across *all* of its pull requests: refreshes their CI +
/// lifecycle, then takes the single action the gating rules imply (fix, wait,
/// merge what's mergeable, finish when all merged, or hold for a human).
async fn review_task(
    state: &AppState,
    settings: &crate::db::models::Settings,
    github: &octocrab::Octocrab,
    task: &Task,
) -> Result<()> {
    // Refresh tracked PRs; if none are tracked yet (detection lag, or a task from
    // before multi-PR tracking), discover them from the branch now.
    let mut prs = refresh_task_prs(state, github, task).await?;
    if prs.is_empty() {
        detect_task_prs(state, github, task).await?;
        prs = queries::list_task_prs(&state.db, task.id).await?;
        if prs.is_empty() {
            // The agent finished but no PR is visible yet. Keep re-detecting (GitHub
            // list indexing lags) until the grace period elapses, then conclude none
            // was opened and fail, so a genuinely PR-less task does not wait forever.
            if Utc::now().signed_duration_since(task.updated_at)
                > chrono::Duration::from_std(PR_DETECT_GRACE).unwrap_or(chrono::Duration::zero())
            {
                return fail(
                    state,
                    task,
                    "the agent finished without opening a pull request",
                )
                .await;
            }
            return Ok(()); // PR closed/merged externally, or not indexed yet.
        }
        // A PR surfaced after the turn ended: point the card's primary link at it.
        if let Some(repo_id) = task.repo_id {
            if let Some(repo) = queries::get_repository(&state.db, repo_id).await? {
                set_primary_pr(state, task.id, &repo.full_name).await?;
            }
        }
    }

    let mut views = Vec::with_capacity(prs.len());
    for pr in &prs {
        let auto_merge =
            pr_repo_policy(state, settings, pr).await? == ReviewPolicy::AutoSquashMerge;
        views.push(pr_review_of(pr, auto_merge));
    }

    match review::decide(&views) {
        // Still settling (CI pending, or no actionable PR this tick): re-check next.
        ReviewDecision::Wait => {}
        // Open, passing PRs remain that need a human to merge; keep awaiting. Only
        // write when the status actually changes (e.g. returning from Merging once
        // the auto PRs landed) so steady-state ticks stay quiet.
        ReviewDecision::Hold => {
            if task.status != TaskStatus::AwaitingReview {
                queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
                state.notify_board();
            }
        }
        // A failing PR: hand back to the agent (or block once the cap is hit).
        ReviewDecision::Fix => handle_ci_failing(state, github, task, &prs).await?,
        // Merge the green, auto-merge PRs now; the next tick finishes once all are.
        ReviewDecision::Merge(indices) => {
            merge_task_prs(state, github, task, &prs, &indices).await?;
        }
        // Every PR merged: the task is complete.
        ReviewDecision::Done => {
            queries::finish_task(&state.db, task.id, TaskColumn::Done, TaskStatus::Done).await?;
            state.notify_board();
            // Let the UI play the task-finished sound.
            state.notify_task_finished(task.id, task.title.clone());
            info!(task_id = %task.id, prs = prs.len(), "all PRs merged; marked done");

            // Close the linked GitHub issue from the task's own repo (best-effort).
            if let Some(repo_id) = task.repo_id {
                if let Some(repo) = queries::get_repository(&state.db, repo_id).await? {
                    if let Ok((owner, name)) = split_full_name(&repo.full_name) {
                        close_linked_issue(github, settings, task, owner, name).await;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Squash-merges the green, auto-merge PRs (`indices`) of a task. A merge that
/// fails is almost always a base conflict (another PR landed first); rather than
/// give up, hand the task to the agent to merge the base in and resolve, bounded
/// by the fix-attempt budget. The agent's "nothing to push" path catches the
/// genuinely unresolvable cases (e.g. restricted merge settings); once the budget
/// is exhausted we block for a human.
async fn merge_task_prs(
    state: &AppState,
    github: &octocrab::Octocrab,
    task: &Task,
    prs: &[TaskPullRequest],
    indices: &[usize],
) -> Result<()> {
    queries::set_task_status(&state.db, task.id, TaskStatus::Merging).await?;
    state.notify_board();

    for &index in indices {
        let pr = &prs[index];
        let Some((owner, name)) = pr.repo_full_name.split_once('/') else {
            continue;
        };
        let number = u64::try_from(pr.pr_number).unwrap_or_default();
        match git::squash_merge(github, owner, name, number).await {
            Ok(()) => {
                // Persist the merge immediately, so a later PR's conflict doesn't
                // make us re-merge this one on the next tick.
                queries::upsert_task_pr(
                    &state.db,
                    task.id,
                    pr.repo_id,
                    &pr.repo_full_name,
                    pr.pr_number,
                    &pr.pr_url,
                    &pr.head_sha,
                    "",
                    "merged",
                )
                .await?;
                info!(task_id = %task.id, pr = %pr.pr_url, "auto-merged a PR");
            }
            Err(error) => {
                if task.ci_fix_attempts < MAX_CI_FIX_ATTEMPTS {
                    let note = format!(
                        "Auto-merge of {} failed: {error}. Re-engaging the agent to resolve the \
                         conflict with the base branch.",
                        pr.pr_url,
                    );
                    queries::flag_merge_conflict(&state.db, task.id, &note).await?;
                    state.notify_board();
                } else {
                    let note = format!(
                        "Auto-merge of {} still failing after {MAX_CI_FIX_ATTEMPTS} resolution \
                         attempts: {error}. It likely conflicts with its base branch or merging \
                         is restricted; resolve it manually.",
                        pr.pr_url,
                    );
                    block(state, task, &note).await?;
                }
                return Ok(());
            }
        }
    }
    state.notify_board();
    Ok(())
}

/// Handles a task with at least one failing PR: absorb a transient flake on each
/// failing PR's head once, otherwise hand the whole task back to the agent (or
/// block it once the fix-attempt cap is reached). The fix turn works every repo.
async fn handle_ci_failing(
    state: &AppState,
    github: &octocrab::Octocrab,
    task: &Task,
    prs: &[TaskPullRequest],
) -> Result<()> {
    let failing = || {
        prs.iter()
            .filter(|pr| pr.pr_state == "open" && pr.ci_state == "failing")
    };

    // Re-run a first-attempt flake on each failing PR and judge it on a later tick.
    let mut reran = false;
    for pr in failing() {
        let Some((owner, name)) = pr.repo_full_name.split_once('/') else {
            continue;
        };
        match git::rerun_failed_runs(github, owner, name, &pr.head_sha).await {
            Ok(count) if count > 0 => {
                reran = true;
                info!(task_id = %task.id, pr = %pr.pr_url, "re-ran failed CI once for a possible flake");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, task_id = %task.id, "could not re-run CI; treating as failed");
            }
        }
    }
    if reran {
        return Ok(());
    }

    if task.ci_fix_attempts < MAX_CI_FIX_ATTEMPTS {
        queries::set_task_status(&state.db, task.id, TaskStatus::CiFailing).await?;
    } else {
        let repos: Vec<&str> = failing().map(|pr| pr.repo_full_name.as_str()).collect();
        let note = format!(
            "CI still failing after {MAX_CI_FIX_ATTEMPTS} fix attempts on: {}. Needs a human.",
            repos.join(", "),
        );
        queries::block_task_ci(&state.db, task.id, &note).await?;
    }
    state.notify_board();
    Ok(())
}

// --- Pull-request tracking (multi-repo) --------------------------------------

/// The stored label for a PR's CI verdict.
fn ci_state_label(status: &git::CiStatus) -> &'static str {
    match status {
        git::CiStatus::Passing => "passing",
        git::CiStatus::Pending => "pending",
        git::CiStatus::Failing(_) => "failing",
    }
}

/// Scans every enabled repo for an open PR on the task's branch and records each
/// one with its CI verdict. Run after an agent turn, since that is when PRs are
/// opened or pushed. Returns the number of open PRs tracked (one, for the common
/// single-repo case).
async fn detect_task_prs(
    state: &AppState,
    github: &octocrab::Octocrab,
    task: &Task,
) -> Result<usize> {
    let Some(branch) = task.branch.as_deref() else {
        return Ok(0);
    };
    let mut found = 0;
    for repo in queries::list_repositories(&state.db)
        .await?
        .into_iter()
        .filter(|repo| repo.enabled)
    {
        let Some((owner, name)) = repo.full_name.split_once('/') else {
            continue;
        };
        let Some(pull) = git::find_open_pr_for_branch(github, owner, name, branch).await? else {
            continue;
        };
        let ci = ci_state_label(&git::ci_status(github, owner, name, &pull.head_sha).await?);
        queries::upsert_task_pr(
            &state.db,
            task.id,
            Some(repo.id),
            &repo.full_name,
            i64::try_from(pull.number).unwrap_or_default(),
            &pull.html_url,
            &pull.head_sha,
            ci,
            "open",
        )
        .await?;
        found += 1;
    }
    Ok(found)
}

/// Points the board card's primary PR (`task.pr_url`) at the focus repo's PR, or
/// the first PR opened if the focus repo has none. The full set lives in
/// `task_pull_requests`; this is only the single link the card shows.
async fn set_primary_pr(state: &AppState, task_id: uuid::Uuid, focus_repo: &str) -> Result<()> {
    let prs = queries::list_task_prs(&state.db, task_id).await?;
    let primary = prs
        .iter()
        .find(|pr| pr.repo_full_name == focus_repo)
        .or_else(|| prs.first());
    if let Some(primary) = primary {
        queries::set_task_pr(&state.db, task_id, &primary.pr_url).await?;
    }
    Ok(())
}

/// Refreshes every tracked PR: an open PR's head + CI, or, once it's no longer
/// open, whether it merged or closed. Returns the updated rows.
async fn refresh_task_prs(
    state: &AppState,
    github: &octocrab::Octocrab,
    task: &Task,
) -> Result<Vec<TaskPullRequest>> {
    for pr in queries::list_task_prs(&state.db, task.id).await? {
        if pr.pr_state != "open" {
            continue; // merged/closed PRs are settled.
        }
        let Some((owner, name)) = pr.repo_full_name.split_once('/') else {
            continue;
        };
        let number = u64::try_from(pr.pr_number).unwrap_or_default();
        let status = git::pr_status(github, owner, name, number).await?;
        let (ci, pr_state, head) = match status.lifecycle {
            git::PrLifecycle::Open => {
                let ci =
                    ci_state_label(&git::ci_status(github, owner, name, &status.head_sha).await?);
                (ci, "open", status.head_sha)
            }
            git::PrLifecycle::Merged => ("", "merged", pr.head_sha.clone()),
            git::PrLifecycle::Closed => ("", "closed", pr.head_sha.clone()),
        };
        queries::upsert_task_pr(
            &state.db,
            task.id,
            pr.repo_id,
            &pr.repo_full_name,
            pr.pr_number,
            &pr.pr_url,
            &head,
            ci,
            pr_state,
        )
        .await?;
    }
    Ok(queries::list_task_prs(&state.db, task.id).await?)
}

/// The effective review policy for a tracked PR's repo (its own, else the global
/// default).
async fn pr_repo_policy(
    state: &AppState,
    settings: &crate::db::models::Settings,
    pr: &TaskPullRequest,
) -> Result<ReviewPolicy> {
    let policy = match pr.repo_id {
        Some(repo_id) => queries::get_repository(&state.db, repo_id)
            .await?
            .and_then(|repo| repo.review_policy),
        None => None,
    };
    Ok(policy.unwrap_or(settings.default_review_policy))
}

/// Maps a stored PR row to the pure review view.
fn pr_review_of(pr: &TaskPullRequest, auto_merge: bool) -> PrReview {
    match pr.pr_state.as_str() {
        "merged" => PrReview::Merged,
        "closed" => PrReview::Closed,
        _ => PrReview::Open {
            ci: match pr.ci_state.as_str() {
                "passing" => PrCi::Passing,
                "failing" => PrCi::Failing,
                _ => PrCi::Pending,
            },
            auto_merge,
        },
    }
}

/// The repos whose branch the CI-fix turn should check out: the focus repo (so
/// its context is present even on the first fix, before a PR is tracked) plus
/// every other repo that has a tracked PR on this branch. Deduped, focus first.
async fn task_branch_repos(
    state: &AppState,
    task: &Task,
    focus: &Repository,
) -> Result<Vec<Repository>> {
    let mut repos = vec![focus.clone()];
    for pr in queries::list_task_prs(&state.db, task.id).await? {
        if pr.repo_full_name == focus.full_name {
            continue;
        }
        if repos.iter().any(|repo| repo.full_name == pr.repo_full_name) {
            continue;
        }
        if let Some(repo_id) = pr.repo_id {
            if let Some(repo) = queries::get_repository(&state.db, repo_id).await? {
                repos.push(repo);
            }
        }
    }
    Ok(repos)
}

/// Gathers the failing CI check names across every tracked open PR, tagging each
/// with `repo#pr` when the task spans more than one PR so the agent can tell
/// which repo is red. Best-effort: an unreachable PR contributes nothing.
async fn collect_failing_checks(
    state: &AppState,
    github: &octocrab::Octocrab,
    task: &Task,
    tag_repo: bool,
) -> Vec<String> {
    let Ok(prs) = queries::list_task_prs(&state.db, task.id).await else {
        return Vec::new();
    };
    let mut failing = Vec::new();
    for pr in prs.iter().filter(|pr| pr.pr_state == "open") {
        let Some((owner, name)) = pr.repo_full_name.split_once('/') else {
            continue;
        };
        if let Ok(git::CiStatus::Failing(checks)) =
            git::ci_status(github, owner, name, &pr.head_sha).await
        {
            for check in checks {
                if tag_repo {
                    failing.push(format!("{}#{}: {check}", pr.repo_full_name, pr.pr_number));
                } else {
                    failing.push(check);
                }
            }
        }
    }
    failing
}

// --- Helpers -----------------------------------------------------------------

/// Closes the GitHub issue a finished task came from, with
/// `state_reason: "completed"`. Best-effort: only for GitHub-sourced tasks with a
/// real issue number, gated by the `close_issue_on_done` setting, and any failure
/// is logged and swallowed so it never affects the completed task. Closing an
/// already-closed issue is harmless.
async fn close_linked_issue(
    github: &octocrab::Octocrab,
    settings: &crate::db::models::Settings,
    task: &Task,
    owner: &str,
    repo_name: &str,
) {
    if !settings.close_issue_on_done
        || task.source_kind != SourceKind::Github
        || task.external_id.trim().is_empty()
    {
        return;
    }

    match git::set_issue_state(
        github,
        owner,
        repo_name,
        &task.external_id,
        "closed",
        Some("completed"),
    )
    .await
    {
        Ok(_) => info!(task_id = %task.id, issue = %task.external_id, "closed the linked issue"),
        Err(error) => {
            warn!(error = %error, task_id = %task.id, "failed to close the linked issue");
        }
    }
}

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

// --- Dead-agent management (heart attacks / the defibrillator) ----------------

/// Kills any `claude -p` process left running in the workspace. The agent is
/// single-threaded, so on a healthy machine none should be running between turns;
/// a leftover is an orphan from an aborted turn that would otherwise keep spinning
/// (and contend on the shared session). The `[c]` keeps pkill from matching its
/// own command line. Best-effort: a cleanup failure must never abort the caller.
async fn kill_agent_process(state: &AppState) {
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
}

/// What the defibrillator should do with a task it just revived from a heart
/// attack. A pure decision so the gating is unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recovery {
    /// Send a fresh task back to To Do to be reworked from a clean branch.
    Requeue,
    /// Return a task that already has a PR to In Review, where the review loop
    /// re-evaluates it (and its own CI-fix budget bounds further attempts).
    ReturnToReview,
    /// Stop reviving and leave it failed for a human: it has died too many times.
    GiveUp,
}

/// Decides how to recover a task given how many heart attacks it has now suffered
/// (this one included) and whether it already has a pull request. A task that
/// keeps dying is left for a human once it hits [`MAX_DEFIBRILLATIONS`].
fn decide_recovery(incident_count: i64, has_pr: bool) -> Recovery {
    if incident_count >= MAX_DEFIBRILLATIONS {
        Recovery::GiveUp
    } else if has_pr {
        Recovery::ReturnToReview
    } else {
        Recovery::Requeue
    }
}

/// The defibrillator: handles a task whose turn died (a heart attack). It stops
/// the orphaned process, records the incident with its diagnostic detail so the
/// operator can patch the cause later, revives the task (or leaves it for a human
/// once it has died too often), and raises a UI alert.
///
/// `detail` is the diagnosis (why we think it died). `status_label` is the task's
/// operational status at death, captured for the incident record.
async fn defibrillate(
    state: &AppState,
    task: &Task,
    status_label: &str,
    detail: &str,
) -> Result<()> {
    warn!(task_id = %task.id, detail, "heart attack: defibrillating");

    // Shock first: make sure no orphaned claude process keeps spinning.
    kill_agent_process(state).await;

    // Decide recovery from how many times this task has now died.
    let incident_count = queries::count_heart_attacks_for_task(&state.db, task.id).await? + 1;
    let recovery = decide_recovery(incident_count, task.pr_url.is_some());
    let recovery_note = match recovery {
        Recovery::Requeue => "Revived: requeued to To Do for a clean re-run.",
        Recovery::ReturnToReview => "Revived: returned the pull request to review.",
        Recovery::GiveUp => "Left for a human: too many heart attacks on this task.",
    };

    // Record the incident before acting, so the alert and its logs survive even if
    // the recovery move below fails. A blank title is unhelpful in the banner.
    let title = if task.title.trim().is_empty() {
        "(untitled task)"
    } else {
        task.title.trim()
    };
    let detail: String = detail.trim().chars().take(2000).collect();
    queries::create_heart_attack(
        &state.db,
        Some(task.id),
        title,
        status_label,
        &detail,
        recovery_note,
    )
    .await?;

    // Carry out the recovery.
    match recovery {
        Recovery::Requeue => {
            let position = top_of_column_position(state, TaskColumn::Todo).await?;
            queries::move_task(&state.db, task.id, TaskColumn::Todo, position).await?;
        }
        Recovery::ReturnToReview => {
            queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
            queries::set_task_status(&state.db, task.id, TaskStatus::AwaitingReview).await?;
        }
        Recovery::GiveUp => {
            let note = format!("{detail} ({recovery_note})");
            let trimmed: String = note.trim().chars().take(800).collect();
            queries::set_task_error(&state.db, task.id, &trimmed).await?;
            queries::move_task(&state.db, task.id, TaskColumn::InReview, task.position).await?;
        }
    }

    state.notify_board();
    state.notify_heart_attack(
        Some(task.id),
        title.to_string(),
        format!("Agent heart attack. {recovery_note}"),
    );
    Ok(())
}

/// The defibrillator watchdog: a backstop for a turn that died without the
/// in-turn heartbeat catching it (an aborted agent loop, a wedged non-stream
/// await). It runs independently of the single-threaded agent loop, so it can act
/// even while that loop is blocked.
///
/// It only reaps a task left `working` with no activity for longer than
/// [`WATCHDOG_TIMEOUT`], which is strictly greater than [`HEARTBEAT_TIMEOUT`]; a
/// healthy turn keeps its activity fresh on every event and self-terminates
/// through the in-turn heartbeat well before this fires, so it never races a live
/// turn.
async fn defibrillator_loop(state: AppState) {
    loop {
        sleep(DEFIB_POLL).await;
        let stale_secs = i64::try_from(WATCHDOG_TIMEOUT.as_secs()).unwrap_or(i64::MAX);
        match queries::find_stranded_task(&state.db, stale_secs).await {
            Ok(Some(task)) => {
                // Invalidate the wedged turn so that, if it ever unblocks, its
                // post-turn handling abandons rather than clobbering the recovery
                // (the same guard a hard reset relies on).
                state.bump_reset_epoch();
                let detail = format!(
                    "The turn was stuck working with no activity for over {} minutes and did \
                     not recover on its own.",
                    WATCHDOG_TIMEOUT.as_secs() / 60,
                );
                if let Err(error) = defibrillate(&state, &task, "working", &detail).await {
                    error!(error = %error, task_id = %task.id, "defibrillator failed to recover a stranded task");
                }
            }
            Ok(None) => {}
            Err(error) => warn!(error = %error, "defibrillator watchdog query failed"),
        }
    }
}

/// Best-effort fetch of a task's issue comments so the brief carries the full
/// discussion, not just the description.
///
/// Returns empty for non-GitHub sources or when GitHub is unreachable: the agent
/// then works from the description alone (as it did before), rather than the task
/// failing over missing comments.
async fn fetch_issue_comments(
    state: &AppState,
    repo: &Repository,
    task: &Task,
) -> Vec<git::IssueComment> {
    if task.source_kind != SourceKind::Github {
        return Vec::new();
    }

    let fetched = async {
        let github = state.github().await?;
        let (owner, repo_name) = split_full_name(&repo.full_name)?;
        git::list_issue_comments(&github, owner, repo_name, &task.external_id).await
    }
    .await;

    match fetched {
        Ok(comments) => comments,
        Err(error) => {
            warn!(task_id = %task.id, %error, "could not fetch issue comments; using the description only");
            Vec::new()
        }
    }
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
    fn defibrillator_revives_then_gives_up() {
        // A fresh task (no PR) is requeued; one with a PR returns to review.
        assert_eq!(decide_recovery(1, false), Recovery::Requeue);
        assert_eq!(decide_recovery(1, true), Recovery::ReturnToReview);
        assert_eq!(decide_recovery(2, false), Recovery::Requeue);
        // Once it has died too many times, stop reviving and leave it for a human,
        // regardless of whether it has a PR.
        assert_eq!(
            decide_recovery(MAX_DEFIBRILLATIONS, false),
            Recovery::GiveUp
        );
        assert_eq!(decide_recovery(MAX_DEFIBRILLATIONS, true), Recovery::GiveUp);
        assert_eq!(
            decide_recovery(MAX_DEFIBRILLATIONS + 1, false),
            Recovery::GiveUp
        );
    }

    #[test]
    fn transient_rate_limit_matches_server_throttle_only() {
        // The exact wording Claude Code emits for the server-side throttle.
        assert!(is_transient_rate_limit(
            "API Error: Server is temporarily limiting requests (not your usage limit) · Rate limited"
        ));
        assert!(is_transient_rate_limit("API Error: Overloaded"));
        assert!(is_transient_rate_limit(
            "{\"type\":\"rate_limit_error\",\"message\":\"slow down\"}"
        ));

        // The subscription usage limit (handled elsewhere) must not match, nor
        // should ordinary failures.
        assert!(!is_transient_rate_limit(
            "Usage limit reached. Your limit resets at 5pm."
        ));
        assert!(!is_transient_rate_limit("Not logged in"));
        assert!(!is_transient_rate_limit(
            "the agent finished without opening a pull request"
        ));
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
            jira_board_id: None,
            title: String::new(),
            body_snapshot: String::new(),
            url: String::new(),
            author_login: None,
            author_avatar_url: None,
            external_state: None,
            board_column: TaskColumn::Todo,
            position: 0.0,
            status: TaskStatus::Queued,
            branch: None,
            pr_url: None,
            error: None,
            ci_fix_attempts: 0,
            hold: false,
            blocking: false,
            notes: String::new(),
            session_id: None,
            started_at: None,
            finished_at: None,
            last_activity_at: None,
            stats_reset_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}
