//! The compose assistant (issue #181): a second, on-demand Claude session that
//! helps the operator draft many issues at once through a chat.
//!
//! It runs in the same workspace as the main agent but is fully isolated from it:
//! its own resumable session (tracked via `compose_turns.session_id`, never the
//! shared `settings.current_session_id`), its own conversation tables, and a
//! PID-targeted process (see [`COMPOSE_PID_FILE`]) so a compose reset or a main
//! agent reset never kills the other's turn. It is on-demand: a turn runs only
//! when the operator sends a message, and nothing keeps it alive between turns.

use std::time::Duration;

use eyre::Result;
use futures::StreamExt;
use tokio::time::timeout;
use tracing::warn;

use super::subscription;
use super::COMPOSE_PID_FILE;
use crate::claude::run_turn as run_claude_turn;
use crate::claude::{AgentEventKind, TurnArgs};
use crate::db::queries;
use crate::secrets::Scrubber;
use crate::state::AppState;

/// A turn silent longer than this is presumed hung and ended (matches the main
/// agent's heartbeat). The compose assistant is interactive, so this is generous.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The guide handed to the assistant at the start of a fresh conversation. On a
/// resumed turn Claude already has this context, so only the operator's message
/// is sent.
const DRAFTING_GUIDE: &str = "\
You are Seraphim's issue-drafting assistant. Help the operator brainstorm and \
write a batch of high-quality issues through conversation. Ask clarifying \
questions when it helps; propose concrete, well-scoped issues.\n\n\
You manage a list of DRAFT issues (not yet created). Whenever the set of drafts \
should change, call the `seraphim-draft` CLI with the COMPLETE current list as \
JSON; it replaces the stored drafts with exactly what you send:\n\n\
  seraphim-draft '{\"drafts\":[{\"title\":\"...\",\"body\":\"...markdown...\",\"repo\":\"owner/name\"}]}'\n\n\
- `repo` is optional; include it (as `owner/name`) when an issue clearly belongs \
to a specific repository.\n\
- Always send the FULL list (including drafts you are keeping), because the call \
replaces all drafts.\n\
- Write real, specific titles and Markdown bodies. Do NOT create issues or open \
PRs yourself; the operator clicks \"Create\" when the batch is ready.\n\n\
Reply conversationally to the operator and keep the draft list in sync via the CLI.";

/// The full prompt for a turn: the guide plus the operator's message on a fresh
/// conversation, or just the message when resuming (Claude retains the guide).
fn build_prompt(message: &str, fresh: bool) -> String {
    if fresh {
        format!("{DRAFTING_GUIDE}\n\n# Operator message\n{message}")
    } else {
        message.to_string()
    }
}

/// Runs one compose turn to completion, persisting and streaming its events.
/// Spawned by the HTTP handler so the request returns immediately; failures are
/// logged and recorded on the turn rather than surfaced to the caller.
pub async fn run(state: AppState, message: String) {
    if let Err(error) = run_inner(&state, message).await {
        warn!(error = %error, "compose turn failed");
        state.notify_compose_changed();
    }
}

