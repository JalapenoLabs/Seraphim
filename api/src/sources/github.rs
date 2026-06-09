//! GitHub issue source backed by octocrab.

use eyre::{Context, Result};
use octocrab::params::State;
use octocrab::Octocrab;
use serde::Deserialize;

use super::types::Issue;

/// How many issues we pull per source per sync tick.
const ISSUES_PER_PAGE: u8 = 50;

/// Config shape stored in `issue_sources.config` for a GitHub source.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    /// Only sync issues carrying all of these labels (empty = no label filter).
    #[serde(default)]
    pub labels: Vec<String>,
}

/// A GitHub issue source bound to one `owner/repo`.
#[derive(Debug, Clone)]
pub struct GitHubSource {
    octo: Octocrab,
    config: GitHubConfig,
}

impl GitHubSource {
    pub fn new(octo: Octocrab, config: GitHubConfig) -> Self {
        Self { octo, config }
    }

    /// `owner/repo` for this source.
    pub fn repo_full_name(&self) -> String {
        format!("{}/{}", self.config.owner, self.config.repo)
    }

    /// Lists open issues, excluding pull requests (the issues API returns both).
    pub async fn list_issues(&self) -> Result<Vec<Issue>> {
        // Bind the handler so the list builder doesn't borrow a temporary.
        let issues_handler = self.octo.issues(&self.config.owner, &self.config.repo);
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
            .wrap_err_with(|| format!("failed to list issues for {}", self.repo_full_name()))?;

        let repo_full_name = self.repo_full_name();
        let issues = page
            .items
            .into_iter()
            // A GitHub "issue" carrying `pull_request` is actually a PR; skip it.
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
    pub async fn comment(&self, issue_number: u64, body: &str) -> Result<()> {
        self.octo
            .issues(&self.config.owner, &self.config.repo)
            .create_comment(issue_number, body)
            .await
            .wrap_err("failed to comment on GitHub issue")?;
        Ok(())
    }
}
