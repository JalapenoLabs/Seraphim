<script lang="ts">
  import type { DndEvent } from 'svelte-dnd-action'
  import type { Settings, Task, TaskColumn } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { dndzone } from 'svelte-dnd-action'
  import { RefreshCw, Pause, Play } from '@lucide/svelte'

  import { COLUMNS } from '$lib/types'
  import { getBoard, listRepos, moveTask, provisionWorkspace, setPaused, syncNow } from '$lib/api'
  import { isWithinSchedule } from '$lib/schedule'
  import Card from '$lib/components/Card.svelte'
  import { Button } from '$lib/components/ui/button'
  import * as Alert from '$lib/components/ui/alert'

  const FLIP_MS = 150

  let settings = $state<Settings | null>(null)
  // One array per lane; svelte-dnd-action mutates these during a drag.
  let columns = $state<Record<TaskColumn, Task[]>>({
    available: [],
    todo: [],
    in_progress: [],
    in_review: [],
    done: [],
    ignored: []
  })

  let eventSource: EventSource | null = null
  // Maps a task's repo_id to its full name, so each card can show its source repo.
  let repoNames = $state<Record<string, string>>({})

  async function load() {
    const [board, repos] = await Promise.all([getBoard(), listRepos()])
    settings = board.settings
    repoNames = Object.fromEntries(repos.map((repo) => [repo.id, repo.full_name]))
    const grouped: Record<TaskColumn, Task[]> = {
      available: [],
      todo: [],
      in_progress: [],
      in_review: [],
      done: [],
      ignored: []
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

  // True when the agent is enabled but the schedule currently holds it idle, so
  // the board can explain why nothing is being picked up. Recomputed on every
  // board reload (the SSE stream keeps that frequent enough).
  const outsideSchedule = $derived(
    !!settings &&
      !settings.agent_paused &&
      settings.availability_enabled &&
      !isWithinSchedule(settings, new Date())
  )

  let checking = $state(false)
  async function checkIssues() {
    checking = true
    try {
      await syncNow()
      await load()
    } finally {
      checking = false
    }
  }

  let retrying = $state(false)
  async function retryProvision() {
    retrying = true
    try {
      await provisionWorkspace()
    } catch {
      // The error is reflected back via settings.config_repo_error on reload.
    } finally {
      await load()
      retrying = false
    }
  }

  onMount(() => {
    load()
    // Live board: the API ticks this stream whenever anything changes.
    eventSource = new EventSource('/api/v1/board/stream')
    eventSource.addEventListener('board', () => load())
  })

  onDestroy(() => eventSource?.close())
</script>

{#if settings?.config_repo_error}
  <Alert.Root variant="destructive" class="mx-6 mt-4 flex items-center justify-between gap-4">
    <div>
      <Alert.Title>Config repo (~/.claude) failed to set up — the agent is halted.</Alert.Title>
      <Alert.Description class="font-mono text-xs break-words">
        {settings.config_repo_error}
      </Alert.Description>
    </div>
    <Button variant="outline" size="sm" disabled={retrying} onclick={retryProvision}>
      {retrying ? 'Retrying…' : 'Retry'}
    </Button>
  </Alert.Root>
{/if}

<div class="flex items-center justify-between px-6 pb-1 pt-4">
  <div class="flex items-baseline gap-2">
    {#if settings}
      <strong class="text-base">{settings.org_name}</strong>
      <span class="text-sm text-muted-foreground">· {settings.claude_model}</span>
      {#if outsideSchedule}
        <span
          class="rounded-full border border-warning/40 px-2 py-0.5 text-xs text-warning"
          title="Outside the availability schedule"
        >
          ⏰ Outside scheduled hours
        </span>
      {/if}
    {/if}
  </div>
  <div class="flex gap-2">
    <Button variant="outline" size="sm" disabled={checking} onclick={checkIssues}>
      <RefreshCw class="size-4 {checking ? 'animate-spin' : ''}" />
      {checking ? 'Checking…' : 'Check issues'}
    </Button>
    {#if settings}
      <Button
        variant={settings.agent_paused ? 'default' : 'outline'}
        size="sm"
        onclick={togglePause}
      >
        {#if settings.agent_paused}
          <Play class="size-4" /> Resume agent
        {:else}
          <Pause class="size-4" /> Pause agent
        {/if}
      </Button>
    {/if}
  </div>
</div>

<div class="grid grid-cols-1 items-start gap-3 p-4 lg:h-full lg:grid-cols-6 lg:px-6 lg:pb-6">
  {#each COLUMNS as column}
    <section class="flex max-h-full min-h-0 flex-col rounded-lg border border-border bg-card">
      <header
        class="flex items-center justify-between border-b border-border px-3 py-2.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground"
      >
        <span>{column.label}</span>
        <span>{columns[column.key].length}</span>
      </header>
      <div
        class="flex min-h-[120px] flex-1 flex-col gap-2 overflow-y-auto p-3"
        use:dndzone={{ items: columns[column.key], flipDurationMs: FLIP_MS }}
        onconsider={(event) => handleConsider(column.key, event)}
        onfinalize={(event) => handleFinalize(column.key, event)}
      >
        {#each columns[column.key] as task (task.id)}
          <div>
            <Card {task} onchange={load} repoName={task.repo_id ? repoNames[task.repo_id] : undefined} />
          </div>
        {/each}
      </div>
    </section>
  {/each}
</div>
