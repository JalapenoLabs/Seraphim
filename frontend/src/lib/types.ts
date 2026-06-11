// Domain types mirroring the Rust API's JSON (snake_case throughout).

export type TaskColumn = 'available' | 'todo' | 'in_progress' | 'in_review' | 'done' | 'ignored'

export type TaskStatus =
  | 'queued'
  | 'preparing'
  | 'working'
  | 'waiting_for_input'
  | 'opening_pr'
  | 'awaiting_review'
  | 'ci_failing'
  | 'ci_blocked'
  | 'merging'
  | 'done'
  | 'failed'

// Friendly status labels for the card badge.
export const STATUS_LABELS = {
  queued: 'queued',
  preparing: 'preparing',
  working: 'working',
  waiting_for_input: 'waiting for input',
  opening_pr: 'opening PR',
  awaiting_review: 'awaiting review',
  ci_failing: 'CI failing',
  ci_blocked: 'CI blocked',
  merging: 'merging',
  done: 'done',
  failed: 'failed'
} as const satisfies Record<TaskStatus, string>

// Tailwind classes coloring each status badge (used with Badge variant="outline").
export const STATUS_BADGE = {
  queued: 'border-border text-muted-foreground',
  preparing: 'border-primary/40 text-primary',
  working: 'border-primary/40 text-primary',
  waiting_for_input: 'border-warning/40 text-warning',
  opening_pr: 'border-primary/40 text-primary',
  awaiting_review: 'border-warning/40 text-warning',
  ci_failing: 'border-warning/40 text-warning',
  ci_blocked: 'border-destructive/40 text-destructive',
  merging: 'border-primary/40 text-primary',
  done: 'border-success/40 text-success',
  failed: 'border-destructive/40 text-destructive'
} as const satisfies Record<TaskStatus, string>

// The label and badge classes for a task's *source ticket* state (the card's
// second badge, distinct from the agent `status` above). GitHub issues are
// always "open"/"closed"; Jira (when wired up) reports project-defined workflow
// names, so those are shown verbatim. Returns null when the state is unknown.
export function ticketStateBadge(task: Task): { label: string; class: string } | null {
  const state = task.external_state
  if (!state) return null
  if (task.source_kind === 'github') {
    return state === 'closed'
      ? { label: 'Closed', class: 'border-muted-foreground/40 text-muted-foreground' }
      : { label: 'Open', class: 'border-success/40 text-success' }
  }
  return { label: state, class: 'border-border text-muted-foreground' }
}

export type ReviewPolicy = 'auto_squash_merge' | 'human_review' | 'none'

// How much of the internet the agent's workspace may reach (modeled on Claude
// Code on the web's network access levels).
export type NetworkAccessLevel = 'none' | 'trusted' | 'full' | 'custom'

// Which Jira deployment we talk to (decides auth scheme + REST version).
export type JiraDeployment = 'cloud' | 'server'

// A Jira board we follow. `status_map` maps a Jira status name to one of our
// kanban columns; `repo_ids` is the set of repos a ticket from this board targets.
export type JiraBoard = {
  id: string
  board_id: number
  name: string
  project_key: string
  sync_enabled: boolean
  status_map: Record<string, TaskColumn>
  repo_ids: string[]
  created_at: string
  updated_at: string
}

export type SourceKind = 'github' | 'jira'

export type Task = {
  id: string
  source_kind: SourceKind
  external_id: string
  repo_id: string | null
  title: string
  body_snapshot: string
  url: string
  // The source ticket's own state, separate from the agent `status` below: for
  // GitHub "open"/"closed", for Jira the workflow status name. Null until known.
  external_state: string | null
  board_column: TaskColumn
  position: number
  status: TaskStatus
  branch: string | null
  pr_url: string | null
  error: string | null
  ci_fix_attempts: number
  hold: boolean
  session_id: string | null
  started_at: string | null
  finished_at: string | null
  last_activity_at: string | null
  created_at: string
  updated_at: string
}

// A recurring weekly window the agent is allowed to work in. Minutes are counted
// from local midnight in the operator's configured time zone; weekday is 0 =
// Monday through 6 = Sunday (matching the Rust side).
export type AvailabilityWindow = {
  weekday: number
  start_minute: number
  end_minute: number
}

