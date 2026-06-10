<script lang="ts">
  import type { AgentEvent, Task } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { page } from '$app/stores'

  import { getTask } from '$lib/api'

  const taskId = $page.params.id ?? ''

  let task = $state<Task | null>(null)
  let events = $state<Pick<AgentEvent, 'type' | 'payload'>[]>([])
  let eventSource: EventSource | null = null

  // Which events are expanded. Tool use/results start collapsed; multiple can be
  // open at once.
  let expanded = $state<Record<number, boolean>>({})

  function isCollapsible(type: string) {
    return type === 'tool_use' || type === 'tool_result'
  }

  function toggle(index: number) {
    expanded[index] = !expanded[index]
  }

  async function load() {
    const detail = await getTask(taskId)
    task = detail.task
    events = detail.events.map((event) => ({ type: event.type, payload: event.payload }))
  }

  // Render an event's payload into a readable line based on its type.
  function describe(event: { type: string; payload: unknown }): string {
    const payload = event.payload as Record<string, unknown>
    if (event.type === 'assistant_text') {
      return String(payload?.text ?? '')
    }
    if (event.type === 'tool_use') {
      return `${payload?.name ?? 'tool'} ${JSON.stringify(payload?.input ?? {})}`
    }
    if (event.type === 'tool_result') {
      const content = payload?.content
      return typeof content === 'string' ? content : JSON.stringify(content ?? '')
    }
    if (event.type === 'system') {
      return `session started (${payload?.model ?? 'model'})`
    }
    if (event.type === 'result') {
      const cost = payload?.total_cost_usd
      return `turn complete${cost ? ` · $${cost}` : ''}`
    }
    return JSON.stringify(event.payload)
  }

  onMount(() => {
    load()
    eventSource = new EventSource(`/api/v1/tasks/${taskId}/stream`)
    eventSource.addEventListener('task', (message) => {
      const envelope = JSON.parse(message.data) as { type: string; payload: unknown }
      events = [...events, envelope]
      // Keep the card header roughly current as the turn progresses.
      load()
    })
  })

  onDestroy(() => eventSource?.close())
</script>

<div class="detail">
  <a class="back" href="/">← Board</a>

  {#if task}
    <header class="head">
      <div>
        <h1>#{task.external_id} {task.title}</h1>
        <div class="meta">
          <span class="badge {task.status}">{task.status.replace('_', ' ')}</span>
          {#if task.branch}<span class="mono">{task.branch}</span>{/if}
          {#if task.pr_url}<a href={task.pr_url} target="_blank" rel="noreferrer">pull request ↗</a>{/if}
          {#if task.url}<a href={task.url} target="_blank" rel="noreferrer">issue ↗</a>{/if}
        </div>
        {#if task.error}<div class="error">{task.error}</div>{/if}
      </div>
    </header>

    {#if task.body_snapshot}
      <section class="body">{task.body_snapshot}</section>
    {/if}

    <section class="stream">
      <h2>Activity</h2>
      {#if events.length === 0}
        <p class="muted">No activity yet.</p>
      {/if}
      {#each events as event, index (index)}
        {@const collapsible = isCollapsible(event.type)}
        {@const open = expanded[index] ?? false}
        <div class="event {event.type}">
          <button
            type="button"
            class="event-head"
            class:clickable={collapsible}
            onclick={() => collapsible && toggle(index)}
          >
            {#if collapsible}<span class="arrow">{open ? '▾' : '▸'}</span>{/if}
            <span class="kind">{event.type.replace('_', ' ')}</span>
          </button>
          <pre
            class="event-body"
            class:clamp3={collapsible && event.type === 'tool_result' && !open}
            class:clamp1={collapsible && event.type === 'tool_use' && !open}
          >{describe(event)}</pre>
        </div>
      {/each}
    </section>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .detail {
    max-width: 960px;
    margin: 0 auto;
    padding: 1.2rem 1.4rem 3rem;
  }

  .back {
    color: var(--muted);
  }

  .head h1 {
    font-size: 1.3rem;
    margin: 0.6rem 0 0.4rem;
  }

  .meta {
    display: flex;
    gap: 0.8rem;
    align-items: center;
    flex-wrap: wrap;
    font-size: 0.85rem;
  }

  .mono {
    font-family: ui-monospace, monospace;
    color: var(--muted);
  }

  .error {
    margin-top: 0.6rem;
    color: var(--danger);
  }

  .body {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.9rem;
    margin: 1rem 0;
    white-space: pre-wrap;
    color: var(--muted);
  }

  .stream h2 {
    font-size: 1rem;
    color: var(--muted);
  }

  .event {
    border-left: 2px solid var(--border);
    padding: 0.2rem 0 0.2rem 0.8rem;
    margin: 0.5rem 0;
  }

  .event.tool_use {
    border-color: var(--accent);
  }

  .event.result {
    border-color: var(--accent-2);
  }

  .event-head {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    background: none;
    border: none;
    padding: 0;
    text-align: left;
  }

  .event-head.clickable {
    cursor: pointer;
  }

  .arrow {
    width: 0.8rem;
    font-size: 0.7rem;
    color: var(--muted);
    flex: none;
  }

  .kind {
    font-size: 0.72rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  pre {
    margin: 0.2rem 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: ui-monospace, monospace;
    font-size: 0.82rem;
  }

  /* Collapsed tool results: first 3 lines; tool uses: a single ellipsized line. */
  .event-body.clamp3 {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .event-body.clamp1 {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