async fn run_inner(state: &AppState, message: String) -> Result<()> {
    let settings = queries::get_settings(&state.db).await?;
    let resume = queries::latest_compose_session_id(&state.db).await?;
    let prompt = build_prompt(&message, resume.is_none());
    let turn_id = queries::create_compose_turn(&state.db, &prompt, resume.as_deref()).await?;
    state.notify_compose_changed();

    // Scrub every stored secret out of anything the assistant echoes, as the main
    // turn does, before it is persisted or streamed.
    let scrubber = Scrubber::new(queries::list_secret_values(&state.db).await?);

    // Record the operator's message (not the guide-wrapped prompt) as event 0, so
    // the transcript reads naturally and survives a reload.
    let prompt_event = serde_json::json!({ "text": message });
    queries::append_compose_event(&state.db, turn_id, 0, "prompt", prompt_event.clone()).await?;
    state.notify_compose(serde_json::json!({
        "type": "prompt", "payload": prompt_event, "created_at": chrono::Utc::now(),
    }));

    let env = queries::list_environment_variables(&state.db)
        .await?
        .into_iter()
        .map(|variable| (variable.key, variable.value))
        .collect();

    let args = TurnArgs {
        container: state.workspace.container().to_string(),
        working_dir: "/workspace".to_string(),
        prompt,
        resume_session_id: resume.clone(),
        model: settings.claude_model.clone(),
        auth_mode: settings.claude_auth_mode,
        oauth_token: subscription::fresh_inference_token(state).await?,
        github_token: queries::get_github_token(&state.db).await?,
        // No task backs the compose chat; its helper (`seraphim-draft`) posts to a
        // task-agnostic endpoint, so a label is enough here.
        task_id: "compose".to_string(),
        internal_api_url: state.internal_api_url.clone(),
        pid_file: COMPOSE_PID_FILE.to_string(),
        env,
    };

    // Reap any compose process leaked by a prior aborted turn (never the main
    // agent's: this targets only the compose PID file).
    kill_compose_process(state).await;

    let mut stream = Box::pin(run_claude_turn(state.workspace.docker(), args));
    let mut seq = 1_i32;
    let mut session_id = resume;
    let mut result_text: Option<String> = None;
    let mut total_cost: Option<f64> = None;
    let mut token_usage: Option<serde_json::Value> = None;
    let mut error_message: Option<String> = None;

    loop {
        let item = match timeout(HEARTBEAT_TIMEOUT, stream.next()).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_elapsed) => {
                error_message = Some("No output from the assistant; presumed hung.".to_string());
                kill_compose_process(state).await;
                break;
            }
        };
        let event = match item {
            Ok(event) => event,
            Err(error) => {
                error_message = Some(format!("Claude stream error: {error}"));
                break;
            }
        };

        // The partial-message usage firehose drives the main agent's live counter;
        // the compose stats settle at turn end, so it is simply skipped here.
        if matches!(&event.kind, AgentEventKind::Usage { .. }) {
            continue;
        }
        if let Some(found) = &event.session_id {
            session_id = Some(found.clone());
        }
        if let AgentEventKind::Result {
            total_cost_usd,
            result_text: text,
            is_error,
        } = &event.kind
        {
            total_cost = *total_cost_usd;
            token_usage = event.raw.get("usage").cloned();
            result_text = text.as_deref().map(|text| scrubber.scrub_text(text));
            if *is_error {
                let message = text
                    .clone()
                    .unwrap_or_else(|| "the assistant reported an error".to_string());
                error_message = Some(scrubber.scrub_text(&message));
            }
        }

        let label = event.type_label();
        let mut payload = event.raw.clone();
        scrubber.scrub_value(&mut payload);
        queries::append_compose_event(&state.db, turn_id, seq, label, payload.clone()).await?;
        if label != "rate_limit" {
            state.notify_compose(serde_json::json!({
                "type": label, "payload": payload, "created_at": chrono::Utc::now(),
            }));
        }
        seq += 1;
    }

    let status = if error_message.is_some() {
        "failed"
    } else {
        "completed"
    };
    queries::finish_compose_turn(
        &state.db,
        turn_id,
        status,
        result_text.as_deref(),
        total_cost,
        token_usage,
        session_id.as_deref(),
    )
    .await?;
    state.notify_compose_changed();
    Ok(())
}

/// Kills the compose assistant's recorded `claude` process (and only it), then
/// removes its PID file. Best-effort: never touches the main agent's process.
async fn kill_compose_process(state: &AppState) {
    let script = format!(
        "pid=$(cat {file} 2>/dev/null); if [ -n \"$pid\" ]; then kill -9 \"$pid\" 2>/dev/null || true; fi; rm -f {file}; true",
        file = COMPOSE_PID_FILE,
    );
    let _ = state
        .workspace
        .exec_capture(
            "/workspace",
            vec!["bash".to_string(), "-lc".to_string(), script],
            vec![],
        )
        .await;
}

/// Resets the compose assistant: stops any running turn, wipes its conversation
/// (and the on-disk Claude session so history truly restarts), and clears all
/// drafts. Deliberately leaves the main agent and its session untouched.
pub async fn reset(state: &AppState) -> Result<()> {
    kill_compose_process(state).await;

    // Delete only this conversation's on-disk session file, by its id, so the main
    // agent's sessions are never affected (unlike the global hard reset).
    if let Some(session) = queries::latest_compose_session_id(&state.db).await? {
        let script = format!(
            ": \"${{CLAUDE_CONFIG_DIR:=/workspace/.claude}}\"; \
             find \"$CLAUDE_CONFIG_DIR/projects\" -type f -name '{session}.jsonl' -delete 2>/dev/null || true",
            session = session,
        );
        let _ = state
            .workspace
            .exec_capture(
                "/workspace",
                vec!["bash".to_string(), "-lc".to_string(), script],
                vec![],
            )
            .await;
    }

    queries::clear_compose_history(&state.db).await?;
    queries::clear_drafts(&state.db).await?;
    state.notify_compose_changed();
    Ok(())
}
