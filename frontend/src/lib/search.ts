// The conditions the navbar issue search can be narrowed by, plus the pure
// helpers that apply and summarize them. The UI (the funnel panel) and the
// fuzzy search both build on this, so adding a new filter is a three-step,
// localized change: add a field here, a clause in `applyFilters`, and a section
// in the filter panel.
import type { Task, TaskColumn, TaskStatus } from './types'

// Every field is "empty means no constraint", so all-empty filters match every
// task. Multi-selects are OR within a field and AND across fields (e.g. author
// A or B, and status working). The date bounds are inclusive calendar dates.
export type SearchFilters = {
  // Logins of issue authors to include.
  authors: string[]
  // Agent statuses to include.
  statuses: TaskStatus[]
  // Board columns (kanban lanes) to include.
  columns: TaskColumn[]
  // Inclusive "created on or after" / "created on or before" dates ("YYYY-MM-DD").
  createdFrom: string
  createdTo: string
}

// A fresh, all-empty filter set. A factory (not a shared constant) so callers
// can safely mutate the returned arrays without aliasing each other.
export function emptyFilters(): SearchFilters {
  return { authors: [], statuses: [], columns: [], createdFrom: '', createdTo: '' }
}

// Narrow the tasks to those satisfying every active condition. Empty conditions
// are skipped, so with no filter set this is the identity.
export function applyFilters(tasks: Task[], filters: SearchFilters): Task[] {
  return tasks.filter((task) => {
    if (filters.authors.length && !(task.author_login && filters.authors.includes(task.author_login))) {
      return false
    }
    if (filters.statuses.length && !filters.statuses.includes(task.status)) {
      return false
    }
    if (filters.columns.length && !filters.columns.includes(task.board_column)) {
      return false
    }
    // `created_at` is a full ISO timestamp; compare on the calendar date alone so
    // both bounds include their whole day regardless of the time of creation.
    const createdDay = task.created_at.slice(0, 10)
    if (filters.createdFrom && createdDay < filters.createdFrom) {
      return false
    }
    if (filters.createdTo && createdDay > filters.createdTo) {
      return false
    }
    return true
  })
}

// How many conditions are active, for the funnel's badge. Each selected
// author/status/column counts once; the date range counts as one whether one
// bound or both are set.
export function countActiveFilters(filters: SearchFilters): number {
  return (
    filters.authors.length +
    filters.statuses.length +
    filters.columns.length +
    (filters.createdFrom || filters.createdTo ? 1 : 0)
  )
}

// An issue author as the filter panel lists them.
export type AuthorOption = { login: string; avatarUrl: string | null }

// The distinct issue authors present in the tasks, with an avatar each, sorted
// by login. Drives the author filter section so it only offers real choices.
export function distinctAuthors(tasks: Task[]): AuthorOption[] {
  const byLogin = new Map<string, AuthorOption>()
  for (const task of tasks) {
    if (task.author_login && !byLogin.has(task.author_login)) {
      byLogin.set(task.author_login, { login: task.author_login, avatarUrl: task.author_avatar_url })
    }
  }
  return [...byLogin.values()].sort((a, b) => a.login.localeCompare(b.login))
}
