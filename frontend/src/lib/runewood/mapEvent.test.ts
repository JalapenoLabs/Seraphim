import { describe, it, expect } from 'vitest'

import { mapActivityEvent, normalizePath, type ActivityEvent } from './mapEvent'

// Builds a `tool_use` activity frame for a given tool + input.
function toolUse(name: string, input: Record<string, unknown>, taskId = 'task-1'): ActivityEvent {
  return {
    task_id: taskId,
    event: {
      type: 'tool_use',
      payload: { name, input },
      created_at: '2026-06-16T01:00:00.000Z'
    }
  }
}

const AT = Date.parse('2026-06-16T01:00:00.000Z')

describe('normalizePath', () => {
  it('strips the /workspace/ prefix so the first segment is the repo', () => {
    expect(normalizePath('/workspace/seraphim/api/src/main.rs')).toBe('seraphim/api/src/main.rs')
  })

  it('keeps an already-relative path as-is', () => {
    expect(normalizePath('seraphim/frontend/src/app.css')).toBe('seraphim/frontend/src/app.css')
  })

  it('drops an absolute path outside the workspace', () => {
    expect(normalizePath('/etc/passwd')).toBeNull()
    expect(normalizePath('/workspace')).toBeNull()
  })

  it('drops paths that escape via ..', () => {
    expect(normalizePath('/workspace/../secret')).toBeNull()
    expect(normalizePath('a/../../b')).toBeNull()
  })

  it('drops empty/blank input', () => {
    expect(normalizePath('')).toBeNull()
    expect(normalizePath('   ')).toBeNull()
    expect(normalizePath(null)).toBeNull()
    expect(normalizePath(undefined)).toBeNull()
  })
})

describe('mapActivityEvent', () => {
  it('maps Write to create off file_path', () => {
    const result = mapActivityEvent(toolUse('Write', { file_path: '/workspace/repo/a.ts' }), 'My task')
    expect(result).toEqual({
      at: AT,
      actor: 'task-1',
      action: 'create',
      path: 'repo/a.ts',
      label: 'My task'
    })
  })

  it('maps Edit and NotebookEdit to modify', () => {
    expect(mapActivityEvent(toolUse('Edit', { file_path: '/workspace/repo/a.ts' }))?.action).toBe(
      'modify'
    )
    expect(
      mapActivityEvent(toolUse('NotebookEdit', { notebook_path: '/workspace/repo/n.ipynb' }))?.action
    ).toBe('modify')
  })

  it('maps Read to scan off file_path, and Grep/Glob to scan off path', () => {
    expect(mapActivityEvent(toolUse('Read', { file_path: '/workspace/repo/a.ts' }))?.action).toBe(
      'scan'
    )
    const grep = mapActivityEvent(toolUse('Grep', { pattern: 'foo', path: '/workspace/repo/src' }))
    expect(grep).toMatchObject({ action: 'scan', path: 'repo/src' })
  })

  it('maps Bash to a pathless pulse labeled with the command', () => {
    const result = mapActivityEvent(toolUse('Bash', { command: 'cargo test' }), 'My task')
    expect(result).toEqual({ at: AT, actor: 'task-1', action: 'pulse', label: 'cargo test' })
    expect(result).not.toHaveProperty('path')
  })

  it('drops non-tool_use events', () => {
    const assistant: ActivityEvent = {
      task_id: 'task-1',
      event: { type: 'assistant_text', payload: { text: 'hi' }, created_at: '2026-06-16T01:00:00Z' }
    }
    expect(mapActivityEvent(assistant)).toBeNull()
  })

  it('drops unknown tools and tools with no usable path', () => {
    expect(mapActivityEvent(toolUse('TodoWrite', { todos: [] }))).toBeNull()
    expect(mapActivityEvent(toolUse('Write', {}))).toBeNull()
    expect(mapActivityEvent(toolUse('Read', { file_path: '/etc/hosts' }))).toBeNull()
  })

  it('drops malformed frames without throwing', () => {
    expect(mapActivityEvent(null)).toBeNull()
    expect(mapActivityEvent({})).toBeNull()
    expect(mapActivityEvent({ task_id: 'task-1', event: { type: 'tool_use' } })).toBeNull()
    // Missing actor (task_id) is unusable for a stable color.
    expect(mapActivityEvent(toolUse('Write', { file_path: '/workspace/repo/a.ts' }, ''))).toBeNull()
  })

  it('falls back to a numeric timestamp when created_at is missing/invalid', () => {
    const frame: ActivityEvent = {
      task_id: 'task-1',
      event: { type: 'tool_use', payload: { name: 'Write', input: { file_path: 'repo/a.ts' } } }
    }
    const result = mapActivityEvent(frame)
    expect(result?.path).toBe('repo/a.ts')
    expect(typeof result?.at).toBe('number')
    expect(Number.isNaN(result?.at)).toBe(false)
  })
})
