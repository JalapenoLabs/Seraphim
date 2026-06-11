//! Inbound issue webhooks for realtime board updates.
//!
//! GitHub and Jira call these endpoints the moment an issue is created or
//! changed, so the board reflects it immediately instead of waiting for the next
//! poll. New issues are placed at the top of their column (the same shared path
//! the poll sync uses), and a successful change ticks the board SSE stream so the
//! open UI refetches at once.
//!
//! The endpoints are unauthenticated at the app layer; the shared webhook secret
//! is the authentication. GitHub signs every delivery with an HMAC-SHA256 of the
//! raw body (`X-Hub-Signature-256`). Jira, depending on deployment, either signs
//! the same way (Cloud dynamic webhooks) or carries the secret as a `?secret=`
//! URL parameter (Server/Data Center); both are accepted.

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use eyre::Result;
use serde::Deserialize;

use crate::db::models::SourceKind;
use crate::db::queries;
use crate::state::AppState;
use crate::{git, jira, orchestrator};

use super::ApiResult;

// --- GitHub ------------------------------------------------------------------

/// `POST /api/v1/webhooks/github` - applies a GitHub `issues` event to the board.
///
/// Configure the webhook on the repo (or org) for the "Issues" event with the
/// shared secret from Settings, pointing at this URL.
pub async fn github(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let secret = queries::get_github_webhook_secret(&state.db).await?;
    if secret.is_empty() {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            "GitHub webhook secret is not configured",
        )
            .into_response());
    }

    let signature = header_str(&headers, "x-hub-signature-256");
    if !verify_github_signature(secret.as_bytes(), &body, signature) {
        return Ok((StatusCode::UNAUTHORIZED, "invalid signature").into_response());
    }

    match header_str(&headers, "x-github-event").unwrap_or_default() {
        // GitHub's first delivery after configuring the hook; acknowledge it.
        "ping" => return Ok((StatusCode::OK, "pong").into_response()),
        // We only act on issue lifecycle events; ignore comments, pushes, etc.
        "issues" => {}
        _ => return Ok(StatusCode::OK.into_response()),
    }

    let event: GithubIssueEvent = match serde_json::from_slice(&body) {
        Ok(event) => event,
        Err(error) => {
            return Ok((StatusCode::BAD_REQUEST, format!("bad payload: {error}")).into_response())
        }
    };

    if apply_github_event(&state, event).await? {
        state.notify_board();
    }
    Ok(StatusCode::OK.into_response())
}

/// Applies one parsed GitHub issues event, returning whether the board changed.
async fn apply_github_event(state: &AppState, event: GithubIssueEvent) -> Result<bool> {
    // Only repos we track and that are enabled react; an event for an unknown or
    // disabled repo is ignored. (Sync-by-poll is not required: a repo can opt into
    // realtime webhooks alone.)
    let Some(repo) =
        queries::get_repository_by_full_name(&state.db, &event.repository.full_name).await?
    else {
        return Ok(false);
    };
    if !repo.enabled {
        return Ok(false);
    }
    let external_id = event.issue.number.to_string();

    if event.action == "deleted" {
        return Ok(queries::delete_issue_task(
            &state.db,
            SourceKind::Github,
            Some(repo.id),
            &external_id,
        )
        .await?);
    }

    // Place or refresh an open issue that passes the repo's label filter (the
    // same gate the poll sync applies). Anything else (closed, or filtered out)
    // only updates the state of a card we already track, never inserting one.
    let matches_labels = repo.issue_labels.is_empty()
        || event.issue.labels.iter().any(|label| {
            repo.issue_labels
                .iter()
                .any(|wanted| wanted.eq_ignore_ascii_case(&label.name))
        });

    if event.issue.state == "open" && matches_labels {
        let issue = git::OpenIssue {
            number: event.issue.number,
            title: event.issue.title,
            body: event.issue.body.unwrap_or_default(),
            url: event.issue.html_url,
            author_login: event.issue.user.login,
            author_avatar_url: event.issue.user.avatar_url,
        };
        // upsert_github_issue also reflects a reopen (closed -> open) by returning
        // the card to Available.
        orchestrator::upsert_github_issue(state, repo.id, &issue, "open").await?;
        Ok(true)
    } else if event.issue.state == "closed" {
        // Closed outside Seraphim: move the tracked card to Done.
        Ok(orchestrator::reflect_closed_github_issue(state, repo.id, &external_id).await?)
    } else {
        // Open but filtered out by the repo's labels: keep the cached state
        // current without moving a card we may not even track.
        Ok(queries::refresh_issue_external_state(
            &state.db,
            SourceKind::Github,
            Some(repo.id),
            &external_id,
            &event.issue.state,
        )
        .await?)
    }
}

