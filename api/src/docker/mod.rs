//! Lifecycle control of the long-lived agent workspace container.
//!
//! The workspace is a powerful-but-dumb sandbox: the API is the only
//! orchestrator and reaches in via `docker exec`. This module wraps the handful
//! of Docker operations we need: running commands and capturing their output,
//! restarting, and a full recreate that re-runs setup scripts.
//!
//! Following M-SERVICES-CLONE, [`Workspace`] is cheaply cloneable (the inner
//! `bollard::Docker` is already `Arc`-backed).

use bollard::container::{
    Config, CreateContainerOptions, LogOutput, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use eyre::{eyre, Context, Result};
use futures::StreamExt;

/// Handle to the agent's workspace container on the host Docker daemon.
#[derive(Debug, Clone)]
pub struct Workspace {
    docker: Docker,
    container: String,
    image_tag: String,
}

/// The running state of a container, as far as the railway lifecycle cares.
///
/// A railway's per-railway container (issue #203) is created lazily and idle-
/// STOPPED (never removed), so these three are the only states the orchestrator
/// distinguishes when deciding whether to create, start, or leave it be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// No container by that name exists yet (it has never been created).
    Absent,
    /// The container exists but is stopped (idle-stopped, or never started).
    Stopped,
    /// The container exists and is running.
    Running,
}

/// The captured result of a one-shot command run in the workspace.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub exit_code: i64,
    pub output: String,
}

impl ExecOutput {
    /// True when the command exited zero.
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}

