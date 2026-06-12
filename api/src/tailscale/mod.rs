//! Management of the Tailscale sidecar container (issue #52).
//!
//! Tailscale runs as its own container (`seraphim-tailscale`) that exposes the UI
//! over the tailnet with `tailscale serve`. The API already drives other
//! containers through the host Docker socket; this module reaches into the
//! Tailscale container the same way to read its status (the serve URL, online
//! state, the login URL when auth is needed) and to run the handful of management
//! commands the operator wants from the UI: connect, disconnect, re-authenticate,
//! and restart.
//!
//! Everything is best-effort and tolerant: the Tailscale CLI's JSON shape is read
//! leniently (unknown fields ignored, missing fields defaulted), and commands are
//! bounded by a timeout so a request never hangs on a node that is waiting for
//! interactive login.

use std::time::Duration;

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use eyre::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::time::{timeout, timeout_at, Instant};

use crate::docker::ExecOutput;

/// Upper bound on a single Tailscale command. Generous, but it keeps a request
/// from hanging forever if the daemon stalls.
const EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// How long we wait for `tailscale login` to print its URL before returning. The
/// command keeps running in the container afterward, so the pending login (and
/// its URL) stay valid for the operator to finish in the browser.
const REAUTH_WAIT: Duration = Duration::from_secs(8);

/// Handle to the Tailscale sidecar container on the host Docker daemon.
#[derive(Debug, Clone)]
pub struct Tailscale {
    docker: Docker,
    container: String,
}

/// The Tailscale node's state as the UI sees it.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a flat status DTO of independent node-state flags for the UI"
)]
#[derive(Debug, Clone, Default, Serialize)]
pub struct TailscaleStatus {
    /// Whether the sidecar container itself is running. When false, the rest is
    /// empty and the management actions are unavailable.
    pub container_running: bool,
    /// The daemon's backend state, e.g. `Running`, `Stopped`, `NeedsLogin`.
    pub backend_state: String,
    /// True when the node is connected to the tailnet (`backend_state == Running`).
    pub connected: bool,
    /// Whether this node currently shows as online in the tailnet.
    pub online: bool,
    /// True when the node needs the operator to authenticate it.
    pub needs_login: bool,
    /// The node's short hostname on the tailnet.
    pub hostname: String,
    /// The node's `MagicDNS` name (trailing dot stripped), e.g. `seraphim.tailnet.ts.net`.
    pub dns_name: String,
    /// The HTTPS URL the UI is reachable at over the tailnet, when known.
    pub url: Option<String>,
    /// The tailnet's name (e.g. the owning account), when known.
    pub tailnet: String,
    /// The node's tailnet IPs.
    pub tailscale_ips: Vec<String>,
    /// A pending interactive-login URL the operator should visit, when one exists.
    pub auth_url: Option<String>,
    /// Whether `tailscale serve` is actively proxying (the UI is being hosted).
    pub serve_active: bool,
}

