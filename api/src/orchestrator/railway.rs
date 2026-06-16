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
//! ## The execution seam (the container lifecycle, issue #203)
//!
//! Every railway has its own workspace container, Claude session, and helper env.
//! [`RailwayHandle`] is the seam the turn runner targets: it resolves the
//! container name and reads/writes the session for a given railway.
//!
//! `main` keeps using the single, compose-managed workspace container (always on,
//! never created, started, or stopped here), so its behavior is **byte-for-byte
//! identical** to before railways existed. A non-`main` railway runs in its own
//! container named `{workspace}-railway-{id}`, created lazily the first time the
//! railway has actionable work ([`RailwayHandle::ensure_running`], called by the
//! agent loop before it execs a turn) and idle-STOPPED by the [`reaper`] after a
//! period with no work, so a later restart is fast and keeps the clones plus the
//! persisted session. The lifecycle state machine
//! (stopped/starting/running/stopping) is maintained on the railway row as the
//! container transitions.
//!
//! For the session, `main` keeps `settings.current_session_id` as the source of
//! truth (so the existing reset and persist paths are untouched) and mirrors it
//! onto its railway row; a non-`main` railway keeps its session only on the row.

use std::collections::HashMap;

use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::models::{Railway, RailwayState};
use crate::db::queries;
use crate::docker::ContainerState;
use crate::state::AppState;

/// How often the supervisor reconciles running agent loops against the `railways`
/// table. Brisk enough that a railway created or deleted in the UI starts or stops
/// working within a few seconds, without polling so tightly it churns the DB.
const SUPERVISE_POLL: Duration = Duration::from_secs(5);

/// How long a non-`main` railway may sit with no work before the reaper idle-STOPS
/// its container.
///
/// Long enough that a lane working a batch of related issues is not torn down
/// between tasks (provisioning a fresh start, even with the clones cached, costs a
/// config-repo pull and setup re-run), short enough that an idle lane frees its
/// container's memory within the hour. The stop preserves the `/workspace` volume,
/// so the restart only re-runs setup, never re-clones. `main` is never reaped.
const RAILWAY_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How often the reaper scans for idle non-`main` railways to stop. Coarse: the
/// idle timeout is in tens of minutes, so a minute of latency on the stop is fine.
const REAPER_POLL: Duration = Duration::from_secs(60);

/// The container name for a railway, given the base workspace container name.
///
/// `main` uses the workspace container as-is (the compose-managed one); every
/// other railway gets a stable, unique `{workspace}-railway-{id}` peer. The id (a
/// UUID) keeps the name collision-free and stable across renames, which a
/// human-typed slug could not guarantee. Pure, so the naming is unit-testable.
pub fn railway_container_name(workspace: &str, railway: &Railway) -> String {
    if railway.is_main {
        workspace.to_string()
    } else {
        format!("{workspace}-railway-{}", railway.id)
    }
}

/// The execution context for one railway: its container and Claude session.
///
/// This is the single place the rest of the orchestrator asks "which container do
/// I exec into?" and "what session do I resume / persist?" for a railway, and (for
/// a non-`main` railway) where the container lifecycle is driven.
#[derive(Debug, Clone)]
pub struct RailwayHandle {
    pub id: Uuid,
    pub is_main: bool,
    /// The container this railway's turns exec into: `main`'s compose-managed
    /// workspace, or a non-`main` railway's own `{workspace}-railway-{id}`.
    container: String,
}

impl RailwayHandle {
    /// Builds the handle for `railway`, resolving its container name against the
    /// workspace. `main` targets the existing single workspace container so its
    /// behavior is unchanged; another railway targets its own per-railway container
    /// (created and managed lazily by [`Self::ensure_running`]).
    pub fn new(state: &AppState, railway: &Railway) -> Self {
        Self {
            id: railway.id,
            is_main: railway.is_main,
            container: railway_container_name(state.workspace.container(), railway),
        }
    }

