//! Process configuration, loaded once from the environment at startup.

use std::env;

use eyre::{Context, Result};

/// All environment-derived settings the API needs to boot.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub api_bind: String,
    /// GitHub token for deterministic sync and PR/merge operations. May be empty
    /// (the app still boots; GitHub calls will simply fail until configured).
    pub gh_token: String,
    /// Default Claude model, used to seed the settings row on first run. The
    /// subscription OAuth token itself is consumed by the workspace container
    /// (injected via compose), not by the API.
    pub claude_model: String,
    pub org_name: String,
    pub workspace_container: String,
}

impl Config {
    /// Reads configuration from the environment, applying sensible defaults.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: env::var("DATABASE_URL").wrap_err("DATABASE_URL must be set")?,
            api_bind: env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:27182".to_string()),
            gh_token: env::var("GH_TOKEN").unwrap_or_default(),
            claude_model: env::var("CLAUDE_MODEL")
                .unwrap_or_else(|_| "claude-opus-4-8[1m]".to_string()),
            org_name: env::var("ORG_NAME").unwrap_or_else(|_| "Seraphim".to_string()),
            workspace_container: env::var("WORKSPACE_CONTAINER")
                .unwrap_or_else(|_| "seraphim-workspace".to_string()),
        })
    }
}
