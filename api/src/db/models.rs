//! Domain types mirroring the Postgres schema.
//!
//! Each enum maps to a Postgres `ENUM` type of the same name (snake_case
//! variants), and each struct maps to a table row via [`sqlx::FromRow`]. All
//! types serialize to the snake_case JSON the frontend consumes.

use chrono::{DateTime, Utc};
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

/// The single-row org / environment profile.
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
}

/// A repository the agent is allowed to clone and work in.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Repository {
    pub id: Uuid,
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: String,
    pub branch_template: String,
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

/// A kanban card: one issue the agent may work.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Task {
    pub id: Uuid,
    pub source_kind: SourceKind,
    pub external_id: String,
    pub repo_id: Option<Uuid>,
    pub title: String,
    pub body_snapshot: String,
    pub url: String,
    pub board_column: TaskColumn,
    pub position: f64,
    pub status: TaskStatus,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub error: Option<String>,
    pub hold: bool,
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