    /// The name of the container this railway's turns exec into.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Ensures this railway's container is running and provisioned before a turn
    /// execs into it. Called by the agent loop the moment it has work for the lane.
    ///
    /// For `main` this is a deliberate no-op: its container is the always-on,
    /// compose-managed workspace, so main-only deployments never enter the create /
    /// start / provision path and behave exactly as before. For a non-`main`
    /// railway it lazily creates the container (cloning the workspace spec) on first
    /// use, starts it if stopped, and (re)provisions it (config repo, network
    /// policy, env setup, the railway's repos), driving the lifecycle state through
    /// `starting` -> `running`.
    ///
    /// # Errors
    /// Propagates a Docker failure (create/start/inspect) or a provisioning failure;
    /// on error the railway is left `stopped` so the next attempt starts cleanly.
    pub async fn ensure_running(&self, state: &AppState) -> eyre::Result<()> {
        // `main` is the compose-managed workspace: always on, never created or
        // stopped here. This early return is what keeps main-only behavior identical.
        if self.is_main {
            return Ok(());
        }

        let observed = state.workspace.container_state(&self.container).await?;
        // Already up and provisioned from an earlier turn: nothing to do. The reaper
        // only ever STOPS the container (never removes it), and a stop flips the
        // lifecycle state away from `running`, so an observed-running container that
        // we marked `running` was provisioned by us and is ready.
        if observed == ContainerState::Running {
            let railway = queries::get_railway(&state.db, self.id).await?;
            if railway.map(|r| r.lifecycle_state) == Some(RailwayState::Running) {
                return Ok(());
            }
        }

        queries::set_railway_lifecycle_state(&state.db, self.id, RailwayState::Starting).await?;
        state.notify_board();

        // Run the bring-up steps, resetting the lifecycle to `stopped` on any
        // failure so a crashed provision never strands the railway in `starting`
        // (which would block the reaper and mislead the UI); the next attempt then
        // starts cleanly from a known state.
        match self.bring_up(state, observed).await {
            Ok(()) => {
                queries::set_railway_lifecycle_state(&state.db, self.id, RailwayState::Running)
                    .await?;
                state.notify_board();
                info!(railway_id = %self.id, "railway container running and provisioned");
                Ok(())
            }
            Err(error) => {
                let _ =
                    queries::set_railway_lifecycle_state(&state.db, self.id, RailwayState::Stopped)
                        .await;
                state.notify_board();
                Err(error)
            }
        }
    }

    /// Creates (if absent), starts (if not running), and provisions the container.
    /// Split out so [`Self::ensure_running`] can wrap it in a single error path that
    /// resets the lifecycle state. `observed` is the state seen before bring-up.
    async fn bring_up(&self, state: &AppState, observed: ContainerState) -> eyre::Result<()> {
        // Bring the container up: create it (cloning the workspace spec) if it has
        // never existed, then start it if it is not already running.
        if observed == ContainerState::Absent {
            info!(railway_id = %self.id, container = %self.container, "creating railway container");
            state
                .workspace
                .create_railway_container(&self.container)
                .await?;
        }
        if observed != ContainerState::Running {
            state.workspace.start_container(&self.container).await?;
        }

        // Provision into the now-running container (config repo, network policy,
        // env setup, this railway's repos). On a restart after an idle-stop the
        // `/workspace` volume still holds the clones, so this only refreshes them
        // and re-runs setup rather than re-cloning from scratch.
        super::provision::provision_workspace(state, self).await
    }

    /// Idle-STOPS this railway's container (stopped, not removed), preserving its
    /// clones and persisted session for a fast restart. A no-op for `main`.
    ///
    /// # Errors
    /// Propagates a Docker stop failure.
    pub async fn stop(&self, state: &AppState) -> eyre::Result<()> {
        if self.is_main {
            return Ok(());
        }
        queries::set_railway_lifecycle_state(&state.db, self.id, RailwayState::Stopping).await?;
        state.notify_board();
        info!(railway_id = %self.id, container = %self.container, "idle-stopping railway container");
        state.workspace.stop_container(&self.container).await?;
        queries::set_railway_lifecycle_state(&state.db, self.id, RailwayState::Stopped).await?;
        state.notify_board();
        Ok(())
    }
}

