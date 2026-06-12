//! Posting a per-turn summary of the agent's reasoning back to the source issue.
//!
//! When [`Settings::post_thoughts_enabled`] is on, the orchestrator collects the
//! agent's "thoughts" during a turn (its extended-thinking blocks and prose),
//! then asks a separate, throwaway Claude invocation to condense them into a
//! short progress note and posts that as a single comment on the GitHub issue.
//! One comment per turn keeps the ticket readable instead of flooding it with
//! raw reasoning, and the summarization reuses the operator's subscription token.

use eyre::Result;
use futures::StreamExt;
use tracing::warn;

use crate::claude::{run_turn, AgentEventKind, TurnArgs};
use crate::db::models::{Settings, SourceKind, Task};
use crate::db::queries;
use crate::git;
use crate::state::AppState;

/// Best-effort: summarize this turn's `thoughts` and post them as one comment on
/// the task's GitHub issue.
///
/// Does nothing when the feature is off, when there were no thoughts, or when the
/// task isn't a GitHub issue with a known `owner/repo`. Any failure (a flaky
/// summarization turn, a GitHub hiccup) is the caller's to log; it never affects
/// the task's own outcome.
pub async fn post_turn_thoughts(
    state: &AppState,
    settings: &Settings,
    task: &Task,
    thoughts: &[String],
) -> Result<()> {
    if !settings.post_thoughts_enabled || thoughts.is_empty() {
        return Ok(());
    }
    if task.source_kind != SourceKind::Github {
        return Ok(());
    }
    let Some(repo_id) = task.repo_id else {
        return Ok(());
    };
    let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
        return Ok(());
    };
    let Some((owner, name)) = repo.full_name.split_once('/') else {
        return Ok(());
    };

    let Some(summary) = summarize(state, settings, thoughts).await? else {
        warn!(task = %task.id, "thoughts summarization produced no text; skipping comment");
        return Ok(());
    };
    let summary = summary.trim();
    if summary.is_empty() {
        return Ok(());
    }

    git::add_issue_comment(
        &state.github().await?,
        owner,
        name,
        &task.external_id,
        summary,
    )
    .await?;
    Ok(())
}

/// Runs a throwaway (non-resuming) Claude turn that condenses the raw thoughts
/// into a brief progress note, returning its final text.
async fn summarize(
    state: &AppState,
    settings: &Settings,
    thoughts: &[String],
) -> Result<Option<String>> {
    let args = TurnArgs {
        container: state.workspace.container().to_string(),
        working_dir: "/workspace".to_string(),
        prompt: build_prompt(thoughts),
        // No resume: this summary is standalone and must not be recorded into (or
        // disturb) the shared task session.
        resume_session_id: None,
        model: settings.claude_model.clone(),
        auth_mode: settings.claude_auth_mode,
        oauth_token: super::subscription::fresh_inference_token(state).await?,
        github_token: queries::get_github_token(&state.db).await?,
        // A summary turn doesn't act as the task, so it gets no helper wiring.
        task_id: String::new(),
        internal_api_url: state.internal_api_url.clone(),
        env: Vec::new(),
    };

    let mut stream = Box::pin(run_turn(state.workspace.docker(), args));
    let mut text: Option<String> = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                if let AgentEventKind::Result {
                    result_text: Some(found),
                    ..
                } = &event.kind
                {
                    text = Some(found.clone());
                }
            }
            Err(error) => {
                warn!(error = %error, "thoughts summarization stream error");
                break;
            }
        }
    }
    Ok(text)
}

/// The summarization prompt: the raw thoughts wrapped in instructions to produce
/// a short, tool-free progress note.
fn build_prompt(thoughts: &[String]) -> String {
    let joined = thoughts.join("\n\n");
    format!(
        "You are condensing an autonomous coding agent's reasoning into a short progress note for \
         the GitHub issue it is working on. Below are the agent's raw thoughts from one work \
         session.\n\n\
         Write a concise, human-readable summary (a few sentences, or a short bullet list) of what \
         the agent considered and did. Be factual and skip filler. Do not use any tools, do not ask \
         questions, and reply with only the summary text.\n\n\
         Agent thoughts:\n{joined}\n"
    )
}