impl Workspace {
    /// Connects to the local Docker daemon (the mounted host socket).
    pub fn connect(container: impl Into<String>, image_tag: impl Into<String>) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .wrap_err("failed to connect to the Docker daemon")?;
        Ok(Self {
            docker,
            container: container.into(),
            image_tag: image_tag.into(),
        })
    }

    /// The underlying Docker handle, for streaming execs (Claude turns).
    pub fn docker(&self) -> &Docker {
        &self.docker
    }

    /// The workspace container name.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Runs a command in the workspace, capturing combined stdout/stderr and the
    /// exit code. Used for git, setup scripts, and other deterministic steps.
    pub async fn exec_capture(
        &self,
        working_dir: &str,
        command: Vec<String>,
        env: Vec<String>,
    ) -> Result<ExecOutput> {
        self.exec_capture_in(&self.container, working_dir, command, env)
            .await
    }

    /// Like [`Self::exec_capture`] but targets a named container, for per-railway
    /// containers (issue #202): the railway carries which container to exec into.
    /// With only the `main` railway, `container` is the default workspace, so this
    /// matches [`Self::exec_capture`].
    pub async fn exec_capture_in(
        &self,
        container: &str,
        working_dir: &str,
        command: Vec<String>,
        env: Vec<String>,
    ) -> Result<ExecOutput> {
        let exec = self
            .docker
            .create_exec(
                container,
                CreateExecOptions {
                    cmd: Some(command),
                    working_dir: Some(working_dir.to_string()),
                    env: Some(env),
                    // Run as the non-root agent user (the universal devcontainer
                    // image's `codespace`; sudo-capable).
                    user: Some("codespace".to_string()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .wrap_err("failed to create docker exec")?;

        let mut collected = String::new();
        if let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec.id, None).await?
        {
            while let Some(chunk) = output.next().await {
                match chunk? {
                    LogOutput::StdOut { message }
                    | LogOutput::StdErr { message }
                    | LogOutput::Console { message } => {
                        collected.push_str(&String::from_utf8_lossy(&message));
                    }
                    LogOutput::StdIn { .. } => {}
                }
            }
        }

        let inspect = self.docker.inspect_exec(&exec.id).await?;
        let exit_code = inspect.exit_code.unwrap_or(-1);

        Ok(ExecOutput {
            exit_code,
            output: collected,
        })
    }

    /// Restarts the workspace container in place (volumes preserved).
    pub async fn restart(&self) -> Result<()> {
        self.docker
            .restart_container(&self.container, None)
            .await
            .wrap_err("failed to restart workspace container")
    }

    /// Recreates the container from its current image and config, preserving the
    /// persistent `/workspace` volume. Mirrors the existing container's spec via
    /// `inspect`, so it stays in sync with whatever compose defined.
    ///
    /// Re-running setup scripts is the caller's job after this returns.
    pub async fn recreate(&self) -> Result<()> {
        let existing = self
            .docker
            .inspect_container(&self.container, None)
            .await
            .wrap_err("failed to inspect workspace container")?;

        let inspected = existing
            .config
            .ok_or_else(|| eyre!("workspace container has no config to recreate from"))?;

        // Rebuild a create-config from the inspected fields, carrying the host
        // config (mounts, socket, restart policy) across the recreate. `inspect`
        // and `create` use different config structs, so we map explicitly.
        let config = Config {
            image: Some(self.image_tag.clone()),
            cmd: inspected.cmd,
            entrypoint: inspected.entrypoint,
            env: inspected.env,
            working_dir: inspected.working_dir,
            labels: inspected.labels,
            host_config: existing.host_config,
            ..Default::default()
        };

        self.docker
            .remove_container(
                &self.container,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .wrap_err("failed to remove workspace container")?;

        let options = CreateContainerOptions {
            name: self.container.clone(),
            platform: None,
        };
        self.docker
            .create_container(Some(options), config)
            .await
            .wrap_err("failed to create workspace container")?;
        self.docker
            .start_container::<String>(&self.container, None)
            .await
            .wrap_err("failed to start recreated workspace container")?;

        Ok(())
    }

    // --- Per-railway container lifecycle (issue #203) ------------------------
    //
    // A non-`main` railway runs in its own container, created lazily on first
    // work and idle-STOPPED (never removed) so a restart keeps its clones and
    // session. `main` keeps using the compose-managed workspace container and
    // never touches any of the methods below, so its behavior is unchanged.

    /// Reports whether `container` is absent, stopped, or running.
    ///
    /// A missing container is [`ContainerState::Absent`] rather than an error, so
    /// the lazy-start path can tell "never created" apart from "created but down".
    pub async fn container_state(&self, container: &str) -> Result<ContainerState> {
        match self.docker.inspect_container(container, None).await {
            Ok(info) => {
                let running = info.state.and_then(|state| state.running).unwrap_or(false);
                Ok(if running {
                    ContainerState::Running
                } else {
                    ContainerState::Stopped
                })
            }
            // bollard surfaces a missing container as a 404 from the daemon; treat
            // only that as "absent" and propagate any other failure.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(ContainerState::Absent),
            Err(error) => {
                Err(error).wrap_err_with(|| format!("failed to inspect container {container}"))
            }
        }
    }

    /// Creates a new railway container named `container`, cloning the existing
    /// workspace container's spec (image, env, mounts, socket, network) so it is a
    /// peer sandbox differing only in name. Does not start it; the caller starts
    /// and provisions it.
    ///
    /// The spec is mirrored from the compose-managed workspace via `inspect`, the
    /// same approach [`Self::recreate`] uses, so a per-railway container stays in
    /// sync with whatever compose defined for the workspace without re-declaring it.
    pub async fn create_railway_container(&self, container: &str) -> Result<()> {
        let template = self
            .docker
            .inspect_container(&self.container, None)
            .await
            .wrap_err("failed to inspect the workspace container to clone its spec")?;

        let config = template
            .config
            .ok_or_else(|| eyre!("workspace container has no config to clone from"))?;

        // Carry the host config (mounts incl. /workspace volume, the Docker socket,
        // ~/.ssh, the seraphim network, restart policy) so the railway container is
        // a faithful peer of the workspace. `inspect` and `create` use different
        // config structs, so map the fields we rely on explicitly.
        let create = Config {
            image: Some(self.image_tag.clone()),
            cmd: config.cmd,
            entrypoint: config.entrypoint,
            env: config.env,
            working_dir: config.working_dir,
            labels: config.labels,
            host_config: template.host_config,
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container.to_string(),
            platform: None,
        };
        self.docker
            .create_container(Some(options), create)
            .await
            .wrap_err_with(|| format!("failed to create railway container {container}"))?;
        Ok(())
    }

    /// Starts an existing (created or idle-stopped) container.
    pub async fn start_container(&self, container: &str) -> Result<()> {
        self.docker
            .start_container(container, None::<StartContainerOptions<String>>)
            .await
            .wrap_err_with(|| format!("failed to start container {container}"))
    }

    /// Stops a running container without removing it, so its clones and persisted
    /// session survive for a fast later restart. A `None` timeout uses Docker's
    /// default grace period before the kill.
    pub async fn stop_container(&self, container: &str) -> Result<()> {
        self.docker
            .stop_container(container, None::<StopContainerOptions>)
            .await
            .wrap_err_with(|| format!("failed to stop container {container}"))
    }

    /// Force-removes a container (and its writable layer), tearing it down for
    /// good. Used when a railway is deleted: its lane no longer exists, so the
    /// per-railway container is removed rather than merely idle-stopped.
    ///
    /// A missing container is treated as success, so a delete is idempotent even
    /// if the container was never created (a railway that never ran any work).
    pub async fn remove_container(&self, container: &str) -> Result<()> {
        match self
            .docker
            .remove_container(
                container,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(()) => Ok(()),
            // Already gone: nothing to remove, treat as success for idempotency.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(error) => {
                Err(error).wrap_err_with(|| format!("failed to remove container {container}"))
            }
        }
    }
}
