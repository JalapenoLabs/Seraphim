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
    /// `seraphim-suggest` helper can post recommendations. Defaults to the
    /// compose service address (`api:27182`) on the shared Docker network.
    pub internal_api_url: String,
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
        })
    }
}
