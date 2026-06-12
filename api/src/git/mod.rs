//! GitHub operations via octocrab: issue listing, org repo discovery, and
//! pull-request detection / CI status / squash-merge.
//!
//! The agent itself opens PRs with `gh` inside the workspace. Seraphim then
//! detects and acts on them deterministically here, rather than parsing the
//! agent's prose.

use eyre::{Context, Result};
use octocrab::params::pulls::MergeMethod;
use octocrab::params::State;
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// How many issues we pull per repo per sync, and repos per org per discovery.
const PER_PAGE: u8 = 100;

/// An open issue synced into the board.
#[derive(Debug, Clone)]
pub struct OpenIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    /// The login and avatar of whoever opened the issue, for the board card.
    pub author_login: String,
    pub author_avatar_url: String,
}

/// Lists open issues for a repo (excluding PRs), optionally label-filtered.
pub async fn list_open_issues(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    labels: &[String],
) -> Result<Vec<OpenIssue>> {
    let handler = octo.issues(owner, repo);
    let mut request = handler.list().state(State::Open).per_page(PER_PAGE);
    if !labels.is_empty() {
        request = request.labels(labels);
    }

    let page = request
        .send()
        .await
        .wrap_err_with(|| format!("failed to list issues for {owner}/{repo}"))?;

    Ok(page
        .items
        .into_iter()
        .filter(|issue| issue.pull_request.is_none())
        .map(|issue| OpenIssue {
            number: issue.number,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            url: issue.html_url.to_string(),
            author_login: issue.user.login,
            author_avatar_url: issue.user.avatar_url.to_string(),
        })
        .collect())
}

/// The numbers of recently-updated closed issues (excluding PRs), most-recent
/// first, optionally label-filtered. One page is enough to catch issues closed
/// outside Seraphim since the last poll, so the board can move them to Done.
pub async fn list_recently_closed_issues(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    labels: &[String],
) -> Result<Vec<u64>> {
    let handler = octo.issues(owner, repo);
    let mut request = handler
        .list()
        .state(State::Closed)
        .sort(octocrab::params::issues::Sort::Updated)
        .direction(octocrab::params::Direction::Descending)
        .per_page(PER_PAGE);
    if !labels.is_empty() {
        request = request.labels(labels);
    }

    let page = request
        .send()
        .await
        .wrap_err_with(|| format!("failed to list closed issues for {owner}/{repo}"))?;

    Ok(page
        .items
        .into_iter()
        .filter(|issue| issue.pull_request.is_none())
        .map(|issue| issue.number)
        .collect())
}

/// A repo discovered when importing an org.
#[derive(Debug, Clone)]
pub struct DiscoveredRepo {
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: String,
}

#[derive(Debug, Deserialize)]
struct RepoLite {
    full_name: String,
    ssh_url: Option<String>,
    clone_url: Option<String>,
    default_branch: Option<String>,
    #[serde(default)]
    archived: bool,
}

/// Enumerates every non-archived repo under an org/user (SSH clone URLs).
pub async fn list_org_repos(octo: &Octocrab, owner: &str) -> Result<Vec<DiscoveredRepo>> {
    let repos: Vec<RepoLite> = octo
        .get(
            format!("/orgs/{owner}/repos?per_page={PER_PAGE}&type=all"),
            None::<&()>,
        )
        .await
        .wrap_err_with(|| format!("failed to list repos for org {owner}"))?;

    Ok(repos
        .into_iter()
        .filter(|repo| !repo.archived)
        .filter_map(|repo| {
            let clone_url = repo.ssh_url.or(repo.clone_url)?;
            Some(DiscoveredRepo {
                full_name: repo.full_name,
                clone_url,
                default_branch: repo.default_branch.unwrap_or_else(|| "main".to_string()),
            })
        })
        .collect())
}

/// An open pull request the agent opened for a work branch.
#[derive(Debug, Clone)]
pub struct OpenPr {
    pub number: u64,
    pub html_url: String,
    pub head_sha: String,
}