#[derive(Debug, Deserialize)]
struct GithubIssueEvent {
    action: String,
    issue: GithubIssue,
    repository: GithubRepo,
}

#[derive(Debug, Deserialize)]
struct GithubIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    /// `"open"` or `"closed"`.
    state: String,
    user: GithubUser,
    #[serde(default)]
    labels: Vec<GithubLabel>,
}

#[derive(Debug, Deserialize)]
struct GithubUser {
    login: String,
    avatar_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepo {
    full_name: String,
}

// --- Jira --------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct JiraWebhookQuery {
    /// Server/Data Center carries the shared secret here (Cloud signs instead).
    #[serde(default)]
    secret: Option<String>,
}

/// `POST /api/v1/webhooks/jira` - applies a Jira issue event to the board.
///
/// Configure a Jira webhook for the "issue created / updated / deleted" events
/// with the shared secret from Settings, pointing at this URL (append
/// `?secret=<value>` on Server/Data Center, which does not sign the body).
pub async fn jira(
    State(state): State<AppState>,
    Query(query): Query<JiraWebhookQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let secret = queries::get_jira_webhook_secret(&state.db).await?;
    if secret.is_empty() {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            "Jira webhook secret is not configured",
        )
            .into_response());
    }
    if !verify_jira(secret.as_bytes(), &body, &headers, query.secret.as_deref()) {
        return Ok((StatusCode::UNAUTHORIZED, "invalid secret").into_response());
    }

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return Ok((StatusCode::BAD_REQUEST, format!("bad payload: {error}")).into_response())
        }
    };

    if apply_jira_event(&state, &payload).await? {
        state.notify_board();
    }
    Ok(StatusCode::OK.into_response())
}

/// Applies one Jira webhook payload, returning whether the board changed. The
/// issue is matched to a followed, sync-enabled board by its project key, then
/// upserted through the same path the poll sync uses.
async fn apply_jira_event(state: &AppState, payload: &serde_json::Value) -> Result<bool> {
    let event = payload
        .get("webhookEvent")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let Some(issue) = payload.get("issue") else {
        return Ok(false);
    };

    if event == "jira:issue_deleted" {
        if let Some(key) = issue.get("key").and_then(serde_json::Value::as_str) {
            return Ok(queries::delete_jira_task(&state.db, key).await?);
        }
        return Ok(false);
    }
    if event != "jira:issue_created" && event != "jira:issue_updated" {
        return Ok(false);
    }

    let Some(project_key) = jira::project_key_from_webhook(issue) else {
        return Ok(false);
    };
    let Some(board) = queries::list_jira_boards_to_sync(&state.db)
        .await?
        .into_iter()
        .find(|board| board.project_key.eq_ignore_ascii_case(&project_key))
    else {
        return Ok(false);
    };

    let settings = queries::get_settings(&state.db).await?;
    let Some(parsed) = jira::issue_from_webhook(issue, &settings.jira_base_url) else {
        return Ok(false);
    };
    orchestrator::upsert_jira_issue(state, &board, &parsed).await?;
    Ok(true)
}

