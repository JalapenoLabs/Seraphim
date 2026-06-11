// Turning a Write/Edit/MultiEdit tool call into a line-level diff for the
// activity log, so an edit reads as a red/green patch instead of a "file
// updated" snippet.
//
// We only have what the tool call carried: for an Edit, the `old_string` and
// `new_string` it swapped; for a Write, the full new `content` (we never see the
// prior file, so a Write is shown as all-additions). That is enough for a
// faithful patch of the change itself, though not for the file's absolute line
// numbers, which only the agent's own editor knows.

export type DiffLineKind = 'add' | 'del' | 'context'

export type DiffLine = {
  kind: DiffLineKind
  text: string
}

export type EditDiff = {
  /** How the tool is labelled in the log: "Write" for new content, "Update" for an edit. */
  verb: 'Write' | 'Update'
  /** The edited file, shortened for display (the `/workspace/` prefix dropped). */
  path: string
  lines: DiffLine[]
  added: number
  removed: number
}

/** The tools whose calls we render as a diff. */
const DIFF_TOOLS = new Set(['Write', 'Edit', 'MultiEdit'])

// The longest-common-subsequence table is O(n*m); edits are tiny so that is
// nothing, but a pathologically large Write should not lock the tab. Past this
// many cells we skip the table and show a plain replace-all.
const LCS_CELL_CAP = 4_000_000

/**
 * Splits text into lines for diffing, dropping the phantom empty line a trailing
 * newline produces so "a\nb\n" is two lines, not three.
 */
function toLines(text: string): string[] {
  if (text === '') {
    return []
  }
  const lines = text.split('\n')
  if (lines.length > 1 && lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines
}

/**
 * A classic longest-common-subsequence line diff: shared lines become context,
 * lines only in `oldText` become deletions, lines only in `newText` additions.
 */
export function diffLines(oldText: string, newText: string): DiffLine[] {
  const a = toLines(oldText)
  const b = toLines(newText)

  if (a.length * b.length > LCS_CELL_CAP) {
    return [
      ...a.map((text): DiffLine => ({ kind: 'del', text })),
      ...b.map((text): DiffLine => ({ kind: 'add', text }))
    ]
  }

  // dp[i][j] = length of the LCS of a[i:] and b[j:].
  const dp: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0)
  )
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }

  // Walk the table, preferring deletions before additions so a replaced block
  // reads old-then-new, the way a unified diff does.
  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      lines.push({ kind: 'context', text: a[i] })
      i++
      j++
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      lines.push({ kind: 'del', text: a[i] })
      i++
    } else {
      lines.push({ kind: 'add', text: b[j] })
      j++
    }
  }
  while (i < a.length) {
    lines.push({ kind: 'del', text: a[i++] })
  }
  while (j < b.length) {
    lines.push({ kind: 'add', text: b[j++] })
  }
  return lines
}

function asString(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

/** Drops the workspace prefix so paths read as `repo/src/foo.rs`, not the full mount path. */
function shortenPath(path: string): string {
  return path.replace(/^\/workspace\//, '')
}

function summarize(verb: EditDiff['verb'], path: string, lines: DiffLine[]): EditDiff {
  let added = 0
  let removed = 0
  for (const line of lines) {
    if (line.kind === 'add') added++
    else if (line.kind === 'del') removed++
  }
  return { verb, path: shortenPath(path), lines, added, removed }
}

/**
 * Builds the diff model for a Write/Edit/MultiEdit `tool_use` payload, or returns
 * null for any other tool so the caller can fall back to its default rendering.
 */
export function editDiff(payload: Record<string, unknown>): EditDiff | null {
  const name = String(payload?.name ?? '')
  if (!DIFF_TOOLS.has(name)) {
    return null
  }
  const input = (payload?.input ?? {}) as Record<string, unknown>
  const path = asString(input.file_path)
  if (!path) {
    return null
  }

  if (name === 'Write') {
    const content = asString(input.content)
    if (content === null) {
      return null
    }
    // No prior file to compare against, so the new content is all additions.
    const lines = toLines(content).map((text): DiffLine => ({ kind: 'add', text }))
    return summarize('Write', path, lines)
  }

  if (name === 'Edit') {
    const oldString = asString(input.old_string)
    const newString = asString(input.new_string)
    if (oldString === null || newString === null) {
      return null
    }
    return summarize('Update', path, diffLines(oldString, newString))
  }

  // MultiEdit: a sequence of edits against one file; diff each and concatenate.
  const edits = Array.isArray(input.edits) ? input.edits : null
  if (!edits) {
    return null
  }
  const lines: DiffLine[] = []
  for (const raw of edits) {
    const edit = (raw ?? {}) as Record<string, unknown>
    const oldString = asString(edit.old_string)
    const newString = asString(edit.new_string)
    if (oldString === null || newString === null) {
      continue
    }
    lines.push(...diffLines(oldString, newString))
  }
  if (lines.length === 0) {
    return null
  }
  return summarize('Update', path, lines)
}
