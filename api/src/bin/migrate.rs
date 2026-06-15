//! Apply the embedded SQL migrations to `$DATABASE_URL`, then exit.
//!
//! The API runs `sqlx::migrate!("./migrations")` at boot (see [`db::connect`]),
//! but these are *runtime* queries, so a broken or duplicate migration (a
//! repeated version number, a typo in an `ALTER TABLE`) still compiles and
//! tests green and only fails when the API first connects to a real database.
//!
//! This binary runs that exact same embedded migration set against a disposable
//! Postgres in CI, so a migration regression fails the pull request instead of
//! only surfacing on a real deploy. It is not part of the shipped image; the
//! API applies its own migrations on startup.
//!
//! Exits non-zero on any connection or migration failure.

use eyre::{Context, Result};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .wrap_err("DATABASE_URL must be set (e.g. postgres://user:pass@localhost:5432/db)")?;

    // One connection is plenty for a one-shot migrate; keeping it at 1 also
    // makes the failure mode obvious if the throwaway Postgres isn't reachable.
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .wrap_err("failed to connect to Postgres")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .wrap_err("failed to run database migrations")?;

    println!("Applied all embedded migrations successfully.");
    Ok(())
}
