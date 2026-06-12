//! Self-update: check whether a newer commit is on the deployed branch, and,
//! when asked, rebuild the stack from the latest source.
//!
//! The API container can drive the host Docker daemon (via the mounted socket)
//! but has no git or repo source of its own, so the actual update runs in a
//! short-lived **updater container**: it bind-mounts the host repo, runs
//! `git pull` (honoring the checked-out branch), then `docker compose up -d
//! --build`. Because that container is started directly (not part of the compose
//! project), it survives the API container being rebuilt out from under it.
//!
//! The version check is lighter: it asks GitHub for the branch's latest commit
//! and compares it to the commit baked into the running image (`GIT_SHA`).

use std::time::Duration;

use eyre::{eyre, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use tracing::{info, warn};

use crate::state::{AppState, UpdateStatus};

/// How often the background check refreshes the cached status.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The image the updater container runs from: the Docker CLI (Alpine), to which
/// we add git + the compose plugin at start. It drives the host daemon to rebuild.
const UPDATER_IMAGE_REPO: &str = "docker";
const UPDATER_IMAGE_TAG: &str = "cli";

/// Spawns the background loop that re-checks for updates every hour (and once at
/// boot), keeping [`AppState`]'s cached status fresh whenever the UI opens.
pub fn spawn_check_loop(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = check(&state).await {
                warn!(error = %error, "scheduled update check failed");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// Checks GitHub for a newer commit on the deployed branch and refreshes the
/// cached status. Returns the new status. Never errors the caller for a transient
/// GitHub hiccup; that is recorded on the status instead.
pub async fn check(state: &AppState) -> Result<UpdateStatus> {
    let cfg = &state.update;
    let mut status = state.update_status();
    status.current_sha.clone_from(&cfg.git_sha);
    status.current_branch.clone_from(&cfg.git_branch);
    status.configured = !cfg.host_repo_dir.trim().is_empty();
    status.checked_at = Some(chrono::Utc::now());
    status.error = None;
    status.update_available = false;
    status.latest_sha = None;

    if cfg.git_sha.is_empty() || cfg.git_sha == "unknown" {
        status.error = Some(
            "This build isn't version-stamped, so updates can't be detected. \
             Deploy with scripts/start.sh so the commit is baked in."
                .to_string(),
        );
        state.set_update_status(status.clone());
        return Ok(status);
    }

    let Some((owner, repo)) = cfg.repo.split_once('/') else {
        status.error = Some(format!("update repo '{}' is not owner/repo", cfg.repo));
        state.set_update_status(status.clone());
        return Ok(status);
    };
    let branch = match cfg.git_branch.as_str() {
        "" | "unknown" => "main",
        branch => branch,
    };

    match latest_commit_sha(state, owner, repo, branch).await {
        Ok(latest) => {
            status.update_available = !latest.eq_ignore_ascii_case(&cfg.git_sha);
            status.latest_sha = Some(latest);
        }
        Err(error) => status.error = Some(format!("update check failed: {error}")),
    }
    state.set_update_status(status.clone());
    Ok(status)
}

/// The latest commit SHA on `branch` of `owner/repo`, via the stored GitHub token.
async fn latest_commit_sha(
    state: &AppState,
    owner: &str,
    repo: &str,
    branch: &str,
) -> Result<String> {
    #[derive(Deserialize)]
    struct CommitRef {
        sha: String,
    }
    let octo = state.github().await?;
    let commit: CommitRef = octo
        .get(
            format!("/repos/{owner}/{repo}/commits/{branch}"),
            None::<&()>,
        )
        .await
        .wrap_err("failed to read the branch's latest commit")?;
    Ok(commit.sha)
}

/// Launches the detached updater container that pulls the latest source and
/// rebuilds the stack. The caller has already confirmed the agent is paused and
/// idle. Marks the cached status as `updating` so the UI reflects it until the
/// new build replaces this process.
pub async fn trigger(state: &AppState) -> Result<()> {
    let cfg = &state.update;
    if cfg.host_repo_dir.trim().is_empty() {
        return Err(eyre!(
            "HOST_REPO_DIR is not set, so the API can't reach the repo to rebuild it. \
             Set it in .env (the host path to the cloned repo) and restart."
        ));
    }

    let docker = state.workspace.docker();
    pull_image(docker).await?;

    // git+ssh are required; the compose plugin is usually already in docker:cli,
    // so its install is best-effort. Then pull (honoring the branch) and rebuild.
    let script = "set -e\n\
        apk add --no-cache git openssh-client >/dev/null 2>&1\n\
        apk add --no-cache docker-cli-compose >/dev/null 2>&1 || true\n\
        git config --global --add safe.directory /repo\n\
        export GIT_SSH_COMMAND='ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'\n\
        cd /repo\n\
        git pull --ff-only\n\
        export GIT_SHA=\"$(git rev-parse HEAD)\"\n\
        export GIT_BRANCH=\"$(git rev-parse --abbrev-ref HEAD)\"\n\
        docker compose up -d --build\n"
        .to_string();

    let mut binds = vec![
        format!("{}:/repo", cfg.host_repo_dir),
        format!("{}:/var/run/docker.sock", cfg.docker_socket),
    ];
    if !cfg.ssh_home.trim().is_empty() {
        binds.push(format!("{}:/root/.ssh:ro", cfg.ssh_home));
    }

    let config: bollard::container::Config<String> = bollard::container::Config {
        image: Some(format!("{UPDATER_IMAGE_REPO}:{UPDATER_IMAGE_TAG}")),
        cmd: Some(vec!["sh".to_string(), "-c".to_string(), script]),
        host_config: Some(bollard::models::HostConfig {
            binds: Some(binds),
            // The updater removes itself once the rebuild finishes.
            auto_remove: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };

    let created = docker
        .create_container::<String, String>(None, config)
        .await
        .wrap_err("failed to create the updater container")?;
    docker
        .start_container::<String>(&created.id, None)
        .await
        .wrap_err("failed to start the updater container")?;

    let mut status = state.update_status();
    status.updating = true;
    state.set_update_status(status);
    info!(container = %created.id, "self-update launched (git pull + compose rebuild)");
    Ok(())
}

/// Pulls the updater image so `create_container` finds it. Pulling an
/// already-present image just refreshes it.
async fn pull_image(docker: &bollard::Docker) -> Result<()> {
    let options = bollard::image::CreateImageOptions {
        from_image: UPDATER_IMAGE_REPO,
        tag: UPDATER_IMAGE_TAG,
        ..Default::default()
    };
    let mut stream = docker.create_image(Some(options), None, None);
    while let Some(item) = stream.next().await {
        item.wrap_err("failed to pull the updater image")?;
    }
    Ok(())
}