/// Finds the open PR whose head branch is `branch` in `owner/repo`, if any.
///
/// Lists open PRs and matches on the head branch ref rather than GitHub's
/// `head=owner:branch` filter. That filter keys on the head repo's *current*
/// owner login, so it silently returns nothing when the repo's org was renamed
/// (the configured `owner` no longer matches the PR's head owner) or the PR
/// comes from a fork. Matching the ref is robust to both, and to the brief
/// indexing lag the filtered endpoint also suffers.
pub async fn find_open_pr_for_branch(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<Option<OpenPr>> {
    let page = octo
        .pulls(owner, repo)
        .list()
        .state(State::Open)
        .per_page(PER_PAGE)
        .send()
        .await
        .wrap_err("failed to list pull requests")?;

    let Some(pull) = page
        .items
        .into_iter()
        .find(|pull| pull.head.ref_field == branch)
    else {
        return Ok(None);
    };

    Ok(Some(OpenPr {
        number: pull.number,
        html_url: pull.html_url.map(|url| url.to_string()).unwrap_or_default(),
        head_sha: pull.head.sha,
    }))
}

/// The lifecycle state of a specific pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrLifecycle {
    Open,
    Merged,
    Closed,
}

/// A tracked PR's current state: whether it's still open (and its latest head, to
/// re-check CI) or has since merged/closed.
#[derive(Debug, Clone)]
pub struct PrStatus {
    pub lifecycle: PrLifecycle,
    pub head_sha: String,
}

/// Looks up one PR by number, to learn whether it's still open (and its head) or
/// was merged (counts toward the task) vs just closed (abandoned).
pub async fn pr_status(octo: &Octocrab, owner: &str, repo: &str, number: u64) -> Result<PrStatus> {
    #[derive(Deserialize)]
    struct Head {
        sha: String,
    }
    #[derive(Deserialize)]
    struct Pull {
        state: String,
        #[serde(default)]
        merged: bool,
        head: Head,
    }
    let pull: Pull = octo
        .get(format!("/repos/{owner}/{repo}/pulls/{number}"), None::<&()>)
        .await
        .wrap_err("failed to fetch pull request state")?;
    let lifecycle = if pull.merged {
        PrLifecycle::Merged
    } else if pull.state == "open" {
        PrLifecycle::Open
    } else {
        PrLifecycle::Closed
    };
    Ok(PrStatus {
        lifecycle,
        head_sha: pull.head.sha,
    })
}

#[derive(Debug, Deserialize)]
struct CombinedStatus {
    state: String,
    total_count: u64,
}

/// The aggregate CI verdict for a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiStatus {
    /// At least one check is still queued or running; no verdict yet.
    Pending,
    /// Every check completed successfully, or there are no checks at all.
    Passing,
    /// At least one check completed unsuccessfully; carries the failing names.
    Failing(Vec<String>),
}