export type Settings = {
  org_name: string
  global_instructions: string
  default_review_policy: ReviewPolicy
  agent_paused: boolean
  claude_model: string
  workspace_image_tag: string
  base_setup_script: string
  config_repo_url: string
  default_branch_template: string
  config_repo_error: string | null
  current_session_id: string | null
  updated_at: string
  claude_token_set: boolean
  github_token_set: boolean
  availability_enabled: boolean
  availability_timezone: string
  availability_windows: AvailabilityWindow[]
  // ISO calendar dates ("YYYY-MM-DD") to skip entirely.
  availability_skip_dates: string[]
  // Outbound network access policy for the agent's workspace.
  network_access_level: NetworkAccessLevel
  // Operator-defined allow-list (used only when the level is "custom").
  network_access_domains: string[]
  // For "custom": also allow the built-in package-manager/registry domains.
  network_access_include_defaults: boolean
  // Auto-pause new work when the subscription usage limit is (nearly) hit.
  usage_limit_pause_enabled: boolean
  // Utilization percent (0-100) at which to auto-pause.
  usage_limit_threshold: number
  // While set and in the future, the agent is auto-paused for usage (ISO string).
  usage_paused_until: string | null
  // Post a per-turn summary of the agent's reasoning back to the source issue.
  post_thoughts_enabled: boolean
  // Jira connection. Cloud uses email + API token; Server/DC uses a PAT.
  jira_enabled: boolean
  jira_deployment: JiraDeployment
  jira_base_url: string
  jira_email: string
  jira_token_set: boolean
  // Masked previews of the stored tokens (e.g. "sk-ant-****abcd"), or null when
  // unset. The raw tokens are never sent.
  claude_token_preview: string | null
  github_token_preview: string | null
  jira_token_preview: string | null
  // Runtime signal: while set and in the future, the agent is in a brief global
  // cooldown after a transient rate limit, auto-retrying the current turn.
  cooldown_until: string | null
}

// A user-defined environment variable as the UI sees it. For a secret, `value`
// is the masked preview returned by the API, never the raw secret.
export type EnvVar = {
  key: string
  value: string
  is_secret: boolean
}

export type Repository = {
  id: string
  full_name: string
  clone_url: string
  default_branch: string
  // Per-repo override of the global branch template; null inherits it.
  branch_template: string | null
  setup_script: string
  instructions: string
  review_policy: ReviewPolicy | null
  enabled: boolean
  sync_issues: boolean
  issue_labels: string[]
  created_at: string
  updated_at: string
}

export type AgentEvent = {
  id: number
  turn_id: string
  seq: number
  type: string
  payload: unknown
  created_at: string
}

// A setup recommendation the agent made after finishing a task.
export type EnvSuggestion = {
  id: string
  task_id: string
  title: string
  detail: string
  acknowledged: boolean
  created_at: string
  acknowledged_at: string | null
}

// A decision the agent escalated to the user.
export type QuestionStatus = 'pending' | 'answered' | 'declined'
export type AnswerKind = 'option' | 'custom' | 'declined'

export type QuestionOption = {
  title: string
  description: string
}

export type Question = {
  id: string
  task_id: string
  prompt: string
  options: QuestionOption[]
  status: QuestionStatus
  answer_kind: AnswerKind | null
  answer: string | null
  acknowledged: boolean
  created_at: string
  answered_at: string | null
}

// A pending question plus its task title, for the notifications sidebar.
export type PendingQuestion = {
  id: string
  task_id: string
  task_title: string
  prompt: string
  options: QuestionOption[]
  created_at: string
}

export type BoardResponse = {
  tasks: Task[]
  settings: Settings
  // Unacknowledged suggestion counts keyed by task id (tasks with none omitted).
  suggestion_counts: Record<string, number>
}

export type TaskDetail = {
  task: Task
  events: AgentEvent[]
  suggestions: EnvSuggestion[]
  questions: Question[]
}

// The kanban lanes, in display order, with human-readable labels.
export const COLUMNS: { key: TaskColumn; label: string }[] = [
  { key: 'available', label: 'Available' },
  { key: 'todo', label: 'To Do' },
  { key: 'in_progress', label: 'In Progress' },
  { key: 'in_review', label: 'In Review' },
  { key: 'done', label: 'Done' },
  { key: 'ignored', label: 'Ignored' }
]

// Known Claude models for the settings dropdown: friendly labels shown to the
// user, coded model ids sent to the agent. Custom ids are still allowed.
// Fable 5, Opus 4.x, and Sonnet 4.6 are 1M-context; Haiku 4.5 is 200K. The
// `[1m]` suffix is Claude Code's way to opt Opus into its 1M window.
export const KNOWN_MODELS: { value: string; label: string }[] = [
  { value: 'claude-opus-4-8[1m]', label: 'Claude Opus 4.8 (1M)' },
  { value: 'claude-opus-4-8', label: 'Claude Opus 4.8 (200K)' },
  { value: 'claude-opus-4-7[1m]', label: 'Claude Opus 4.7 (1M)' },
  { value: 'claude-fable-5', label: 'Claude Fable 5 (1M)' },
  { value: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6 (1M)' },
  { value: 'claude-haiku-4-5', label: 'Claude Haiku 4.5 (200K)' }
]

// --- GitHub issue thread (conversation view) ---------------------------------

export type IssueUser = {
  login: string
  avatar_url: string
  html_url: string
}

export type IssueLabel = {
  name: string
  color: string
}

export type IssueComment = {
  user: IssueUser
  body: string | null
  created_at: string
  author_association: string
}

export type IssueDetail = {
  number: number
  title: string
  state: 'open' | 'closed'
  user: IssueUser
  body: string | null
  created_at: string
  author_association: string
  labels: IssueLabel[]
  assignees: IssueUser[]
  milestone: { title: string } | null
}

export type IssueThread = {
  issue: IssueDetail
  comments: IssueComment[]
}

export type ConfigBundle = {
  settings: Record<string, unknown>
  repositories: unknown[]
}
