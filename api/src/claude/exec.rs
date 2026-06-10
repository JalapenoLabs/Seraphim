//! Runs a Claude Code turn inside the workspace container and streams its events.
//!
//! We `docker exec` the `claude` CLI in headless mode with stream-json output,
//! read its stdout/stderr line by line, and parse each line into an
//! [`AgentEvent`]. The orchestrator consumes the resulting stream to persist
//! turns/events and push live updates to the UI.

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use eyre::{eyre, Result};
use futures::{Stream, StreamExt};

use super::events::{parse_line, AgentEvent};

/// Everything needed to run one turn of the long-lived conversation.
#[derive(Debug, Clone)]
pub struct TurnArgs {
    /// Name of the workspace container to exec into.
    pub container: String,
    /// Working directory inside the container (the cloned repo).
    pub working_dir: String,
    /// The prompt for this turn.
    pub prompt: String,
    /// Session to resume; `None` starts (and the result reports) a fresh one.
    pub resume_session_id: Option<String>,
    /// Model id, e.g. `claude-opus-4-8[1m]`.
    pub model: String,
    /// Subscription OAuth token, injected into the exec env (not the container).
    pub oauth_token: String,
    /// GitHub token, so the agent's `gh`/`git` are authed for this turn.
    pub github_token: String,
}

/// Builds the `claude` argv for a headless, fully-autonomous turn.
fn build_command(args: &TurnArgs) -> Vec<String> {
    let mut command = vec![
        "claude".to_string(),
        "-p".to_string(),
        args.prompt.clone(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        // stream-json with -p requires --verbose to emit the full event stream.
        "--verbose".to_string(),
        "--permission-mode".to_string(),
        "bypassPermissions".to_string(),
        "--model".to_string(),
        args.model.clone(),
    ];

    if let Some(session_id) = &args.resume_session_id {
        command.push("--resume".to_string());
        command.push(session_id.clone());
    }

    command
}

/// Executes a turn, yielding each parsed [`AgentEvent`] as it arrives.
///
/// The stream completes when the `claude` process exits. Errors from the Docker
/// API surface as stream items; UTF-8 boundaries split across read chunks are
/// handled by buffering until a newline is seen.
pub fn run_turn(docker: &Docker, args: TurnArgs) -> impl Stream<Item = Result<AgentEvent>> + '_ {
    async_stream::try_stream! {
        let exec = docker
            .create_exec(
                &args.container,
                CreateExecOptions {
                    cmd: Some(build_command(&args)),
                    working_dir: Some(args.working_dir.clone()),
                    // Secrets are injected per-exec from the database, never baked
                    // into the container's environment.
                    env: Some(vec![
                        format!("CLAUDE_CODE_OAUTH_TOKEN={}", args.oauth_token),
                        format!("GH_TOKEN={}", args.github_token),
                    ]),
                    // Claude must not run as root with bypassPermissions.
                    user: Some("node".to_string()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?;

        let StartExecResults::Attached { mut output, .. } =
            docker.start_exec(&exec.id, None).await?
        else {
            Err(eyre!("docker exec did not attach an output stream"))?;
            return;
        };

        let mut buffer: Vec<u8> = Vec::new();

        while let Some(chunk) = output.next().await {
            let bytes = match chunk? {
                LogOutput::StdOut { message }
                | LogOutput::StdErr { message }
                | LogOutput::Console { message } => message,
                LogOutput::StdIn { .. } => continue,
            };
            buffer.extend_from_slice(&bytes);

            // Drain every complete line currently in the buffer.
            while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = buffer.drain(..=newline).collect();
                let text = String::from_utf8_lossy(&line);
                for event in parse_line(&text) {
                    yield event;
                }
            }
        }

        // Flush a trailing line with no terminating newline.
        if !buffer.is_empty() {
            let text = String::from_utf8_lossy(&buffer);
            for event in parse_line(&text) {
                yield event;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_includes_resume_when_session_present() {
        let args = TurnArgs {
            container: "seraphim-workspace".to_string(),
            working_dir: "/workspace/repo".to_string(),
            prompt: "do the thing".to_string(),
            resume_session_id: Some("sess-1".to_string()),
            model: "claude-opus-4-8[1m]".to_string(),
            oauth_token: "tok".to_string(),
            github_token: "gh".to_string(),
        };
        let command = build_command(&args);
        assert!(command.contains(&"--resume".to_string()));
        assert!(command.contains(&"sess-1".to_string()));
        assert!(command.contains(&"bypassPermissions".to_string()));
        assert!(command.contains(&"stream-json".to_string()));
    }

    #[test]
    fn command_omits_resume_for_fresh_session() {
        let args = TurnArgs {
            container: "seraphim-workspace".to_string(),
            working_dir: "/workspace/repo".to_string(),
            prompt: "start".to_string(),
            resume_session_id: None,
            model: "claude-opus-4-8[1m]".to_string(),
            oauth_token: "tok".to_string(),
            github_token: "gh".to_string(),
        };
        let command = build_command(&args);
        assert!(!command.contains(&"--resume".to_string()));
    }
}
