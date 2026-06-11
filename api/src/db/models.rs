//! Domain types mirroring the Postgres schema.
//!
//! Each enum maps to a Postgres `ENUM` type of the same name (snake_case
//! variants), and each struct maps to a table row via [`sqlx::FromRow`]. All
//! types serialize to the snake_case JSON the frontend consumes.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

/// Where an issue originates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "source_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Github,
    Jira,
    /// A ticket that lives only in our database, with no external tracker.
    Internal,
}

/// What Seraphim does with a pull request once the agent opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "review_policy", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ReviewPolicy {
    /// Squash-merge automatically once CI is green (e.g. JalapenoLabs).
    AutoSquashMerge,
    /// Leave the PR open for a human to review (e.g. MooreslabAI).
    HumanReview,
    /// Open the PR and take no further action.
    None,
}

/// How much of the internet the agent's workspace may reach, modeled on Claude
/// Code on the web's network access levels. Translated into the agent's
/// `~/.claude/settings.json` permissions during provisioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "network_access_level", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccessLevel {
    /// No outbound network access.
    None,
    /// The built-in allow-list of package registries, version-control hosts, and
    /// cloud SDKs only.
    Trusted,
    /// Any destination (unrestricted).
    Full,
    /// The operator's own allow-list, optionally plus the built-in defaults.
    Custom,
}

/// Which Jira deployment we are talking to, which decides both the auth scheme
/// and the REST API version. Cloud uses Basic auth (email + API token) and REST
/// v3; Server / Data Center uses a Bearer personal access token and REST v2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "jira_deployment", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JiraDeployment {
    Cloud,
    Server,
}

/// The kanban lane a card sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "task_column", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TaskColumn {
    Available,
    Todo,
    InProgress,
    InReview,
    Done,
    /// Parked: synced but deliberately set aside; the agent never pulls these.
    Ignored,
}

/// Fine-grained operational state while a task is being worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "task_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Preparing,
    Working,
    /// Parked while the agent waits for the user to answer its question(s).
    WaitingForInput,
    OpeningPr,
    AwaitingReview,
    /// The PR's CI is red and the task is queued for an agent fix turn.
    CiFailing,
    /// The agent stopped fixing CI (out of scope, or the retry cap hit); the PR
    /// is left in review for a human.
    CiBlocked,
    Merging,
    Done,
    Failed,
}

/// Lifecycle of a question the agent escalated to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "question_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum QuestionStatus {
    /// Awaiting the user's answer.
    Pending,
    /// The user picked an option or typed a custom answer.
    Answered,
    /// The user declined to choose and wants to discuss it instead.
    Declined,
}

/// How the user responded to a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "answer_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AnswerKind {
    /// One of the agent's suggested options.
    Option,
    /// Free-form text the user typed instead.
    Custom,
    /// The user declined to answer and asked to discuss it.
    Declined,
}

/// A recurring weekly window during which the agent may pick up new work.
///
/// Times are minutes from local midnight in the operator's configured time zone,
/// so they stay stable across daylight-saving shifts (the zone, not the offset,
/// is stored). `start_minute` is inclusive and `end_minute` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityWindow {
    /// Day of week, `0` = Monday through `6` = Sunday
    /// (matches `chrono::Weekday::num_days_from_monday`).
    pub weekday: u8,
    /// Inclusive start of the window, in minutes from midnight (`0..=1440`).
    pub start_minute: u16,
    /// Exclusive end of the window, in minutes from midnight (`0..=1440`).
    pub end_minute: u16,
}

