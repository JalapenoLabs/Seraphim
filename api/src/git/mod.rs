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
use serde::Deserialize;

/// How many issues we pull per repo per sync, and repos per org per discovery.
const PER_PAGE: u8 = 100;

/// An open issue synced into the board.
#[derive(Debug, Clone)]
pub struct OpenIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
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
        })
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

/// Finds the open PR whose head is `branch` in `owner/repo`, if any.
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
        // GitHub expects the head filter as "owner:branch".
        .head(format!("{owner}:{branch}"))
        .per_page(5)
        .send()
        .await
        .wrap_err("failed to list pull requests")?;

    let Some(pull) = page.items.into_iter().next() else {
        return Ok(None);
    };

    Ok(Some(OpenPr {
        number: pull.number,
        html_url: pull.html_url.map(|url| url.to_string()).unwrap_or_default(),
        head_sha: pull.head.sha,
    }))
}

#[derive(Debug, Deserialize)]
struct CheckRunsResponse {
    total_count: u64,
    check_runs: Vec<CheckRun>,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    status: String,
    conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CombinedStatus {
    state: String,
}

/// Whether CI is green for a commit: every check run completed successfully and
/// the legacy combined status is not failing. No checks at all counts as green.
pub async fn checks_green(octo: &Octocrab, owner: &str, repo: &str, sha: &str) -> Result<bool> {
    let check_runs: CheckRunsResponse = octo
        .get(
            format!("/repos/{owner}/{repo}/commits/{sha}/check-runs"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to fetch check runs")?;

    let all_check_runs_passing = check_runs.check_runs.iter().all(|run| {
        run.status == "completed"
            && matches!(
                run.conclusion.as_deref(),
                Some("success" | "neutral" | "skipped")
            )
    });
    if !all_check_runs_passing {
        return Ok(false);
    }

    // Legacy commit statuses (non-Actions CI). "pending" with no checks blocks;
    // "success" or an empty status set passes.
    let combined: CombinedStatus = octo
        .get(
            format!("/repos/{owner}/{repo}/commits/{sha}/status"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to fetch combined commit status")?;

    let no_legacy_failures =
        combined.state == "success" || (combined.state == "pending" && check_runs.total_count > 0);

    Ok(no_legacy_failures)
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
