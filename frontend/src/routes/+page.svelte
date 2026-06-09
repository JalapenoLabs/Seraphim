<script lang="ts">
  import type { DndEvent } from 'svelte-dnd-action'
  import type { Settings, Task, TaskColumn } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { dndzone } from 'svelte-dnd-action'

  import { COLUMNS } from '$lib/types'
  import { getBoard, moveTask, setPaused } from '$lib/api'
  import Card from '$lib/components/Card.svelte'

  const FLIP_MS = 150

  let settings = $state<Settings | null>(null)
  // One array per lane; svelte-dnd-action mutates these during a drag.
  let columns = $state<Record<TaskColumn, Task[]>>({
    available: [],
    todo: [],
    in_progress: [],
    in_review: [],
    done: []
  })

  let eventSource: EventSource | null = null

  async function load() {
    const board = await getBoard()
    settings = board.settings
    const grouped: Record<TaskColumn, Task[]> = {
      available: [],
      todo: [],
      in_progress: [],
      in_review: [],
      done: []
    }
    for (const task of board.tasks) {
      grouped[task.board_column].push(task)
    }
    for (const key of Object.keys(grouped) as TaskColumn[]) {
      grouped[key].sort((left, right) => left.position - right.position)
    }
    columns = grouped
  }

  function handleConsider(column: TaskColumn, event: CustomEvent<DndEvent<Task>>) {
    columns[column] = event.detail.items
  }

  async function handleFinalize(column: TaskColumn, event: CustomEvent<DndEvent<Task>>) {
    const items = event.detail.items
    columns[column] = items

    const movedId = event.detail.info.id
    const index = items.findIndex((task) => task.id === movedId)
    // Finalize fires on both zones; only the destination contains the card.
    if (index === -1) {
      return
    }

    await moveTask(movedId, column, computePosition(items, index))
    await load()
  }

  // Fractional rank: drop between neighbors by taking their midpoint.
  function computePosition(items: Task[], index: number): number {
    const previous = items[index - 1]?.position
    const next = items[index + 1]?.position
    if (previous !== undefined && next !== undefined) {
      return (previous + next) / 2
    }
    if (next !== undefined) {
      return next - 1
    }
    if (previous !== undefined) {
      return previous + 1
    }
    return 1
  }

  async function togglePause() {
    if (!settings) {
      return
    }
    settings = await setPaused(!settings.agent_paused)
  }

  onMount(() => {
    load()
    // Live board: the API ticks this stream whenever anything changes.
    eventSource = new EventSource('/api/v1/board/stream')
    eventSource.addEventListener('board', () => load())
  })

  onDestroy(() => eventSource?.close())
</script>

<div class="board-header">
  <div class="org">
    {#if settings}
      <strong>{settings.org_name}</strong>
      <span class="muted">· {settings.claude_model}</span>
    {/if}
  </div>
  {#if settings}
    <button class:primary={settings.agent_paused} onclick={togglePause}>
      {settings.agent_paused ? '▶ Resume agent' : '⏸ Pause agent'}
    </button>
  {/if}
</div>

<div class="board">
  {#each COLUMNS as column}
    <section class="lane">
      <header>
        <span>{column.label}</span>
        <span class="count">{columns[column.key].length}</span>
      </header>
      <div
        class="cards"
        use:dndzone={{ items: columns[column.key], flipDurationMs: FLIP_MS }}
        onconsider={(event) => handleConsider(column.key, event)}
        onfinalize={(event) => handleFinalize(column.key, event)}
      >
        {#each columns[column.key] as task (task.id)}
          <div>
            <Card {task} onchange={load} />
          </div>
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  .board-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem 1.4rem 0.4rem;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .board {
    display: grid;
    grid-template-columns: repeat(5, minmax(220px, 1fr));
    gap: 0.9rem;
    padding: 0.6rem 1.4rem 1.4rem;
    height: 100%;
    align-items: start;
  }

  .lane {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    max-height: 100%;
  }

  .lane > header {
    display: flex;
    justify-content: space-between;
    padding: 0.7rem 0.8rem;
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    font-size: 0.85rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .count {
    color: var(--muted);
  }

  .cards {
    padding: 0.7rem;
    overflow-y: auto;
    min-height: 120px;
    flex: 1;
  }

  @media (max-width: 1100px) {
    .board {
      grid-template-columns: 1fr;
      height: auto;
    }
  }
</style>
