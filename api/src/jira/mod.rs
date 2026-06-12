//! Jira integration: a dual-mode (Cloud + Server / Data Center) REST client plus
//! the pure mapping between Jira statuses and our kanban columns.
//!
//! Cloud authenticates with Basic auth (account email + API token) against REST
//! v3; Server / Data Center uses a Bearer personal access token against REST v2.
//! The deployment, base URL, and credentials all come from the settings row.
//! Board discovery and issue listing use the Agile API, whose path is the same on
//! both deployments.
//!
//! This module covers connecting, discovering boards, reading tickets, and moving
//! a ticket's status. Having the agent autonomously code a Jira ticket (which, for
//! a board that spans several repos, means branching and opening a PR in more than
//! one repo) is a separate, still-open execution model and is not wired here.

use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use eyre::{eyre, Result};
use serde::Deserialize;

use crate::db::models::{JiraDeployment, Settings, TaskColumn};

/// Pagination/runaway guards: at most this many items pulled per board/board list.
const MAX_BOARDS: usize = 500;
const MAX_ISSUES_PER_BOARD: usize = 500;
const PAGE_SIZE: i64 = 50;

/// The Agile API path (board discovery + board issues), identical on Cloud and
/// Server, hence not deployment-dependent like [`JiraConfig::api_base`].
const AGILE_API: &str = "/rest/agile/1.0";

// --- Pure connection config + helpers ---------------------------------------

/// A resolved Jira connection, built from the settings row + the secret token.
#[derive(Debug, Clone)]
pub struct JiraConfig {
    pub deployment: JiraDeployment,
    /// Site base URL with any trailing slash trimmed, e.g. `https://acme.atlassian.net`.
    pub base_url: String,
    pub email: String,
    pub token: String,
}

impl JiraConfig {
    /// Builds a config from settings and the separately fetched secret token,
    /// returning `None` unless Jira is enabled and the essentials are present.
    pub fn from_settings(settings: &Settings, token: &str) -> Option<Self> {
        if !settings.jira_enabled {
            return None;
        }
        let base_url = settings
            .jira_base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        if base_url.is_empty() || token.is_empty() {
            return None;
        }
        Some(Self {
            deployment: settings.jira_deployment,
            base_url,
            email: settings.jira_email.trim().to_string(),
            token: token.to_string(),
        })
    }

    /// The REST API version path: v3 on Cloud, v2 on Server / Data Center.
    fn api_base(&self) -> &'static str {
        match self.deployment {
            JiraDeployment::Cloud => "/rest/api/3",
            JiraDeployment::Server => "/rest/api/2",
        }
    }

    /// The `Authorization` header value: Basic (Cloud) or Bearer (Server).
    fn auth_header(&self) -> String {
        match self.deployment {
            JiraDeployment::Cloud => {
                let encoded = STANDARD.encode(format!("{}:{}", self.email, self.token));
                format!("Basic {encoded}")
            }
            JiraDeployment::Server => format!("Bearer {}", self.token),
        }
    }
}

/// The column a Jira status maps to under a board's configured map. An unmapped
/// status falls back to `Available`, so a newly synced ticket is always placed.
pub fn column_for_status(status_map: &HashMap<String, TaskColumn>, status: &str) -> TaskColumn {
    status_map
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(status))
        .map_or(TaskColumn::Available, |(_, column)| *column)
}

/// The Jira status name to transition to for one of our columns: the
/// (deterministically chosen) mapped status whose column matches, or `None` when
/// nothing maps there.
pub fn status_for_column(
    status_map: &HashMap<String, TaskColumn>,
    column: TaskColumn,
) -> Option<String> {
    let mut matches: Vec<&String> = status_map
        .iter()
        .filter(|(_, mapped)| **mapped == column)
        .map(|(name, _)| name)
        .collect();
    matches.sort();
    matches.into_iter().next().cloned()
}

