//! Issue sources: a provider-agnostic surface over GitHub (and, later, Jira).
//!
//! Per M-DI-HIERARCHY we dispatch over a concrete enum rather than a `dyn`
//! trait. Adding Jira is a new variant plus a match arm.

pub mod github;
pub mod types;

use eyre::{eyre, Result};
use octocrab::Octocrab;

use crate::db::models::{IssueSource, SourceKind};
use github::{GitHubConfig, GitHubSource};
use types::Issue;

/// A configured, ready-to-poll issue source.
#[derive(Debug, Clone)]
pub enum Source {
    GitHub(GitHubSource),
}

impl Source {
    /// Builds a live source from its stored config and the shared GitHub client.
    pub fn from_model(model: &IssueSource, octo: &Octocrab) -> Result<Self> {
        match model.kind {
            SourceKind::Github => {
                let config: GitHubConfig = serde_json::from_value(model.config.0.clone())
                    .map_err(|error| eyre!("invalid GitHub source config: {error}"))?;
                Ok(Self::GitHub(GitHubSource::new(octo.clone(), config)))
            }
            SourceKind::Jira => Err(eyre!("Jira sources are not implemented yet")),
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            Self::GitHub(_) => SourceKind::Github,
        }
    }

    /// The `owner/repo` this source targets, if applicable.
    pub fn repo_full_name(&self) -> Option<String> {
        match self {
            Self::GitHub(source) => Some(source.repo_full_name()),
        }
    }

    pub async fn list_issues(&self) -> Result<Vec<Issue>> {
        match self {
            Self::GitHub(source) => source.list_issues().await,
        }
    }
}
