// Maps Seraphim's live agent activity stream into runewood events for the watch
// page's activity forest (issue #180). This is the only place Seraphim specifics
// touch runewood; the library stays a generic event renderer.
//
// Pure and defensive by design: the stream-json schema drifts across Claude Code
// versions, so every unrecognized or malformed shape is dropped, never thrown
// (mirroring the Rust parser's `Other` handling). It is unit-tested in
// `mapEvent.test.ts`.

import type { RunewoodEvent } from 'runewood'

// Repo ignore globs so the forest stays clean (build output, deps, lockfiles).
// Passed to runewood's `exclude` option.
export const DEFAULT_EXCLUDES = [
  '**/node_modules/**',
  '**/target/**',
  '**/dist/**',
  '**/build/**',
  '**/.git/**',
  '**/.svelte-kit/**',
  '**/__pycache__/**',
  '**/.venv/**',
  '**/*.lock'
]

// The shape of one `activity` SSE frame's data (loose: read defensively).
export type ActivityEvent = {
  task_id?: string
  event?: {
    type?: string
    payload?: unknown
    created_at?: string
  }
}

// How each Claude tool maps to a runewood action and where its path comes from.
// A `pulse` tool (Bash) has no path and renders on the actor, not the tree.
type ToolRule =
  | { action: 'create' | 'modify' | 'scan'; pathKeys: readonly string[] }
  | { action: 'pulse' }

const TOOL_RULES: Record<string, ToolRule> = {
  Write: { action: 'create', pathKeys: ['file_path', 'path'] },
  Edit: { action: 'modify', pathKeys: ['file_path', 'path'] },
  MultiEdit: { action: 'modify', pathKeys: ['file_path', 'path'] },
  NotebookEdit: { action: 'modify', pathKeys: ['notebook_path', 'file_path', 'path'] },
  Read: { action: 'scan', pathKeys: ['file_path', 'path'] },
  Grep: { action: 'scan', pathKeys: ['path'] },
  Glob: { action: 'scan', pathKeys: ['path'] },
  Bash: { action: 'pulse' }
}

// Reads a string property off a loose record, or null when absent/not a string.
function readString(record: Record<string, unknown> | undefined, key: string): string | null {
  const value = record?.[key]
  return typeof value === 'string' ? value : null
}

// Normalizes a tool path to a forest path whose first segment is the repo: strips
// a leading `/workspace/` (the agent's cwd, where repos are cloned flat), keeps
// already-relative paths as-is, and drops anything that escapes the workspace (an
// absolute path elsewhere, or a `..` segment). Returns null to drop the event.
export function normalizePath(raw: string | null | undefined): string | null {
  if (!raw) {
    return null
  }
  let path = raw.trim()
  if (!path) {
    return null
  }

  const WORKSPACE_PREFIX = '/workspace/'
  if (path.startsWith(WORKSPACE_PREFIX)) {
    path = path.slice(WORKSPACE_PREFIX.length)
  } else if (path.startsWith('/')) {
    // Absolute path outside the workspace (or `/workspace` itself): escapes, drop.
    return null
  }

  path = path.replace(/^\/+/, '')
  if (!path || path.split('/').some((segment) => segment === '..')) {
    return null
  }
  return path
}

// Maps one activity frame to a single runewood event, or null to drop it. Only
// `tool_use` frames produce events (see `TOOL_RULES`); `taskTitle` is used as the
// actor's display label (Bash pulses prefer the command).
export function mapActivityEvent(
  input: ActivityEvent | null | undefined,
  taskTitle?: string
): RunewoodEvent | null {
  const event = input?.event
  if (!event || event.type !== 'tool_use') {
    return null
  }
  const actor = input?.task_id
  if (!actor) {
    return null
  }

  const payload = (event.payload ?? {}) as Record<string, unknown>
  const name = readString(payload, 'name')
  if (!name) {
    return null
  }
  const rule = TOOL_RULES[name]
  if (!rule) {
    return null
  }

  const parsed = event.created_at ? Date.parse(event.created_at) : Number.NaN
  const at = Number.isNaN(parsed) ? Date.now() : parsed

  const toolInput = (payload.input ?? {}) as Record<string, unknown>

  if (rule.action === 'pulse') {
    // A shell command: a contributor pulse with no file target.
    const command = readString(toolInput, 'command')
    return { at, actor, action: 'pulse', label: command ?? taskTitle }
  }

  const rawPath = rule.pathKeys.map((key) => readString(toolInput, key)).find((value) => value)
  const path = normalizePath(rawPath)
  if (!path) {
    return null
  }
  return { at, actor, action: rule.action, path, label: taskTitle }
}
