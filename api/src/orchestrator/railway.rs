//! Per-railway agent execution: the loop supervisor and the per-railway seam.
//!
//! A **railway** is a parallel agent lane (see the data model in `db::models`).
//! Issue #202 makes the orchestrator railway-aware by running **one agent loop
//! per railway**, supervised here, while sync, review, and the defibrillator stay
//! single global loops. Each loop serializes only its own railway's work, so two
//! railways with independent repos progress at the same time.
//!
//! The set of railways changes at runtime (a later issue adds create/delete), so
//! the [`supervise`] loop reconciles the *running* agent loops against the
//! `railways` table on an interval: it spawns a loop for a new railway and aborts
//! one for a removed railway. With only the undeletable `main` railway present,
//! that is exactly one loop, behaving like the previous single agent loop.
//!
//! ## The execution seam (and what is deferred to #203)
//!
//! Every railway has its own workspace container, Claude session, and helper env.
//! [`RailwayHandle`] is the seam the turn runner targets: it resolves the
//! container name and reads/writes the session for a given railway. The
//! **container lifecycle** (naming, lazy start, idle-stop, provisioning) is issue
//! #203, not this one. So for now [`RailwayHandle::container`] returns the single
//! existing workspace container for `main` (keeping its behavior byte-for-byte
//! identical) and a derived, not-yet-created name for any other railway; #203
//! drops in the real per-railway containers behind this same method.
//!
//! For the session, `main` keeps `settings.current_session_id` as the source of
//! truth (so the existing reset and persist paths are untouched) and mirrors it
//! onto its railway row; a non-`main` railway keeps its session only on the row.

use std::collections::HashMap;

use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::models::Railway;
use crate::db::queries;
use crate::state::AppState;

/// How often the supervisor reconciles running agent loops against the `railways`
/// table. Brisk enough that a railway created or deleted in the UI starts or stops
/// working within a few seconds, without polling so tightly it churns the DB.
const SUPERVISE_POLL: Duration = Duration::from_secs(5);

/// The execution context for one railway: its container and Claude session.
///
/// This is the single place the rest of the orchestrator asks "which container do
/// I exec into?" and "what session do I resume / persist?" for a railway, so the
/// per-railway container lifecycle (#203) has exactly one seam to fill in.
#[derive(Debug, Clone)]
pub struct RailwayHandle {
    pub id: Uuid,
    pub is_main: bool,
    /// The container this railway's turns exec into (see the module docs: real
    /// per-railway containers are #203; today only `main`'s container exists).
    container: String,
}

impl RailwayHandle {
    /// Builds the handle for `railway`, resolving its container name against the
    /// workspace. `main` targets the existing single workspace container so its
    /// behavior is unchanged; another railway gets a derived name (a seam for #203,
    /// which will actually create and manage that container).
    pub fn new(state: &AppState, railway: &Railway) -> Self {
        let container = if railway.is_main {
            state.workspace.container().to_string()
        } else {
            // Seam for #203: a stable, per-railway container name. Not created or
            // started here; with only `main` present this branch is never taken.
            format!("{}-railway-{}", state.workspace.container(), railway.id)
        };
        Self {
            id: railway.id,
            is_main: railway.is_main,
            container,
        }
    }

    /// The name of the container this railway's turns exec into.
    pub fn container(&self) -> &str {
        &self.container
    }
}

/// Builds the [`RailwayHandle`] for an existing railway id, for code paths that
/// hold a task (or a railway id) but not a running agent loop, e.g. a reset
/// triggered from the HTTP layer or the global defibrillator watchdog.
///
/// Returns the `main` handle as a fallback if the railway has since been deleted,
/// so a best-effort cleanup (killing an orphaned process) still targets the only
/// container that exists today rather than failing. With only `main` present this
/// is always the main handle.
pub async fn handle_for(state: &AppState, railway_id: Uuid) -> eyre::Result<RailwayHandle> {
    // Fall back to `main` if the railway was deleted out from under us, so a
    // best-effort cleanup still targets a real container rather than erroring.
    let railway = match queries::get_railway(&state.db, railway_id).await? {
        Some(railway) => railway,
        None => queries::get_main_railway(&state.db).await?,
    };
    Ok(RailwayHandle::new(state, &railway))
}

/// Reads this railway's current Claude session id (empty string means none yet).
///
/// `main` reads `settings.current_session_id` so it stays the source of truth and
/// the existing reset / persist logic is unchanged; another railway reads its own
/// row. Returns the session as `Option`, mapping the empty string to `None` the
/// way the turn runner expects (`None` starts a fresh conversation).
pub async fn read_session(
    state: &AppState,
    handle: &RailwayHandle,
) -> eyre::Result<Option<String>> {
    let session = if handle.is_main {
        queries::get_settings(&state.db).await?.current_session_id
    } else {
        queries::get_railway(&state.db, handle.id)
            .await?
            .map(|railway| railway.session_id)
    };
    Ok(session.filter(|id| !id.trim().is_empty()))
}

/// Persists this railway's Claude session id.
///
/// For `main` it writes `settings.current_session_id` (the source of truth) and
/// mirrors the value onto main's railway row; for another railway it writes only
/// the row. `None` clears the session.
pub async fn write_session(
    state: &AppState,
    handle: &RailwayHandle,
    session_id: Option<&str>,
) -> eyre::Result<()> {
    if handle.is_main {
        queries::set_current_session_id(&state.db, session_id).await?;
    }
    queries::set_railway_session_id(&state.db, handle.id, session_id.unwrap_or_default()).await?;
    Ok(())
}

