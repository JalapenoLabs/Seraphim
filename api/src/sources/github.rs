//! GitHub issue source backed by octocrab.
//!
//! A source targets either a single `owner/repo` or, when `repo` is omitted, an
//! entire org/user (auto-discovery). Either way it resolves to a set of
//! [`RepoTarget`]s, each of which is synced for issues.

use eyre::{Context, Result};
use octocrab::params::State;
use octocrab::Octocrab;
use serde::Deserialize;

use super::types::{Issue, RepoTarget};

/// How many issues we pull per repo per sync tick.
const ISSUES_PER_PAGE: u8 = 50;
/// How many repos we enumerate per org per sync tick.
const REPOS_PER_PAGE: u8 = 100;

/// Config shape stored in `issue_sources.config` for a GitHub source.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubConfig {
    /// Org or user that owns the repos.
    pub owner: String,
    /// A single repo to target. Omit to auto-discover every repo under `owner`.
    #[serde(default)]
    pub repo: Option<String>,
    /// Only sync issues carrying all of these labels (empty = no label filter).
    #[serde(default)]
    pub labels: Vec<String>,
}

/// A GitHub issue source bound to an owner (and optionally one repo).
#[derive(Debug, Clone)]
pub struct GitHubSource {
    octo: Octocrab,
    config: GitHubConfig,
}

/// Minimal repo shape from the GitHub repos API.
#[derive(Debug, Deserialize)]
struct RepoLite {
    full_name: String,
    ssh_url: Option<String>,
    clone_url: Option<String>,
    default_branch: Option<String>,
    #[serde(default)]
    archived: bool,
}

impl GitHubSource {
    pub fn new(octo: Octocrab, config: GitHubConfig) -> Self {
        Self { octo, config }
    }

    /// Resolves this source to the concrete repos it should sync.
    pub async fn targets(&self) -> Result<Vec<RepoTarget>> {
        if let Some(repo) = &self.config.repo {
            let full_name = format!("{}/{}", self.config.owner, repo);
            return Ok(vec![RepoTarget {
                owner: self.config.owner.clone(),
                repo: repo.clone(),
                full_name: full_name.clone(),
                // Default to SSH; users on SSH hosts can clone immediately, and
                // it's editable on the Repos page afterward.
                clone_url: format!("git@github.com:{full_name}.git"),
                default_branch: "main".to_string(),
            }]);
        }

        // Org mode: enumerate every non-archived repo under the owner.
        let repos: Vec<RepoLite> = self
            .octo
            .get(
                format!(
                    "/orgs/{}/repos?per_page={REPOS_PER_PAGE}&type=all",
                    self.config.owner
                ),
                None::<&()>,
            )
            .await
            .wrap_err_with(|| format!("failed to list repos for org {}", self.config.owner))?;

        let targets = repos
            .into_iter()
            .filter(|repo| !repo.archived)
            .filter_map(|repo| {
                let clone_url = repo.ssh_url.or(repo.clone_url)?;
                let name = repo.full_name.rsplit('/').next()?.to_string();
                Some(RepoTarget {
                    owner: self.config.owner.clone(),
                    repo: name,
                    full_name: repo.full_name,
                    clone_url,
                    default_branch: repo.default_branch.unwrap_or_else(|| "main".to_string()),
                })
            })
            .collect();

        Ok(targets)
    }

    /// Lists open issues for one repo, excluding pull requests.
    pub async fn list_issues_for(&self, owner: &str, repo: &str) -> Result<Vec<Issue>> {
        let issues_handler = self.octo.issues(owner, repo);
        let mut request = issues_handler
            .list()
            .state(State::Open)
            .per_page(ISSUES_PER_PAGE);

        if !self.config.labels.is_empty() {
            request = request.labels(&self.config.labels);
        }

        let page = request
            .send()
            .await
            .wrap_err_with(|| format!("failed to list issues for {owner}/{repo}"))?;

        let repo_full_name = format!("{owner}/{repo}");
        let issues = page
            .items
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .map(|issue| Issue {
                external_id: issue.number.to_string(),
                title: issue.title,
                body: issue.body.unwrap_or_default(),
                url: issue.html_url.to_string(),
                repo_full_name: Some(repo_full_name.clone()),
            })
            .collect();

        Ok(issues)
    }

    /// Posts a comment on an issue (used to narrate the agent's progress).
    #[expect(dead_code, reason = "wired up in phase 2 for progress comments")]
    pub async fn comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        body: &str,
    ) -> Result<()> {
        self.octo
            .issues(owner, repo)
            .create_comment(issue_number, body)
            .await
            .wrap_err("failed to comment on GitHub issue")?;
        Ok(())
    }
}