/// The project key a webhook's `issue` object belongs to, used to match it to a
/// followed board (boards are keyed by project). `None` if the shape is missing.
pub fn project_key_from_webhook(issue: &serde_json::Value) -> Option<String> {
    issue
        .get("fields")?
        .get("project")?
        .get("key")?
        .as_str()
        .map(str::to_string)
}

/// Builds a [`JiraIssue`] from a webhook payload's `issue` object. The webhook
/// carries the same issue resource the REST sync reads, so this mirrors
/// [`JiraClient::list_board_issues`]'s field extraction. `base_url` builds the
/// browse URL. `None` when the payload has no issue key.
pub fn issue_from_webhook(issue: &serde_json::Value, base_url: &str) -> Option<JiraIssue> {
    let key = issue.get("key")?.as_str()?.to_string();
    let fields = issue.get("fields");
    let summary = fields
        .and_then(|fields| fields.get("summary"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let status = fields
        .and_then(|fields| fields.get("status"))
        .and_then(|status| status.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let description = description_to_text(fields.and_then(|fields| fields.get("description")));
    let url = format!("{}/browse/{}", base_url.trim_end_matches('/'), key);
    Some(JiraIssue {
        key,
        summary,
        status,
        url,
        description,
    })
}

// --- Public DTOs -------------------------------------------------------------

/// The current account, returned by a connection test.
#[derive(Debug, Clone)]
pub struct JiraIdentity {
    pub display_name: String,
}

/// A board surfaced by discovery.
#[derive(Debug, Clone)]
pub struct JiraBoardSummary {
    pub board_id: i64,
    pub name: String,
    pub project_key: String,
}

/// A ticket pulled from a board during sync.
#[derive(Debug, Clone)]
pub struct JiraIssue {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub url: String,
    pub description: String,
}

// --- The async REST client ---------------------------------------------------

/// A configured Jira client. Built on demand from the stored connection so a
/// token saved in the UI takes effect without a restart (mirrors the GitHub one).
pub struct JiraClient {
    config: JiraConfig,
    http: reqwest::Client,
}

impl JiraClient {
    pub fn new(config: JiraConfig) -> Result<Self> {
        let http = reqwest::Client::builder().user_agent("seraphim").build()?;
        Ok(Self { config, http })
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T> {
        let response = self
            .http
            .get(url)
            .header("Authorization", self.config.auth_header())
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(eyre!("Jira GET {url} failed ({status}): {body}"));
        }
        Ok(response.json::<T>().await?)
    }

    /// Confirms the connection works and returns the authenticated account.
    pub async fn verify(&self) -> Result<JiraIdentity> {
        let url = format!("{}{}/myself", self.config.base_url, self.config.api_base());
        let me: RawMyself = self.get_json(&url).await?;
        Ok(JiraIdentity {
            display_name: me
                .display_name
                .or(me.name)
                .or(me.email_address)
                .unwrap_or_else(|| "Jira user".to_string()),
        })
    }

    /// All boards the account can see (for the "discover boards" picker).
    pub async fn list_boards(&self) -> Result<Vec<JiraBoardSummary>> {
        let mut boards = Vec::new();
        let mut start_at = 0i64;
        loop {
            let url = format!(
                "{}{}/board?startAt={start_at}&maxResults={PAGE_SIZE}",
                self.config.base_url, AGILE_API
            );
            let page: BoardPage = self.get_json(&url).await?;
            for raw in &page.values {
                boards.push(JiraBoardSummary {
                    board_id: raw.id,
                    name: raw.name.clone(),
                    project_key: raw
                        .location
                        .as_ref()
                        .and_then(|location| location.project_key.clone())
                        .unwrap_or_default(),
                });
            }
            if page.is_last || page.values.is_empty() || boards.len() >= MAX_BOARDS {
                break;
            }
            start_at += PAGE_SIZE;
        }
        Ok(boards)
    }

    /// The issues currently on a board, capped at [`MAX_ISSUES_PER_BOARD`].
    pub async fn list_board_issues(&self, board_id: i64) -> Result<Vec<JiraIssue>> {
        let mut issues = Vec::new();
        let mut start_at = 0i64;
        loop {
            let url = format!(
                "{}{}/board/{board_id}/issue?fields=summary,status,description&startAt={start_at}&maxResults={PAGE_SIZE}",
                self.config.base_url,
                AGILE_API
            );
            let page: IssuePage = self.get_json(&url).await?;
            for raw in &page.issues {
                issues.push(JiraIssue {
                    key: raw.key.clone(),
                    summary: raw.fields.summary.clone().unwrap_or_default(),
                    status: raw
                        .fields
                        .status
                        .as_ref()
                        .map(|status| status.name.clone())
                        .unwrap_or_default(),
                    url: format!("{}/browse/{}", self.config.base_url, raw.key),
                    description: description_to_text(raw.fields.description.as_ref()),
                });
            }
            let page_len = i64::try_from(page.issues.len()).unwrap_or(PAGE_SIZE);
            if page.issues.is_empty()
                || start_at + page_len >= page.total
                || issues.len() >= MAX_ISSUES_PER_BOARD
            {
                break;
            }
            start_at += PAGE_SIZE;
        }
        Ok(issues)
    }

    /// Transitions an issue to the workflow status named `target` (matched against
    /// the available transitions' destination, case-insensitively). Returns
    /// whether a transition was actually performed; `false` means no transition
    /// from the current status leads there (already there, or not allowed).
    pub async fn transition_issue(&self, issue_key: &str, target: &str) -> Result<bool> {
        let url = format!(
            "{}{}/issue/{issue_key}/transitions",
            self.config.base_url,
            self.config.api_base()
        );
        let available: TransitionsResponse = self.get_json(&url).await?;
        let Some(transition) = available.transitions.into_iter().find(|transition| {
            transition
                .to
                .as_ref()
                .is_some_and(|to| to.name.eq_ignore_ascii_case(target))
                || transition.name.eq_ignore_ascii_case(target)
        }) else {
            return Ok(false);
        };

        let response = self
            .http
            .post(&url)
            .header("Authorization", self.config.auth_header())
            .json(&serde_json::json!({ "transition": { "id": transition.id } }))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(eyre!(
                "Jira transition of {issue_key} failed ({status}): {body}"
            ));
        }
        Ok(true)
    }

    /// Creates a `Task`-type issue in `project_key` and returns its key + URL.
    /// Cloud (REST v3) takes the description as Atlassian Document Format; Server
    /// (v2) takes plain text, so the description is encoded per deployment.
    pub async fn create_issue(
        &self,
        project_key: &str,
        summary: &str,
        description: &str,
    ) -> Result<CreatedJiraIssue> {
        #[derive(Deserialize)]
        struct Created {
            key: String,
        }

        let url = format!("{}{}/issue", self.config.base_url, self.config.api_base());

        let mut fields = serde_json::json!({
            "project": { "key": project_key },
            "summary": summary,
            "issuetype": { "name": "Task" },
        });
        if !description.trim().is_empty() {
            fields["description"] = match self.config.deployment {
                JiraDeployment::Cloud => serde_json::json!({
                    "type": "doc",
                    "version": 1,
                    "content": [{
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": description }],
                    }],
                }),
                JiraDeployment::Server => serde_json::json!(description),
            };
        }

        let response = self
            .http
            .post(&url)
            .header("Authorization", self.config.auth_header())
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "fields": fields }))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(eyre!("Jira issue creation failed ({status}): {body}"));
        }

        let created: Created = response.json().await?;
        Ok(CreatedJiraIssue {
            url: format!("{}/browse/{}", self.config.base_url, created.key),
        })
    }
}

