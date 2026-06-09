//! Lifecycle control of the long-lived agent workspace container.
//!
//! The workspace is a powerful-but-dumb sandbox: the API is the only
//! orchestrator and reaches in via `docker exec`. This module wraps the handful
//! of Docker operations we need: running commands and capturing their output,
//! restarting, and a full recreate that re-runs setup scripts.
//!
//! Following M-SERVICES-CLONE, [`Workspace`] is cheaply cloneable (the inner
//! `bollard::Docker` is already `Arc`-backed).

use bollard::container::{LogOutput, RemoveContainerOptions};
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
        let exec = self
            .docker
            .create_exec(
                &self.container,
                CreateExecOptions {
                    cmd: Some(command),
                    working_dir: Some(working_dir.to_string()),
                    env: Some(env),
                    // Run as the non-root agent user (sudo-capable, docker-group).
                    user: Some("node".to_string()),
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
        let config = bollard::container::Config {
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

        let options = bollard::container::CreateContainerOptions {
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
}