/// The single-row org / environment profile.
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the settings DB row; each flag is an independent stored column"
)]
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Settings {
    pub org_name: String,
    pub global_instructions: String,
    pub default_review_policy: ReviewPolicy,
    pub agent_paused: bool,
    pub claude_model: String,
    pub workspace_image_tag: String,
    pub base_setup_script: String,
    /// Git URL of the `~/.claude` config repo cloned into the workspace.
    pub config_repo_url: String,
    /// Branch template applied to repos auto-discovered from an org.
    pub default_branch_template: String,
    /// Config-repo setup error, if any (NULL = healthy / no config repo).
    pub config_repo_error: Option<String>,
    pub current_session_id: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// Whether a Claude OAuth token is stored (the token itself is never sent).
    pub claude_token_set: bool,
    /// Whether a GitHub token is stored (the token itself is never sent).
    pub github_token_set: bool,
    /// When true, the agent only works during [`Self::availability_windows`].
    pub availability_enabled: bool,
    /// IANA time zone the windows and skip dates are interpreted in (e.g.
    /// `America/Denver`). The database itself always stores UTC.
    pub availability_timezone: String,
    /// Weekly availability windows. Empty means "any time of day".
    pub availability_windows: Json<Vec<AvailabilityWindow>>,
    /// Calendar dates to skip entirely (vacations, holidays).
    pub availability_skip_dates: Json<Vec<NaiveDate>>,
    /// Outbound network access level enforced in the agent's workspace.
    pub network_access_level: NetworkAccessLevel,
    /// Operator-defined allow-list (used only when the level is `custom`).
    pub network_access_domains: Json<Vec<String>>,
    /// For `custom`: also allow the built-in package-manager/registry domains.
    pub network_access_include_defaults: bool,
    /// Auto-pause new work when the subscription usage limit is (nearly) hit.
    pub usage_limit_pause_enabled: bool,
    /// Utilization percent (0-100) at which to auto-pause; Claude's own
    /// early-warning fires around 80%.
    pub usage_limit_threshold: i32,
    /// Runtime state: while set and in the future, the agent is auto-paused for
    /// usage and pulls no new work; cleared once the limit window resets.
    pub usage_paused_until: Option<DateTime<Utc>>,
    /// Post a per-turn summary of the agent's reasoning back to the source issue.
    pub post_thoughts_enabled: bool,
    /// Whether the Jira integration is turned on (a connection is configured).
    pub jira_enabled: bool,
    /// Cloud vs Server/Data Center, deciding auth scheme and REST version.
    pub jira_deployment: JiraDeployment,
    /// Jira site base URL, e.g. `https://acme.atlassian.net`.
    pub jira_base_url: String,
    /// Account email (the Basic-auth username on Cloud; unused on Server).
    pub jira_email: String,
    /// Whether a Jira API token / PAT is stored (the token itself is never sent).
    pub jira_token_set: bool,
    /// Masked preview of the stored Jira API token. Filled like
    /// [`Self::claude_token_preview`].
    #[sqlx(default)]
    pub jira_token_preview: Option<String>,
    /// Masked preview of the stored Claude token, e.g. `sk-ant-****abcd`. Not a
    /// DB column; the settings handler fills it from the raw token so an operator
    /// can recognize what is stored without it being revealed.
    #[sqlx(default)]
    pub claude_token_preview: Option<String>,
    /// Masked preview of the stored GitHub token. Filled like
    /// [`Self::claude_token_preview`].
    #[sqlx(default)]
    pub github_token_preview: Option<String>,
    /// Runtime UI signal: while set and in the future, the agent is in a brief
    /// global cooldown after a transient rate limit, about to retry the current
    /// turn. Not a DB column; the board handler fills it from the live in-memory
    /// value on [`crate::state::AppState`].
    #[sqlx(default)]
    pub cooldown_until: Option<DateTime<Utc>>,
}

/// A user-defined environment variable injected into the agent's execs.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EnvVar {
    pub id: Uuid,
    pub key: String,
    pub value: String,
    /// When true, the value is scrubbed from output and only ever returned masked.
    pub is_secret: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One environment variable as submitted by the settings UI.
///
/// `value` is optional so a secret can be left unchanged: `None` means "keep the
/// stored value" (the UI never receives raw secrets to send back), while `Some`
/// sets a new value.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvVarWrite {
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
}

/// A repository the agent is allowed to clone and work in.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Repository {
    pub id: Uuid,
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: String,
    /// Per-repo branch-name template, or `None` to inherit
    /// [`Settings::default_branch_template`].
    pub branch_template: Option<String>,
    pub setup_script: String,
    pub instructions: String,
    pub review_policy: Option<ReviewPolicy>,
    pub enabled: bool,
    /// Poll this repo for issues during sync.
    pub sync_issues: bool,
    /// Only sync issues carrying all of these labels (empty = no filter).
    pub issue_labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What a repository delete will purge, so the UI can spell it out before the