/// A freshly created Jira ticket.
#[derive(Debug, Clone)]
pub struct CreatedJiraIssue {
    pub url: String,
}

/// Flattens a Jira description into plain text. Server (v2) returns a string;
/// Cloud (v3) returns an Atlassian Document Format tree, so we collect its text
/// nodes. Either way the result is a readable snapshot for the task body.
fn description_to_text(value: Option<&serde_json::Value>) -> String {
    fn walk(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::String(text) => {
                out.push_str(text);
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            serde_json::Value::Object(map) => {
                // ADF text nodes carry the prose under "text"; "type":"paragraph"
                // and the like just nest "content". A newline after each block
                // keeps paragraphs readable.
                if let Some(serde_json::Value::String(text)) = map.get("text") {
                    out.push_str(text);
                }
                if let Some(content) = map.get("content") {
                    walk(content, out);
                    out.push('\n');
                }
            }
            _ => {}
        }
    }

    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => {
            let mut out = String::new();
            walk(other, &mut out);
            out.trim().to_string()
        }
        None => String::new(),
    }
}

// --- Wire DTOs (private) -----------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawMyself {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    name: Option<String>,
    #[serde(rename = "emailAddress")]
    email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BoardPage {
    #[serde(default)]
    values: Vec<RawBoard>,
    #[serde(rename = "isLast", default)]
    is_last: bool,
}

