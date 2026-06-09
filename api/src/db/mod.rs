//! Database access: connection pool, migrations, and typed queries.
//!
//! Queries are plain runtime `sqlx` calls (not the compile-time macros) so the
//! crate builds without a live database. Domain types live in [`models`].

pub mod models;
pub mod queries;

use eyre::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Connects to Postgres and runs any pending migrations.
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(database_url)
        .await
        .wrap_err("failed to connect to Postgres")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .wrap_err("failed to run database migrations")?;

    Ok(pool)
}
