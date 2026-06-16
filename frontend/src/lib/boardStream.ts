// A single shared connection to the board SSE stream (`/api/v1/board/stream`).
//
// The board page, the global stats banner, and every per-railway stats strip all
// want the same throttled `usage` nudge (and, for some, the `board` change tick)
// to refetch live during a turn. Previously each component opened its own
// EventSource, so a board with many railways held many connections to the same
// endpoint. This module collapses that to ONE connection, ref-counted: the first
// subscriber opens it, the last to leave closes it, and every `usage` / `board`
// event fans out to all current listeners.
//
// Usage from a component:
//   import { onMount, onDestroy } from 'svelte'
//   import { subscribeBoardStream } from '$lib/boardStream'
//   let unsubscribe: (() => void) | null = null
//   onMount(() => { unsubscribe = subscribeBoardStream({ usage: refresh }) })
//   onDestroy(() => unsubscribe?.())

// Which board events a subscriber cares about. Both are optional, so a component
// can listen for just the `usage` tick (the stats strips) or both.
export type BoardStreamListener = {
  // The throttled mid-turn nudge that token usage advanced.
  usage?: () => void
  // Any board change (a card moved, a task updated, etc.).
  board?: () => void
}

// The single shared connection and its live listeners. Module-level so every
// caller shares one EventSource; `null` whenever no one is subscribed.
let source: EventSource | null = null
const listeners = new Set<BoardStreamListener>()

function handleUsage() {
  for (const listener of listeners) {
    listener.usage?.()
  }
}

function handleBoard() {
  for (const listener of listeners) {
    listener.board?.()
  }
}

// Opens the shared connection on the first subscriber.
function open() {
  if (source) {
    return
  }
  source = new EventSource('/api/v1/board/stream')
  source.addEventListener('usage', handleUsage)
  source.addEventListener('board', handleBoard)
}

// Closes the shared connection once the last subscriber leaves, so an idle page
// holds no connection.
function close() {
  if (!source) {
    return
  }
  source.removeEventListener('usage', handleUsage)
  source.removeEventListener('board', handleBoard)
  source.close()
  source = null
}

// Subscribes to the shared board stream, opening the single connection if needed.
// Returns an unsubscribe function that removes this listener and closes the
// connection when none remain.
export function subscribeBoardStream(listener: BoardStreamListener): () => void {
  listeners.add(listener)
  open()
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) {
      close()
    }
  }
}