#[derive(Debug, Deserialize)]
struct RawBoard {
    id: i64,
    name: String,
    location: Option<BoardLocation>,
}

#[derive(Debug, Deserialize)]
struct BoardLocation {
    #[serde(rename = "projectKey")]
    project_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IssuePage {
    #[serde(default)]
    issues: Vec<RawIssue>,
    #[serde(default)]
    total: i64,
}

#[derive(Debug, Deserialize)]
struct RawIssue {
    key: String,
    fields: IssueFields,
}

#[derive(Debug, Deserialize)]
struct IssueFields {
    summary: Option<String>,
    status: Option<IssueStatus>,
    description: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct IssueStatus {
    name: String,
}

#[derive(Debug, Deserialize)]
struct TransitionsResponse {
    #[serde(default)]
    transitions: Vec<Transition>,
}

#[derive(Debug, Deserialize)]
struct Transition {
    id: String,
    name: String,
    to: Option<TransitionTo>,
}

#[derive(Debug, Deserialize)]
struct TransitionTo {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(deployment: JiraDeployment) -> JiraConfig {
        JiraConfig {
            deployment,
            base_url: "https://acme.atlassian.net".to_string(),
            email: "bot@acme.com".to_string(),
            token: "secret-token".to_string(),
        }
    }

    #[test]
    fn cloud_uses_basic_auth_and_v3() {
        let cloud = config(JiraDeployment::Cloud);
        assert_eq!(cloud.api_base(), "/rest/api/3");
        // Basic auth = base64("email:token").
        let expected = STANDARD.encode("bot@acme.com:secret-token");
        assert_eq!(cloud.auth_header(), format!("Basic {expected}"));
    }

    #[test]
    fn server_uses_bearer_auth_and_v2() {
        let server = config(JiraDeployment::Server);
        assert_eq!(server.api_base(), "/rest/api/2");
        assert_eq!(server.auth_header(), "Bearer secret-token");
    }

    #[test]
    fn agile_api_path_is_versionless() {
        // The Agile API is unversioned per deployment, unlike the core REST API.
        assert_eq!(AGILE_API, "/rest/agile/1.0");
    }

    #[test]
    fn status_maps_to_column_case_insensitively() {
        let mut map = HashMap::new();
        map.insert("In Progress".to_string(), TaskColumn::InProgress);
        map.insert("Done".to_string(), TaskColumn::Done);
        assert_eq!(
            column_for_status(&map, "in progress"),
            TaskColumn::InProgress
        );
        assert_eq!(column_for_status(&map, "Done"), TaskColumn::Done);
        // Unknown statuses fall back to Available so the ticket is still placed.
        assert_eq!(column_for_status(&map, "Backlog"), TaskColumn::Available);
    }

