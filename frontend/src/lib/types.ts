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

// Known Claude models for the settings dropdown; users can also enter a custom
// id (e.g. a model released after this build).
export const KNOWN_MODELS: string[] = [
  'claude-opus-4-8[1m]',
  'claude-opus-4-8',
  'claude-sonnet-4-6',
  'claude-haiku-4-5',
  'claude-fable-5'
]

export type ConfigBundle = {
  settings: Record<string, unknown>
  repositories: unknown[]
}