// --- Signature / secret verification -----------------------------------------

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// Verifies GitHub's `X-Hub-Signature-256: sha256=<hex>` over the raw body.
fn verify_github_signature(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(hex_sig) = header.and_then(|header| header.strip_prefix("sha256=")) else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };
    verify_hmac(secret, body, &expected)
}

/// Accepts a Jira delivery signed with the secret (`X-Hub-Signature[-256]`, as
/// Cloud's dynamic webhooks do) or carrying it as the `?secret=` URL parameter
/// (Server/Data Center, which does not sign).
fn verify_jira(
    secret: &[u8],
    body: &[u8],
    headers: &HeaderMap,
    query_secret: Option<&str>,
) -> bool {
    if let Some(provided) = query_secret {
        if constant_time_eq(provided.as_bytes(), secret) {
            return true;
        }
    }
    for header_name in ["x-hub-signature-256", "x-hub-signature"] {
        if let Some(value) = header_str(headers, header_name) {
            let hex_sig = value.strip_prefix("sha256=").unwrap_or(value);
            if let Ok(expected) = hex::decode(hex_sig) {
                if verify_hmac(secret, body, &expected) {
                    return true;
                }
            }
        }
    }
    false
}

/// True when HMAC-SHA256(secret, body) equals `expected`. `verify_slice` does the
/// comparison in constant time.
fn verify_hmac(secret: &[u8], body: &[u8], expected: &[u8]) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(expected).is_ok()
}

/// Length-checked constant-time byte comparison for the plain shared secret.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    fn sign(secret: &[u8], body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn github_signature_accepts_only_the_right_secret_and_body() {
        let secret = b"topsecret";
        let body = br#"{"action":"opened"}"#;
        let header = sign(secret, body);

        assert!(verify_github_signature(secret, body, Some(&header)));
        // Wrong secret, tampered body, malformed header, and missing header all fail.
        assert!(!verify_github_signature(
            b"wrong-secret",
            body,
            Some(&header)
        ));
        assert!(!verify_github_signature(
            secret,
            br#"{"action":"closed"}"#,
            Some(&header)
        ));
        assert!(!verify_github_signature(
            secret,
            body,
            Some("sha256=not-hex")
        ));
        assert!(!verify_github_signature(secret, body, Some("deadbeef")));
        assert!(!verify_github_signature(secret, body, None));
    }

    #[test]
    fn parses_a_github_issues_event() {
        let body = r#"{
            "action": "opened",
            "issue": {
                "number": 42,
                "title": "Realtime sync",
                "body": "please",
                "html_url": "https://github.com/o/r/issues/42",
                "state": "open",
                "user": { "login": "octocat", "avatar_url": "https://avatars/x" },
                "labels": [{ "name": "enhancement" }]
            },
            "repository": { "full_name": "o/r" }
        }"#;
        let event: GithubIssueEvent = serde_json::from_str(body).unwrap();
        assert_eq!(event.action, "opened");
        assert_eq!(event.issue.number, 42);
        assert_eq!(event.issue.state, "open");
        assert_eq!(event.issue.user.login, "octocat");
        assert_eq!(event.issue.labels[0].name, "enhancement");
        assert_eq!(event.repository.full_name, "o/r");
    }

    #[test]
    fn jira_accepts_url_secret_or_hmac() {
        let secret = b"jira-secret";
        let body = br#"{"webhookEvent":"jira:issue_created"}"#;
        let empty = HeaderMap::new();

        // URL-parameter path (Server/DC).
        assert!(verify_jira(secret, body, &empty, Some("jira-secret")));
        assert!(!verify_jira(secret, body, &empty, Some("nope")));
        // No secret presented at all.
        assert!(!verify_jira(secret, body, &empty, None));

        // Signed path (Cloud).
        let mut signed = HeaderMap::new();
        signed.insert("x-hub-signature-256", sign(secret, body).parse().unwrap());
        assert!(verify_jira(secret, body, &signed, None));
    }
}