/// Builds the [`RailwayHandle`] for the undeletable `main` railway, for the global
/// code paths (startup provision, the manual workspace endpoints) that always
/// target the compose-managed workspace container.
pub async fn handle_for_main(state: &AppState) -> eyre::Result<RailwayHandle> {
    let main = queries::get_main_railway(&state.db).await?;
    Ok(RailwayHandle::new(state, &main))
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

// --- Idle-stop reaper --------------------------------------------------------

/// Whether a non-`main` railway's container should be idle-stopped now.
///
/// A railway is reapable when its container is up, no turn is running on it, and
/// its most recent task activity is older than [`RAILWAY_IDLE_TIMEOUT`] (or it has
/// no activity at all, e.g. a lane that was started but never worked anything).
/// `main` and a railway that is not `running` are never reaped. Pure, taking the
/// elapsed idle time as a `Duration`, so the decision is unit-testable without a
/// clock or a database.
fn should_reap(
    is_main: bool,
    lifecycle_state: RailwayState,
    has_running_turn: bool,
    idle_for: Duration,
) -> bool {
    if is_main || lifecycle_state != RailwayState::Running || has_running_turn {
        return false;
    }
    idle_for >= RAILWAY_IDLE_TIMEOUT
}

/// The idle-stop reaper: a single global loop that stops the container of any
/// non-`main` railway that has had no work for [`RAILWAY_IDLE_TIMEOUT`].
///
/// Stopping (not removing) keeps the railway's `/workspace` volume, so the next
/// [`RailwayHandle::ensure_running`] only re-runs setup, never re-clones. `main` is
/// the always-on compose workspace and is skipped entirely, so a main-only
/// deployment's reaper finds nothing to do and never touches its container.
pub async fn reaper(state: AppState) {
    loop {
        sleep(REAPER_POLL).await;
        if let Err(error) = reap_idle_railways(&state).await {
            warn!(error = %error, "railway idle-stop reaper failed");
        }
    }
}

/// One reaper pass: idle-stop every eligible non-`main` railway.
async fn reap_idle_railways(state: &AppState) -> eyre::Result<()> {
    let now = chrono::Utc::now();
    for railway in queries::list_railways(&state.db).await? {
        if railway.is_main {
            continue;
        }
        let has_running_turn = queries::railway_has_running_turn(&state.db, railway.id).await?;
        // No recorded activity counts as "idle forever", so a railway started but
        // never worked is reaped once it crosses the timeout from when we noticed.
        let idle_for = match queries::railway_last_activity(&state.db, railway.id).await? {
            Some(last) => (now - last).to_std().unwrap_or(Duration::ZERO),
            None => RAILWAY_IDLE_TIMEOUT,
        };
        if should_reap(
            railway.is_main,
            railway.lifecycle_state,
            has_running_turn,
            idle_for,
        ) {
            let handle = RailwayHandle::new(state, &railway);
            if let Err(error) = handle.stop(state).await {
                warn!(error = %error, railway_id = %railway.id, "failed to idle-stop a railway container");
            }
        }
    }
    Ok(())
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

    #[test]
    fn main_uses_the_workspace_container_unchanged() {
        // `main` must target the existing compose workspace container verbatim, so
        // a main-only deployment execs into exactly the same place as before.
        let main = railway(Uuid::nil(), true);
        assert_eq!(
            railway_container_name("seraphim-workspace", &main),
            "seraphim-workspace"
        );
    }

    #[test]
    fn non_main_gets_a_stable_unique_peer_name() {
        // A non-`main` railway gets a derived, id-qualified peer name so two
        // railways never collide and the name is stable across renames.
        let id = Uuid::parse_str("00000000-0000-0000-0000-0000000000ab").unwrap();
        let other = railway(id, false);
        assert_eq!(
            railway_container_name("seraphim-workspace", &other),
            "seraphim-workspace-railway-00000000-0000-0000-0000-0000000000ab"
        );
    }

    #[test]
    fn reaper_never_touches_main() {
        // `main` is the always-on compose workspace; it is never idle-stopped, even
        // when reported running, idle, and with no turn. This is what keeps a
        // main-only deployment's reaper inert.
        assert!(!should_reap(
            true,
            RailwayState::Running,
            false,
            RAILWAY_IDLE_TIMEOUT * 10,
        ));
    }

    #[test]
    fn reaper_stops_an_idle_running_railway() {
        // A running, turn-free, long-idle non-`main` railway is reaped.
        assert!(should_reap(
            false,
            RailwayState::Running,
            false,
            RAILWAY_IDLE_TIMEOUT,
        ));
        assert!(should_reap(
            false,
            RailwayState::Running,
            false,
            RAILWAY_IDLE_TIMEOUT * 2,
        ));
    }

    #[test]
    fn reaper_spares_busy_or_fresh_or_down_railways() {
        // A live turn pins the container open regardless of idle time.
        assert!(!should_reap(
            false,
            RailwayState::Running,
            true,
            RAILWAY_IDLE_TIMEOUT * 5,
        ));
        // Not yet idle long enough: leave it up.
        assert!(!should_reap(
            false,
            RailwayState::Running,
            false,
            RAILWAY_IDLE_TIMEOUT / 2,
        ));
        // Already stopped / mid-transition: nothing to reap.
        for state in [
            RailwayState::Stopped,
            RailwayState::Starting,
            RailwayState::Stopping,
        ] {
            assert!(!should_reap(false, state, false, RAILWAY_IDLE_TIMEOUT * 5));
        }
    }
}
