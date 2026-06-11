//! Process configuration, loaded once from the environment at startup.

use std::env;

use eyre::{Context, Result};

/// All environment-derived settings the API needs to boot.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub api_bind: String,
    /// Default Claude model, used to seed the settings row on first run. App
    /// tokens (Claude OAuth, GitHub) live in the database, not the environment.
    pub claude_model: String,
    pub org_name: String,
    pub workspace_container: String,
    /// URL the workspace container uses to reach this API, so the agent's
    /// `seraphim-ask` and `seraphim-suggest` helpers can post questions and
    /// recommendations. Defaults to the compose service address (`api:27182`),
    /// reachable on the shared Docker network.
    pub internal_api_url: String,
    /// Self-update settings: where the running build came from and how to rebuild.
    pub update: UpdateConfig,
}

/// What the self-update needs: the commit/branch the running image was built from
/// (baked at build time) and the host paths the updater container bind-mounts to
/// `git pull` + `docker compose up -d --build`.
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    /// The commit the running image was built from (`unknown` if not stamped).
    pub git_sha: String,
    /// The branch the running image was built from (`unknown` if not stamped).
    pub git_branch: String,
    /// `owner/repo` on GitHub to check for newer commits.
    pub repo: String,
    /// Absolute HOST path to the checked-out repo, bind-mounted into the updater
    /// so it can `git pull` + rebuild. Empty disables the actual update (the
    /// version check still works).
    pub host_repo_dir: String,
    /// HOST path to `~/.ssh`, mounted read-only so the updater's `git pull` can use
    /// SSH credentials (mirrors the workspace). Empty skips the mount.
    pub ssh_home: String,
    /// HOST path to the Docker socket, mounted into the updater so it can drive
    /// the daemon (`docker compose`).
    pub docker_socket: String,
}

impl Config {
    /// Reads configuration from the environment, applying sensible defaults.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?,
            api_bind: env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:27182".to_string()),
            claude_model: env::var("CLAUDE_MODEL")
                .unwrap_or_else(|_| "claude-opus-4-8[1m]".to_string()),
            org_name: env::var("ORG_NAME").unwrap_or_else(|_| "Seraphim".to_string()),
            workspace_container: env::var("WORKSPACE_CONTAINER")
                .unwrap_or_else(|_| "seraphim-workspace".to_string()),
            internal_api_url: env::var("INTERNAL_API_URL")
                .unwrap_or_else(|_| "http://api:27182".to_string()),
            update: UpdateConfig {
                git_sha: env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                git_branch: env::var("GIT_BRANCH").unwrap_or_else(|_| "unknown".to_string()),
                repo: env::var("SERAPHIM_REPO")
                    .unwrap_or_else(|_| "JalapenoLabs/Seraphim".to_string()),
                host_repo_dir: env::var("HOST_REPO_DIR").unwrap_or_default(),
                ssh_home: env::var("SSH_HOME").unwrap_or_default(),
                docker_socket: env::var("DOCKER_SOCKET")
                    .unwrap_or_else(|_| "/var/run/docker.sock".to_string()),
            },
        })
    }
}