    #[test]
    fn column_maps_back_to_a_status_deterministically() {
        let mut map = HashMap::new();
        map.insert("Selected".to_string(), TaskColumn::Todo);
        map.insert("Ready".to_string(), TaskColumn::Todo);
        map.insert("Done".to_string(), TaskColumn::Done);
        // Two statuses map to Todo; the smaller name wins, so it is stable.
        assert_eq!(
            status_for_column(&map, TaskColumn::Todo).as_deref(),
            Some("Ready")
        );
        assert_eq!(
            status_for_column(&map, TaskColumn::Done).as_deref(),
            Some("Done")
        );
        assert_eq!(status_for_column(&map, TaskColumn::InReview), None);
    }

    #[test]
    fn config_requires_enabled_url_and_token() {
        let mut settings = sample_settings();
        settings.jira_enabled = false;
        assert!(JiraConfig::from_settings(&settings, "tok").is_none());

        settings.jira_enabled = true;
        settings.jira_base_url = "  https://acme.atlassian.net/  ".to_string();
        let built = JiraConfig::from_settings(&settings, "tok").expect("config");
        // Trailing slash and surrounding whitespace are trimmed.
        assert_eq!(built.base_url, "https://acme.atlassian.net");

        assert!(JiraConfig::from_settings(&settings, "").is_none());
    }

    #[test]
    fn description_flattens_adf_and_strings() {
        assert_eq!(
            description_to_text(Some(&serde_json::json!("plain text"))),
            "plain text"
        );
        let adf = serde_json::json!({
            "type": "doc",
            "content": [
                { "type": "paragraph", "content": [ { "type": "text", "text": "Hello" } ] },
                { "type": "paragraph", "content": [ { "type": "text", "text": "World" } ] }
            ]
        });
        assert_eq!(description_to_text(Some(&adf)), "Hello\nWorld");
        assert_eq!(description_to_text(None), "");
    }

    fn sample_settings() -> Settings {
        use chrono::Utc;
        use sqlx::types::Json;
        Settings {
            org_name: String::new(),
            global_instructions: String::new(),
            default_review_policy: crate::db::models::ReviewPolicy::None,
            agent_paused: false,
            claude_model: String::new(),
            workspace_image_tag: String::new(),
            base_setup_script: String::new(),
            config_repo_url: String::new(),
            default_branch_template: String::new(),
            config_repo_error: None,
            current_session_id: None,
            updated_at: Utc::now(),
            claude_token_set: false,
            claude_auth_mode: crate::db::models::ClaudeAuthMode::Subscription,
            claude_usage_token_set: false,
            github_token_set: false,
            availability_enabled: false,
            availability_timezone: "UTC".to_string(),
            availability_windows: Json(Vec::new()),
            availability_skip_dates: Json(Vec::new()),
            network_access_level: crate::db::models::NetworkAccessLevel::Full,
            network_access_domains: Json(Vec::new()),
            network_access_include_defaults: true,
            usage_limit_pause_enabled: false,
            usage_limit_threshold: 80,
            usage_paused_until: None,
            post_thoughts_enabled: false,
            close_issue_on_done: true,
            jira_enabled: true,
            jira_deployment: JiraDeployment::Cloud,
            jira_base_url: "https://acme.atlassian.net".to_string(),
            jira_email: "bot@acme.com".to_string(),
            jira_token_set: true,
            github_webhook_secret_set: false,
            jira_webhook_secret_set: false,
            attention_sound_enabled: true,
            completion_sound_enabled: true,
            attention_sound_custom: false,
            completion_sound_custom: false,
            jira_token_preview: None,
            claude_token_preview: None,
            github_token_preview: None,
            cooldown_until: None,
        }
    }
}
