// Domain types mirroring the Rust API's JSON (snake_case throughout).

export type TaskColumn = 'available' | 'todo' | 'in_progress' | 'in_review' | 'done' | 'ignored'

export type TaskStatus =
  | 'queued'
  | 'preparing'
  | 'working'
  | 'opening_pr'
  | 'awaiting_review'
  | 'merging'
  | 'done'
  | 'failed'

export type ReviewPolicy = 'auto_squash_merge' | 'human_review' | 'none'

export type SourceKind = 'github' | 'jira'

export type Task = {
  id: string
  source_kind: SourceKind
  external_id: string
  repo_id: string | null
  title: string
  body_snapshot: string
  url: string
  board_column: TaskColumn
  position: number
  status: TaskStatus
  branch: string | null
  pr_url: string | null
  error: string | null
  hold: boolean
  session_id: string | null
  started_at: string | null
  finished_at: string | null
  last_activity_at: string | null
  created_at: string
  updated_at: string
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
  current_session_id: string | null
  updated_at: string
  claude_token_set: boolean
  github_token_set: boolean
}

export type Repository = {
  id: string
  full_name: string
  clone_url: string
  default_branch: string
  branch_template: string
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

export type BoardResponse = {
  tasks: Task[]
  settings: Settings
}

export type TaskDetail = {
  task: Task
  events: AgentEvent[]
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

export type ConfigBundle = {
  settings: Record<string, unknown>
  repositories: unknown[]
}
