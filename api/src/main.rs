//! Seraphim API: the orchestrator brain.
//!
//! Boots the database, the workspace handle, and the GitHub client, then spawns
//! the background loops (issue sync, the autonomous agent, and auto-merge review)
//! and serves the REST + SSE surface the kanban UI talks to.

mod claude;
mod config;
mod db;
mod docker;
mod git;
mod http;
mod orchestrator;
mod state;

use eyre::{Context, Result};
use mimalloc::MiMalloc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::docker::Workspace;
use crate::state::AppState;

/// mimalloc as the global allocator for a free throughput win (M-MIMALLOC-APPS).
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    info!(org = %config.org_name, "starting Seraphim API");

    let db = db::connect(&config.database_url).await?;
    bootstrap_settings(&db, &config).await?;

    let settings = db::queries::get_settings(&db).await?;
    let workspace = Workspace::connect(
        config.workspace_container.clone(),
        settings.workspace_image_tag.clone(),
    )?;

    let state = AppState::new(db, workspace, config.internal_api_url.clone());
    orchestrator::spawn(state.clone());

    let listener = tokio::net::TcpListener::bind(&config.api_bind)
        .await
        .wrap_err_with(|| format!("failed to bind {}", config.api_bind))?;
    info!(bind = %config.api_bind, "listening");

    axum::serve(listener, http::router(state))
        .await
        .wrap_err("server error")?;

    Ok(())
}

/// Seeds the org name and default model from the environment on first run, while
/// the settings row still holds its seed defaults. Never overrides UI edits.
async fn bootstrap_settings(db: &sqlx::PgPool, config: &Config) -> Result<()> {
    let settings = db::queries::get_settings(db).await?;

    let org_name = (settings.org_name == "Seraphim" && config.org_name != "Seraphim")
        .then(|| config.org_name.clone());
    let claude_model = (settings.claude_model == "claude-opus-4-8[1m]"
        && config.claude_model != "claude-opus-4-8[1m]")
        .then(|| config.claude_model.clone());

    if org_name.is_some() || claude_model.is_some() {
        db::queries::update_settings(db, org_name, None, None, claude_model, None, None, None)
            .await?;
    }
    Ok(())
}
