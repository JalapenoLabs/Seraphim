//! GitHub pull-request operations: detection, CI status, and squash-merge.
//!
//! The agent itself opens PRs with `gh` inside the workspace. Seraphim then
//! detects and acts on them deterministically here, rather than parsing the
//! agent's prose.

use eyre::{Context, Result};
use octocrab::params::pulls::MergeMethod;
use octocrab::params::State;
use octocrab::Octocrab;
use serde::Deserialize;

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
