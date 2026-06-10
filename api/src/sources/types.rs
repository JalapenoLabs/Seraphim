//! Provider-agnostic issue types.

/// A concrete repository a source resolves to (one in single-repo mode, many in
/// org auto-discovery mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoTarget {
    pub owner: String,
    pub repo: String,
    /// `owner/repo`.
    pub full_name: String,
    /// URL to clone with (SSH by default).
    pub clone_url: String,
    pub default_branch: String,
}

/// A single issue pulled from a source, normalized across providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Stable identifier within the source (GitHub issue number, Jira key).
    pub external_id: String,
    pub title: String,
    pub body: String,
    /// Web URL a human can open.
    pub url: String,
    /// `owner/repo` this issue belongs to, when the source knows it.
    pub repo_full_name: Option<String>,
}