impl Tailscale {
    /// Connects to the local Docker daemon (the mounted host socket).
    pub fn connect(container: impl Into<String>) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .wrap_err("failed to connect to the Docker daemon")?;
        Ok(Self {
            docker,
            container: container.into(),
        })
    }

    /// Whether the Tailscale container is currently running.
    async fn is_running(&self) -> bool {
        match self.docker.inspect_container(&self.container, None).await {
            Ok(info) => info.state.and_then(|state| state.running).unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Runs `tailscale <args>` in the container (as root, which the CLI needs to
    /// reach the local daemon socket), capturing combined stdout/stderr + the exit
    /// code. Bounded by [`EXEC_TIMEOUT`].
    async fn run(&self, args: &[&str]) -> Result<ExecOutput> {
        let mut cmd = vec!["tailscale".to_string()];
        cmd.extend(args.iter().map(|arg| (*arg).to_string()));

        let exec = self
            .docker
            .create_exec(
                &self.container,
                CreateExecOptions {
                    cmd: Some(cmd),
                    user: Some("root".to_string()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .wrap_err("failed to create tailscale exec")?;

        let output = timeout(EXEC_TIMEOUT, self.drain(&exec.id))
            .await
            .wrap_err("the tailscale command timed out")??;
        let inspect = self.docker.inspect_exec(&exec.id).await?;
        Ok(ExecOutput {
            exit_code: inspect.exit_code.unwrap_or(-1),
            output,
        })
    }

    /// Reads an exec's combined output to completion.
    async fn drain(&self, exec_id: &str) -> Result<String> {
        let mut collected = String::new();
        if let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(exec_id, None).await?
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
        Ok(collected)
    }

    /// The full node status, including the serve URL and any pending login URL.
    pub async fn status(&self) -> Result<TailscaleStatus> {
        if !self.is_running().await {
            return Ok(TailscaleStatus::default());
        }
        let status = self.run(&["status", "--json"]).await?;
        // Serve status is a nice-to-have; never let it fail the whole status read.
        let serve_active = match self.run(&["serve", "status", "--json"]).await {
            Ok(serve) => serve_is_active(&serve.output),
            Err(_) => false,
        };
        Ok(parse_status(&status.output, true, serve_active))
    }

    /// Connects the node to the tailnet (`tailscale up`). After a prior `down`, this
    /// reconnects with the node's existing preferences (it does not reset them).
    pub async fn up(&self) -> Result<ExecOutput> {
        self.run(&["up"]).await
    }

    /// Disconnects the node from the tailnet (`tailscale down`). The container keeps
    /// running, so the node can be brought back up without a restart.
    pub async fn down(&self) -> Result<ExecOutput> {
        self.run(&["down"]).await
    }

    /// Starts an interactive login and returns the URL the operator must visit.
    ///
    /// `tailscale login` blocks until the user finishes authenticating, so we only
    /// wait long enough to capture the printed URL ([`REAUTH_WAIT`]) and then leave
    /// it running: the daemon keeps the pending login (and its URL) alive until the
    /// browser visit completes. `force` re-authenticates even an already-logged-in
    /// node, which is how the operator gets a fresh login link.
    pub async fn reauth(&self, force: bool) -> Result<Option<String>> {
        let mut cmd = vec!["tailscale".to_string(), "login".to_string()];
        if force {
            cmd.push("--force-reauth".to_string());
        }
        let exec = self
            .docker
            .create_exec(
                &self.container,
                CreateExecOptions {
                    cmd: Some(cmd),
                    user: Some("root".to_string()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .wrap_err("failed to start tailscale login")?;

        let mut collected = String::new();
        if let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec.id, None).await?
        {
            let deadline = Instant::now() + REAUTH_WAIT;
            // Read until the login URL appears, the command exits, or our wait is up
            // (whichever comes first); login keeps running in the container after.
            while let Ok(Some(chunk)) = timeout_at(deadline, output.next()).await {
                match chunk {
                    Ok(
                        LogOutput::StdOut { message }
                        | LogOutput::StdErr { message }
                        | LogOutput::Console { message },
                    ) => collected.push_str(&String::from_utf8_lossy(&message)),
                    Ok(LogOutput::StdIn { .. }) => {}
                    Err(_) => break,
                }
                if extract_login_url(&collected).is_some() {
                    break;
                }
            }
        }
        Ok(extract_login_url(&collected))
    }

    /// Restarts the Tailscale container in place (re-runs its entrypoint, which
    /// re-applies the serve config and any auth key). Volumes are preserved.
    pub async fn restart(&self) -> Result<()> {
        self.docker
            .restart_container(&self.container, None)
            .await
            .wrap_err("failed to restart the tailscale container")
    }
}

/// `tailscale status --json`, read leniently into just the fields the UI needs.
#[derive(Debug, Default, Deserialize)]
struct RawStatus {
    #[serde(rename = "BackendState")]
    backend_state: Option<String>,
    #[serde(rename = "AuthURL")]
    auth_url: Option<String>,
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Option<Vec<String>>,
    #[serde(rename = "Self")]
    self_node: Option<RawNode>,
    #[serde(rename = "CurrentTailnet")]
    tailnet: Option<RawTailnet>,
}

#[derive(Debug, Default, Deserialize)]
struct RawNode {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
    #[serde(rename = "HostName")]
    host_name: Option<String>,
    #[serde(rename = "Online")]
    online: Option<bool>,
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTailnet {
    #[serde(rename = "Name")]
    name: Option<String>,
}

/// Maps `tailscale status --json` plus the externally-determined container/serve
/// state into the UI status. Pure, so it is unit-tested against real CLI output.
fn parse_status(json: &str, container_running: bool, serve_active: bool) -> TailscaleStatus {
    let raw: RawStatus = serde_json::from_str(json).unwrap_or_default();
    let backend_state = raw.backend_state.unwrap_or_default();
    let node = raw.self_node.unwrap_or_default();

    let dns_name = node
        .dns_name
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_string();
    let url = (!dns_name.is_empty()).then(|| format!("https://{dns_name}"));

    TailscaleStatus {
        container_running,
        connected: backend_state == "Running",
        needs_login: matches!(backend_state.as_str(), "NeedsLogin" | "NoState"),
        online: node.online.unwrap_or(false),
        hostname: node.host_name.unwrap_or_default(),
        dns_name,
        url,
        tailnet: raw
            .tailnet
            .and_then(|tailnet| tailnet.name)
            .unwrap_or_default(),
        tailscale_ips: node.tailscale_ips.or(raw.tailscale_ips).unwrap_or_default(),
        auth_url: raw.auth_url.filter(|url| !url.trim().is_empty()),
        serve_active,
        backend_state,
    }
}

/// Whether `tailscale serve status --json` shows anything being served. The CLI
/// prints `null`/`{}` when nothing is served, else a config with `TCP`/`Web` keys.
fn serve_is_active(json: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::Object(map)) => {
            map.get("TCP").is_some_and(|value| !value.is_null())
                || map.get("Web").is_some_and(|value| !value.is_null())
        }
        _ => false,
    }
}

/// Extracts the interactive-login URL from `tailscale login` output. The CLI
/// prints something like "To authenticate, visit:\n\n\thttps://login.tailscale.com/a/abc".
fn extract_login_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|token| token.starts_with("https://") && token.contains("tailscale.com/"))
        .map(|token| token.trim_end_matches(['.', ',', ')']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "BackendState": "Running",
        "AuthURL": "",
        "TailscaleIPs": ["100.101.102.103", "fd7a::1"],
        "Self": {
            "HostName": "seraphim",
            "DNSName": "seraphim.tail1234.ts.net.",
            "Online": true,
            "TailscaleIPs": ["100.101.102.103"]
        },
        "CurrentTailnet": { "Name": "acme.org" }
    }"#;

    #[test]
    fn parses_a_running_node_into_a_url_and_state() {
        let status = parse_status(SAMPLE, true, true);
        assert!(status.connected);
        assert!(status.online);
        assert!(!status.needs_login);
        assert_eq!(status.hostname, "seraphim");
        // The trailing dot is stripped and the URL is the HTTPS serve address.
        assert_eq!(status.dns_name, "seraphim.tail1234.ts.net");
        assert_eq!(
            status.url.as_deref(),
            Some("https://seraphim.tail1234.ts.net")
        );
        assert_eq!(status.tailnet, "acme.org");
        assert_eq!(status.tailscale_ips, vec!["100.101.102.103"]);
        assert!(status.serve_active);
        assert!(status.auth_url.is_none());
        assert_eq!(status.backend_state, "Running");
    }

    #[test]
    fn flags_a_node_that_needs_login_and_surfaces_its_url() {
        let json = r#"{ "BackendState": "NeedsLogin",
            "AuthURL": "https://login.tailscale.com/a/abc123" }"#;
        let status = parse_status(json, true, false);
        assert!(!status.connected);
        assert!(status.needs_login);
        assert_eq!(
            status.auth_url.as_deref(),
            Some("https://login.tailscale.com/a/abc123")
        );
        // No Self node, so no URL.
        assert!(status.url.is_none());
    }

    #[test]
    fn garbage_status_does_not_panic() {
        let status = parse_status("not json", true, false);
        assert!(!status.connected);
        assert_eq!(status.backend_state, "");
    }

    #[test]
    fn serve_active_detects_a_served_config() {
        assert!(serve_is_active(
            r#"{ "TCP": { "443": {} }, "Web": { "x": {} } }"#
        ));
        assert!(!serve_is_active("{}"));
        assert!(!serve_is_active("null"));
        assert!(!serve_is_active(r#"{ "TCP": null }"#));
    }

    #[test]
    fn extracts_the_login_url_from_cli_output() {
        let output = "\nTo authenticate, visit:\n\n\thttps://login.tailscale.com/a/abc123\n\n";
        assert_eq!(
            extract_login_url(output).as_deref(),
            Some("https://login.tailscale.com/a/abc123")
        );
        assert!(extract_login_url("Success.").is_none());
    }
}