/// Supervises the per-railway agent loops: reconciles the running set against the
/// `railways` table forever, spawning a loop for each new railway and aborting one
/// for any railway that has been removed.
///
/// `spawn_one` builds the agent loop future for a railway; it is injected so the
/// orchestrator owns the actual loop body while this module owns the bookkeeping.
/// A loop that exits on its own (it should not, but defensively) is restarted on
/// the next tick because its handle is finished and gets pruned.
pub async fn supervise<F>(state: AppState, spawn_one: F)
where
    F: Fn(AppState, RailwayHandle) -> JoinHandle<()>,
{
    let mut running: HashMap<Uuid, JoinHandle<()>> = HashMap::new();

    loop {
        match queries::list_railways(&state.db).await {
            Ok(railways) => reconcile(&state, &railways, &mut running, &spawn_one),
            Err(error) => warn!(error = %error, "railway supervisor: failed to list railways"),
        }
        sleep(SUPERVISE_POLL).await;
    }
}

/// Reconciles `running` against the current `railways`: drop loops whose railway
/// is gone (or whose task has already finished) and spawn loops for railways that
/// have none yet. Pure bookkeeping over the join handles; the spawn/abort side
/// effects are driven entirely by the diff between the two id sets.
fn reconcile<F>(
    state: &AppState,
    railways: &[Railway],
    running: &mut HashMap<Uuid, JoinHandle<()>>,
    spawn_one: &F,
) where
    F: Fn(AppState, RailwayHandle) -> JoinHandle<()>,
{
    let current: std::collections::HashSet<Uuid> = railways.iter().map(|r| r.id).collect();

    // Stop loops for railways that no longer exist, and prune any that have ended
    // on their own so the next tick respawns them.
    running.retain(|id, handle| {
        if !current.contains(id) {
            handle.abort();
            info!(railway_id = %id, "railway supervisor: stopped agent loop for a removed railway");
            return false;
        }
        !handle.is_finished()
    });

    // Start a loop for any railway that does not have one yet.
    for railway in railways {
        if running.contains_key(&railway.id) {
            continue;
        }
        let handle = RailwayHandle::new(state, railway);
        info!(
            railway_id = %railway.id,
            name = %railway.name,
            "railway supervisor: starting agent loop"
        );
        running.insert(railway.id, spawn_one(state.clone(), handle));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A railway row with just the fields the reconciler reads.
    fn railway(id: Uuid, is_main: bool) -> Railway {
        Railway {
            id,
            name: if is_main {
                "main".into()
            } else {
                "other".into()
            },
            description: String::new(),
            session_id: String::new(),
            paused: false,
            lifecycle_state: crate::db::models::RailwayState::Stopped,
            is_main,
            position: 0.0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// A spawn fn that just records how many loops it created and returns a handle
    /// to a future that parks forever (so it never reports finished on its own).
    fn counting_spawn(spawned: Arc<AtomicUsize>) -> impl Fn(Uuid) -> JoinHandle<()> {
        move |_id| {
            spawned.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(std::future::pending())
        }
    }

    #[tokio::test]
    async fn reconcile_spawns_one_loop_per_railway_and_only_once() {
        let spawned = Arc::new(AtomicUsize::new(0));
        let spawn = counting_spawn(spawned.clone());
        let mut running: HashMap<Uuid, JoinHandle<()>> = HashMap::new();

        let main = railway(Uuid::new_v4(), true);
        let railways = vec![main.clone()];

        // Re-running with the same set must not spawn a second loop for `main`:
        // the single-railway case stays exactly one loop, as before.
        reconcile_with(&railways, &mut running, &spawn);
        reconcile_with(&railways, &mut running, &spawn);

        assert_eq!(spawned.load(Ordering::SeqCst), 1);
        assert_eq!(running.len(), 1);
        assert!(running.contains_key(&main.id));
    }

    #[tokio::test]
    async fn reconcile_adds_and_removes_loops_as_railways_change() {
        let spawned = Arc::new(AtomicUsize::new(0));
        let spawn = counting_spawn(spawned.clone());
        let mut running: HashMap<Uuid, JoinHandle<()>> = HashMap::new();

        let main = railway(Uuid::new_v4(), true);
        let second = railway(Uuid::new_v4(), false);

        // A second railway appears: a new loop is started, the first kept.
        reconcile_with(&[main.clone()], &mut running, &spawn);
        reconcile_with(&[main.clone(), second.clone()], &mut running, &spawn);
        assert_eq!(spawned.load(Ordering::SeqCst), 2);
        assert_eq!(running.len(), 2);

        // The second railway is deleted: its loop is aborted and dropped, `main`
        // stays. No new loops are spawned for the steady `main`.
        reconcile_with(&[main.clone()], &mut running, &spawn);
        assert_eq!(spawned.load(Ordering::SeqCst), 2);
        assert_eq!(running.len(), 1);
        assert!(running.contains_key(&main.id));
        assert!(!running.contains_key(&second.id));
    }

    /// Test shim mirroring [`reconcile`] but with a spawn fn keyed only on the id,
    /// so the tests don't need an [`AppState`] / container resolution.
    fn reconcile_with<F>(
        railways: &[Railway],
        running: &mut HashMap<Uuid, JoinHandle<()>>,
        spawn_one: &F,
    ) where
        F: Fn(Uuid) -> JoinHandle<()>,
    {
        let current: std::collections::HashSet<Uuid> = railways.iter().map(|r| r.id).collect();
        running.retain(|id, handle| {
            if !current.contains(id) {
                handle.abort();
                return false;
            }
            !handle.is_finished()
        });
        for railway in railways {
            if running.contains_key(&railway.id) {
                continue;
            }
            running.insert(railway.id, spawn_one(railway.id));
        }
    }
}