/// The aggregate CI verdict for a commit, from GitHub Actions workflow runs plus
/// the legacy combined commit status.
///
/// CI is read from the Actions API (`/actions/runs?head_sha=`), not the Checks
/// API: the Checks API is accessible only to GitHub Apps, so the fine-grained PAT
/// Seraphim authenticates with gets a 403 there. A PAT with `Actions: read` can
/// list workflow runs, which carry the same pass/fail verdict (their jobs are the
/// check runs). Legacy non-Actions CI is folded in via the combined commit status.
///
/// Waits for every run to finish before reporting [`CiStatus::Failing`], so a
/// downstream fix sees the complete set of failures rather than a partial one.
/// A commit with no CI at all is [`CiStatus::Passing`].
pub async fn ci_status(octo: &Octocrab, owner: &str, repo: &str, sha: &str) -> Result<CiStatus> {
    let runs: WorkflowRunsPage = octo
        .get(
            format!("/repos/{owner}/{repo}/actions/runs?head_sha={sha}&per_page={PER_PAGE}"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to list workflow runs")?;

    let mut pending = false;
    let mut failures: Vec<String> = Vec::new();
    for run in &runs.workflow_runs {
        if run.status != "completed" {
            pending = true;
        } else if !matches!(
            run.conclusion.as_deref(),
            // `cancelled` is not a failure: a run superseded by `cancel-in-progress`
            // concurrency is replaced by a newer run, not a real CI failure.
            Some("success" | "neutral" | "skipped" | "cancelled")
        ) {
            failures.push(run.name.clone());
        }
    }

    // Legacy commit statuses (non-Actions CI). Only relevant when real contexts are
    // present; GitHub reports `pending` with `total_count = 0` when there are none.
    let combined: CombinedStatus = octo
        .get(
            format!("/repos/{owner}/{repo}/commits/{sha}/status"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to fetch combined commit status")?;
    match combined.state.as_str() {
        "failure" | "error" => failures.push("commit status".to_string()),
        "pending" if combined.total_count > 0 => pending = true,
        _ => {}
    }

    if pending {
        Ok(CiStatus::Pending)
    } else if failures.is_empty() {
        Ok(CiStatus::Passing)
    } else {
        Ok(CiStatus::Failing(failures))
    }
}

#[derive(Debug, Deserialize)]
struct WorkflowRunsPage {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    id: u64,
    /// The workflow's display name, reported as the failing check when it fails.
    name: String,
    status: String,
    conclusion: Option<String>,
    run_attempt: u32,
}

/// Re-runs the failed jobs of any first-attempt workflow run at `head_sha`.
///
/// Gives a failed run exactly one automatic retry before an agent fix turn or a
/// human is spent on it, absorbing transient infrastructure flakes (a runner
/// hiccup, a base-image pull failure, a network blip). Idempotency is stateless:
/// a run that was already retried sits at `run_attempt >= 2` and is skipped, so
/// repeated review passes never re-run the same commit's CI more than once.
///
/// Only `failure`, `timed_out`, and `startup_failure` runs are retried; a
/// `cancelled` run (e.g. one superseded by `cancel-in-progress`) is left alone.
///
/// Returns the number of runs whose failed jobs were re-queued; `0` means
/// nothing was retried (no eligible failures, or each had already been retried).
///
/// # Errors
/// If listing the commit's workflow runs fails. A failure to re-queue an
/// individual run is logged and skipped rather than aborting the sweep.
pub async fn rerun_failed_runs(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    head_sha: &str,
) -> Result<u64> {
    let page: WorkflowRunsPage = octo
        .get(
            format!("/repos/{owner}/{repo}/actions/runs?head_sha={head_sha}&per_page={PER_PAGE}"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to list workflow runs for the commit")?;

    let mut reran = 0_u64;
    for run in &page.workflow_runs {
        let retriable = run.status == "completed"
            && matches!(
                run.conclusion.as_deref(),
                Some("failure" | "timed_out" | "startup_failure")
            );
        // Skip anything that already used its one automatic retry.
        if !retriable || run.run_attempt > 1 {
            continue;
        }
        match octo
            .post::<(), serde_json::Value>(
                format!(
                    "/repos/{owner}/{repo}/actions/runs/{id}/rerun-failed-jobs",
                    id = run.id
                ),
                None,
            )
            .await
        {
            Ok(_) => reran += 1,
            Err(error) => {
                tracing::warn!(error = %error, run_id = run.id, "failed to re-run a workflow run");
            }
        }
    }
    Ok(reran)
}

#[derive(Debug, Deserialize)]
struct GitRef {
    object: GitRefObject,
}

#[derive(Debug, Deserialize)]
struct GitRefObject {
    sha: String,
}

/// The commit SHA at the tip of `branch`.
///
/// Uses the git-ref API, which is strongly consistent, so it reflects a
/// just-pushed commit immediately (unlike the pulls list, which can lag).
pub async fn branch_head_sha(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<String> {
    let git_ref: GitRef = octo
        .get(
            format!("/repos/{owner}/{repo}/git/ref/heads/{branch}"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to read branch ref")?;
    Ok(git_ref.object.sha)
}

/// Squash-merges a pull request.
pub async fn squash_merge(octo: &Octocrab, owner: &str, repo: &str, number: u64) -> Result<()> {
    octo.pulls(owner, repo)
        .merge(number)
        .method(MergeMethod::Squash)
        .send()
        .await
        .wrap_err("failed to squash-merge pull request")?;
    Ok(())
}

/// Closes a pull request without merging it (used by a task hard reset). Leaves
/// the head branch intact; [`delete_remote_branch`] removes that separately.
pub async fn close_pull_request(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<()> {
    octo.pulls(owner, repo)
        .update(number)
        .state(octocrab::params::pulls::State::Closed)
        .send()
        .await
        .wrap_err("failed to close pull request")?;
    Ok(())
}

/// Deletes a branch from the remote (its `heads/<branch>` ref). Used by a task
/// hard reset to discard the work; deleting an open PR's head branch also closes
/// that PR on GitHub, but we close it explicitly first so the order never matters.
pub async fn delete_remote_branch(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<()> {
    octo.repos(owner, repo)
        .delete_ref(&octocrab::params::repos::Reference::Branch(
            branch.to_string(),
        ))
        .await
        .wrap_err("failed to delete remote branch")?;
    Ok(())
}

// --- Issue thread (GitHub-style detail view) ---------------------------------
//
// These structs deserialize straight from the GitHub REST shapes and serialize
// to the frontend unchanged (GitHub already uses the snake_case the UI expects),
// so the issue view renders the real conversation: author, avatars, labels,
// assignees, and comments.

/// A GitHub account as it appears on an issue or comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueUser {
    pub login: String,
    pub avatar_url: String,
    #[serde(default)]
    pub html_url: String,
}

/// An issue label with its hex color (no leading `#`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueLabel {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMilestone {
    pub title: String,
}

/// One comment in the conversation (the issue body is rendered separately).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueComment {
    pub user: IssueUser,
    #[serde(default)]
    pub body: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub author_association: String,
}

/// The issue itself: header, opener, body, and sidebar metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub number: u64,
    pub title: String,
    /// `"open"` or `"closed"`.
    pub state: String,
    pub user: IssueUser,
    #[serde(default)]
    pub body: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub author_association: String,
    #[serde(default)]
    pub labels: Vec<IssueLabel>,
    #[serde(default)]
    pub assignees: Vec<IssueUser>,
    pub milestone: Option<IssueMilestone>,
}

/// An issue plus its comments, powering the GitHub-style conversation view.
#[derive(Debug, Clone, Serialize)]
pub struct IssueThread {
    pub issue: IssueDetail,
    pub comments: Vec<IssueComment>,
}

/// Fetches an issue's comments (oldest first), for the agent's task brief.
///
/// Returns up to the first 100 comments (one page), matching the rest of the
/// issue handling here; that covers any realistic discussion and bounds the
/// prompt size.
pub async fn list_issue_comments(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    number: &str,
) -> Result<Vec<IssueComment>> {
    let comments: Vec<IssueComment> = octo
        .get(
            format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page={PER_PAGE}"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to fetch issue comments")?;
    Ok(comments)
}

/// Fetches an issue and its comments for the conversation view.
pub async fn get_issue_thread(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    number: &str,
) -> Result<IssueThread> {
    let issue: IssueDetail = octo
        .get(
            format!("/repos/{owner}/{repo}/issues/{number}"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to fetch issue")?;
    let comments: Vec<IssueComment> = octo
        .get(
            format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to fetch issue comments")?;
    Ok(IssueThread { issue, comments })
}

/// Opens or closes an issue (with an optional close reason) and returns the
/// updated issue. `state` is `"open"` or `"closed"`; `state_reason` is GitHub's
/// `"completed"` / `"not_planned"` when closing.
pub async fn set_issue_state(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    number: &str,
    state: &str,
    state_reason: Option<&str>,
) -> Result<IssueDetail> {
    let mut body = json!({ "state": state });
    if let Some(reason) = state_reason {
        body["state_reason"] = json!(reason);
    }
    let issue: IssueDetail = octo
        .patch(
            format!("/repos/{owner}/{repo}/issues/{number}"),
            Some(&body),
        )
        .await
        .wrap_err("failed to update issue state")?;
    Ok(issue)
}

/// Posts a comment to an issue and returns the created comment.
/// A freshly opened GitHub issue, enough to link to it.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedIssue {
    pub html_url: String,
}

/// Opens a new issue on `owner/repo` and returns its number + URL.
pub async fn create_issue(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
) -> Result<CreatedIssue> {
    let issue: CreatedIssue = octo
        .post(
            format!("/repos/{owner}/{repo}/issues"),
            Some(&json!({ "title": title, "body": body })),
        )
        .await
        .wrap_err("failed to create GitHub issue")?;
    Ok(issue)
}

pub async fn add_issue_comment(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    number: &str,
    body: &str,
) -> Result<IssueComment> {
    let comment: IssueComment = octo
        .post(
            format!("/repos/{owner}/{repo}/issues/{number}/comments"),
            Some(&json!({ "body": body })),
        )
        .await
        .wrap_err("failed to post issue comment")?;
    Ok(comment)
}
