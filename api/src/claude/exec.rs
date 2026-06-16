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
use crate::db::models::ClaudeAuthMode;

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
    /// How [`Self::oauth_token`] is injected: a subscription token becomes
    /// `CLAUDE_CODE_OAUTH_TOKEN`; an API key becomes `ANTHROPIC_API_KEY`.
    pub auth_mode: ClaudeAuthMode,
    /// The Claude credential (subscription token or API key), injected into the
    /// exec env per [`Self::auth_mode`] (never baked into the container).
    pub oauth_token: String,
    /// GitHub token, so the agent's `gh`/`git` are authed for this turn.
    pub github_token: String,
    /// The task being worked, so the agent's helpers (`seraphim-ask`,
    /// `seraphim-suggest`) can attribute their output (exported as
    /// `SERAPHIM_TASK_ID`).
    pub task_id: String,
    /// URL the workspace uses to reach the API (exported as `SERAPHIM_API_URL`).
    pub internal_api_url: String,
    /// Path inside the container where this turn records its `claude` PID, so a
    /// reset can kill exactly this agent's process and never the other agent's
    /// (the main agent and the compose assistant share one workspace; issue #181).
    pub pid_file: String,
    /// User-defined environment variables (`key`, `value`) for this exec.
    pub env: Vec<(String, String)>,
}

/// Builds the exec environment: the auth tokens, the agent-helper wiring, and
/// any user-defined variables. User variables come last so they cannot shadow
/// the tokens and wiring we control.
fn build_env(args: &TurnArgs) -> Vec<String> {
    let credential = match args.auth_mode {
        ClaudeAuthMode::Subscription => format!("CLAUDE_CODE_OAUTH_TOKEN={}", args.oauth_token),
        ClaudeAuthMode::ApiKey => format!("ANTHROPIC_API_KEY={}", args.oauth_token),
    };
    let mut env = vec![
        credential,
        format!("GH_TOKEN={}", args.github_token),
        format!("SERAPHIM_TASK_ID={}", args.task_id),
        format!("SERAPHIM_API_URL={}", args.internal_api_url),
    ];
    for (key, value) in &args.env {
        env.push(format!("{key}={value}"));
    }
    env
}

/// Builds the `claude` argv for a headless, fully-autonomous turn.
fn build_claude_argv(args: &TurnArgs) -> Vec<String> {
    let mut command = vec![
        "claude".to_string(),
        "-p".to_string(),
        args.prompt.clone(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        // stream-json with -p requires --verbose to emit the full event stream.
        "--verbose".to_string(),
        // Opt into the raw Messages-API stream events so we get live, per-chunk
        // token usage (`message_start` / `message_delta`) for the live counter,
        // not just the coarse totals at message/turn boundaries.
        "--include-partial-messages".to_string(),
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

/// Wraps the `claude` invocation in a tiny shell that records the process's PID to
/// `args.pid_file` before waiting on it. This lets a reset kill exactly this
/// agent's `claude` (by the recorded PID) and never the other agent's, since the
/// main agent and the compose assistant run in the same workspace (issue #181).
///
/// The claude args are passed as positional parameters (not interpolated into the
/// script) so an arbitrary prompt can never break the shell quoting. `claude` runs
/// in the background inheriting the exec's stdout/stderr, so streaming is
/// unaffected; the script's exit status is claude's, via `wait`.
fn build_command(args: &TurnArgs) -> Vec<String> {
    // $1 is the pid file; the rest ($@ after the shift) is the claude argv.
    let script =
        "pidfile=\"$1\"; shift; \"$@\" & cmd_pid=$!; printf '%s' \"$cmd_pid\" > \"$pidfile\"; \
         wait \"$cmd_pid\""
            .to_string();
    let mut command = vec![
        "bash".to_string(),
        "-c".to_string(),
        script,
        // $0 (a label) then $1 (the pid file); claude argv follows as $2..
        "seraphim-turn".to_string(),
        args.pid_file.clone(),
    ];
    command.extend(build_claude_argv(args));
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
                    env: Some(build_env(&args)),
                    // Claude must not run as root with bypassPermissions; the
                    // universal devcontainer image's non-root user is `codespace`.
                    user: Some("codespace".to_string()),
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
            auth_mode: ClaudeAuthMode::Subscription,
            oauth_token: "tok".to_string(),
            github_token: "gh".to_string(),
            task_id: "task-1".to_string(),
            internal_api_url: "http://api:27182".to_string(),
            pid_file: "/tmp/seraphim-agent.pid".to_string(),
            env: vec![],
        };
        let command = build_command(&args);
        assert!(command.contains(&"--resume".to_string()));
        assert!(command.contains(&"sess-1".to_string()));
        assert!(command.contains(&"bypassPermissions".to_string()));
        assert!(command.contains(&"stream-json".to_string()));
        assert!(command.contains(&"--include-partial-messages".to_string()));
    }

    #[test]
    fn command_omits_resume_for_fresh_session() {
        let args = TurnArgs {
            container: "seraphim-workspace".to_string(),
            working_dir: "/workspace/repo".to_string(),
            prompt: "start".to_string(),
            resume_session_id: None,
            model: "claude-opus-4-8[1m]".to_string(),
            auth_mode: ClaudeAuthMode::Subscription,
            oauth_token: "tok".to_string(),
            github_token: "gh".to_string(),
            task_id: "task-1".to_string(),
            internal_api_url: "http://api:27182".to_string(),
            pid_file: "/tmp/seraphim-agent.pid".to_string(),
            env: vec![],
        };
        let command = build_command(&args);
        assert!(!command.contains(&"--resume".to_string()));
    }
}
