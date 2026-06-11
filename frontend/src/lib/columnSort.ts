// Per-column kanban sorting: the sort levels, the cycle order the column header
// button steps through, the comparators, and session persistence. Shared by the
// board page and the `ColumnSort` button so both agree on the levels.
import type { Task } from './types'

export type SortKey =
  | 'custom'
  | 'id_asc'
  | 'id_desc'
  | 'created_asc'
  | 'created_desc'
  | 'updated_asc'
  | 'updated_desc'

// The order the button cycles through, looping back to the start. Left-click
// steps forward, right-click steps backward.
export const SORT_CYCLE: SortKey[] = [
  'custom',
  'id_asc',
  'id_desc',
  'created_asc',
  'created_desc',
  'updated_asc',
  'updated_desc'
]

export type SortDirection = 'asc' | 'desc' | null

// What the button shows: a short word and the direction (rendered as an arrow).
// "custom" reads as "Auto" and carries no direction.
export const SORT_META: Record<SortKey, { label: string; direction: SortDirection }> = {
  custom: { label: 'Auto', direction: null },
  id_asc: { label: 'Id', direction: 'asc' },
  id_desc: { label: 'Id', direction: 'desc' },
  created_asc: { label: 'Created', direction: 'asc' },
  created_desc: { label: 'Created', direction: 'desc' },
  updated_asc: { label: 'Updated', direction: 'asc' },
  updated_desc: { label: 'Updated', direction: 'desc' }
}

export function nextSort(key: SortKey): SortKey {
  const index = SORT_CYCLE.indexOf(key)
  return SORT_CYCLE[(index + 1) % SORT_CYCLE.length]
}

export function prevSort(key: SortKey): SortKey {
  const index = SORT_CYCLE.indexOf(key)
  return SORT_CYCLE[(index - 1 + SORT_CYCLE.length) % SORT_CYCLE.length]
}

// External ids are ticket numbers ("42") or keys ("PROJ-123"); compare them
// naturally so 2 < 10 and "PROJ-2" < "PROJ-10".
function compareId(a: Task, b: Task): number {
  return a.external_id.localeCompare(b.external_id, undefined, { numeric: true, sensitivity: 'base' })
}

function compareDate(a: string, b: string): number {
  return new Date(a).getTime() - new Date(b).getTime()
}

// Returns a new array sorted per `key`. "custom" restores the board's manual
// order (ascending by fractional position). `Array.sort` is stable, so equal
// keys keep their relative order.
export function sortTasks(tasks: Task[], key: SortKey): Task[] {
  const sorted = [...tasks]
  switch (key) {
    case 'custom':
      sorted.sort((a, b) => a.position - b.position)
      break
    case 'id_asc':
      sorted.sort((a, b) => compareId(a, b))
      break
    case 'id_desc':
      sorted.sort((a, b) => compareId(b, a))
      break
    case 'created_asc':
      sorted.sort((a, b) => compareDate(a.created_at, b.created_at))
      break
    case 'created_desc':
      sorted.sort((a, b) => compareDate(b.created_at, a.created_at))
      break
    case 'updated_asc':
      sorted.sort((a, b) => compareDate(a.updated_at, b.updated_at))
      break
    case 'updated_desc':
      sorted.sort((a, b) => compareDate(b.updated_at, a.updated_at))
      break
  }
  return sorted
}

// --- Session persistence -----------------------------------------------------
// One entry per column, so a chosen sort survives a refresh (per the issue) but
// not a new tab/session.
const STORAGE_PREFIX = 'seraphim.columnSort.'

export function loadSort(columnKey: string): SortKey {
  if (typeof sessionStorage === 'undefined') {
    return 'custom'
  }
  const stored = sessionStorage.getItem(STORAGE_PREFIX + columnKey)
  return stored && (SORT_CYCLE as string[]).includes(stored) ? (stored as SortKey) : 'custom'
}

export function saveSort(columnKey: string, key: SortKey): void {
  if (typeof sessionStorage === 'undefined') {
    return
  }
  sessionStorage.setItem(STORAGE_PREFIX + columnKey, key)
}