/// user confirms. Counts the repo's tasks and everything that cascades from them.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RepoDeletionImpact {
    pub tasks: i64,
    pub turns: i64,
    pub events: i64,
    pub questions: i64,
    pub suggestions: i64,
}

/// One comment on an internal ticket. `author` is `"user"` (the operator) or
/// `"agent"` (Seraphim).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct InternalComment {
    pub id: Uuid,
    pub task_id: Uuid,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// A Jira board we follow for tickets.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct JiraBoard {
    pub id: Uuid,
    /// Jira's own numeric board id.
    pub board_id: i64,
    pub name: String,
    pub project_key: String,
    /// Poll this board for issues during sync.
    pub sync_enabled: bool,
    /// Maps a Jira status name (e.g. "In Progress") to one of our kanban lanes.
    /// Unmapped statuses fall back to `Available` on first sync.
    pub status_map: Json<HashMap<String, TaskColumn>>,
    /// The repositories a ticket from this board may target. A single ticket can
    /// span several repos (e.g. a shared "BUG" board), so this is a set.
    pub repo_ids: Json<Vec<Uuid>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A kanban card: one issue the agent may work.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Task {
    pub id: Uuid,
    pub source_kind: SourceKind,
    pub external_id: String,
    pub repo_id: Option<Uuid>,
    /// For a Jira task, the followed board it came from, so a column move can map
    /// back to a Jira status and transition the ticket. `None` for GitHub tasks.
    pub jira_board_id: Option<Uuid>,
    pub title: String,
    pub body_snapshot: String,
    pub url: String,
    /// The source ticket's own state, distinct from the agent's `status`: for
    /// GitHub "open" / "closed", for Jira the workflow status name. `None` until
    /// a sync or state change records it.
    pub external_state: Option<String>,
    pub board_column: TaskColumn,
    pub position: f64,
    pub status: TaskStatus,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub error: Option<String>,
    /// Fix turns already spent on this task's failing CI (bounds retry thrash).
    pub ci_fix_attempts: i32,
    pub hold: bool,
    /// When true, while this task is in progress the agent pulls no new work, so
    /// dependent tasks wait until it finishes (queue serialization).
    pub blocking: bool,
    /// The operator's private scratchpad for this task. Stored only here and
    /// never written back to the source ticket.
    pub notes: String,
    pub session_id: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One Claude Code invocation against a task.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Turn {
    pub id: Uuid,
    pub task_id: Uuid,
    pub idx: i32,
    pub prompt: String,
    pub status: String,
    pub result_text: Option<String>,
    pub total_cost_usd: Option<f64>,
    pub token_usage: Option<Json<serde_json::Value>>,
    pub session_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A single parsed stream-json event, persisted for the live feed and history.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    pub turn_id: Uuid,
    pub seq: i32,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Json<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// A setup recommendation the agent made after finishing a task.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EnvSuggestion {
    pub id: Uuid,
    pub task_id: Uuid,
    pub title: String,
    pub detail: String,
    /// Checked off by the user; the board badge counts the unacknowledged ones.
    pub acknowledged: bool,
    pub created_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

/// One environment suggestion as posted by the agent's `seraphim-suggest`.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvSuggestionWrite {
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

/// One suggested answer the agent offers alongside a question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub title: String,
    #[serde(default)]
    pub description: String,
}

/// A decision the agent escalated to the user, stored on its task.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Question {
    pub id: Uuid,
    pub task_id: Uuid,
    pub prompt: String,
    /// Up to three suggested answers; the UI adds "something else" and "decline".
    pub options: Json<Vec<QuestionOption>>,
    pub status: QuestionStatus,
    pub answer_kind: Option<AnswerKind>,
    pub answer: Option<String>,
    /// Whether the answer has already been delivered to the agent on a resume.
    pub acknowledged: bool,
    pub created_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
}

/// A pending question plus its task's title, for the notifications sidebar.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PendingQuestion {
    pub id: Uuid,
    pub task_id: Uuid,
    pub task_title: String,
    pub prompt: String,
    pub options: Json<Vec<QuestionOption>>,
    pub created_at: DateTime<Utc>,
}
