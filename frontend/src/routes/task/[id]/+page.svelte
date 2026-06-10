<script lang="ts">
  import type { AgentEvent, Task } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { page } from '$app/stores'

  import { getTask } from '$lib/api'
  import { STATUS_BADGE, STATUS_LABELS } from '$lib/types'
  import { PaneGroup, type PaneGroupAPI } from 'paneforge'

  import { Badge } from '$lib/components/ui/badge'
  import * as Alert from '$lib/components/ui/alert'
  import * as Resizable from '$lib/components/ui/resizable'

  const taskId = $page.params.id ?? ''

  type StreamEvent = Pick<AgentEvent, 'type' | 'payload'>

  let task = $state<Task | null>(null)
  let events = $state<StreamEvent[]>([])
  let eventSource: EventSource | null = null

  // Tool use/results/thinking start collapsed; any number can be open at once.
  let expanded = $state<Record<number, boolean>>({})

  // The resizable split's imperative handle, so double-clicking the divider can
  // snap the panes back to an even 50/50.
  let paneGroup = $state<PaneGroupAPI>()

  function resetSplit() {
    paneGroup?.setLayout([50, 50])
  }

  // Leading glyph per event type, modeled on Claude Code's transcript: a filled
  // dot for the agent's own actions, a corner connector for their output.
  const MARKERS = {
    assistant_text: '●',
    tool_use: '●',
    result: '●',
    thinking: '✻',
    tool_result: '⎿',
    system: '⎿'
  } as const satisfies Record<string, string>

  const MARKER_COLORS = {
    assistant_text: 'text-foreground',
    tool_use: 'text-primary',
    result: 'text-success',
    thinking: 'text-warning',
    tool_result: 'text-muted-foreground',
    system: 'text-muted-foreground'
  } as const satisfies Record<string, string>

  function marker(type: string): string {
    return MARKERS[type as keyof typeof MARKERS] ?? '●'
  }

  function markerColor(type: string): string {
    return MARKER_COLORS[type as keyof typeof MARKER_COLORS] ?? 'text-foreground'
  }

  function isCollapsible(type: string): boolean {
    return type === 'tool_use' || type === 'tool_result' || type === 'thinking'
  }

  // The classes for an event's text line: its color, plus how it clamps when
  // collapsed (tool calls to one line, output/thinking to a few).
  function lineClasses(type: string, open: boolean): string {
    let color = 'text-foreground'
    if (type === 'tool_result' || type === 'system') {
      color = 'text-muted-foreground'
    } else if (type === 'thinking') {
      color = 'italic text-warning'
    }
    if (!open && type === 'tool_use') {
      return `min-w-0 flex-1 truncate ${color}`
    }
    if (!open && (type === 'tool_result' || type === 'thinking')) {
      return `min-w-0 flex-1 whitespace-pre-wrap break-words line-clamp-4 ${color}`
    }
    return `min-w-0 flex-1 whitespace-pre-wrap break-words ${color}`
  }

  function toggle(index: number) {
    expanded[index] = !expanded[index]
  }

  async function load() {
    const detail = await getTask(taskId)
    task = detail.task
    events = detail.events.map((event) => ({ type: event.type, payload: event.payload }))
  }

  // The most telling argument of a tool call, so `Bash(cargo build)` reads at a
  // glance instead of a wall of JSON. Falls back to the whole input object.
  function toolSummary(payload: Record<string, unknown>): string {
    const name = String(payload?.name ?? 'tool')
    const input = (payload?.input ?? {}) as Record<string, unknown>
    const headline =
      input.command ?? input.file_path ?? input.path ?? input.pattern ?? input.url ?? input.description
    const argument = headline === undefined ? JSON.stringify(input) : String(headline)
    return `${name}(${argument})`
  }

  function describe(event: StreamEvent): string {
    const payload = event.payload as Record<string, unknown>
    if (event.type === 'thinking') {
      return String(payload?.thinking ?? '')
    }
    if (event.type === 'assistant_text') {
      return String(payload?.text ?? '')
    }
    if (event.type === 'tool_use') {
      return toolSummary(payload)
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
      const envelope = JSON.parse(message.data) as StreamEvent
      events = [...events, envelope]
      load()
    })
  })

  onDestroy(() => eventSource?.close())
</script>

<div class="flex h-full flex-col gap-3 p-4">
  <a href="/" class="text-sm text-muted-foreground hover:text-foreground">← Board</a>

  {#if task}
    <PaneGroup
      bind:this={paneGroup}
      direction="horizontal"
      autoSaveId="seraphim-task-split-v2"
      class="flex min-h-0 w-full flex-1 overflow-hidden"
    >
      <Resizable.Pane defaultSize={50} minSize={25} class="min-w-0">
        <div class="h-full min-w-0 overflow-y-auto pr-3">
          <div class="text-sm tabular-nums text-muted-foreground">#{task.external_id}</div>
          <h1 class="mb-3 mt-1 text-xl font-semibold leading-snug">{task.title}</h1>

          <Badge variant="outline" class={STATUS_BADGE[task.status]}>
            {STATUS_LABELS[task.status] ?? task.status}
          </Badge>

          <dl class="mt-4 space-y-3 text-sm">
            {#if task.branch}
              <div>
                <dt class="text-xs uppercase tracking-wide text-muted-foreground">Branch</dt>
                <dd class="break-words font-mono">{task.branch}</dd>
              </div>
            {/if}
            {#if task.pr_url}
              <div>
                <dt class="text-xs uppercase tracking-wide text-muted-foreground">Pull request</dt>
                <dd class="break-words">
                  <a href={task.pr_url} target="_blank" rel="noreferrer" class="text-primary hover:underline">
                    {task.pr_url.replace('https://github.com/', '')} ↗
                  </a>
                </dd>
              </div>
            {/if}
            {#if task.url}
              <div>
                <dt class="text-xs uppercase tracking-wide text-muted-foreground">Issue</dt>
                <dd>
                  <a href={task.url} target="_blank" rel="noreferrer" class="text-primary hover:underline">
                    open on GitHub ↗
                  </a>
                </dd>
              </div>
            {/if}
          </dl>

          {#if task.error}
            <Alert.Root variant="destructive" class="mt-4">
              <Alert.Description class="break-words">{task.error}</Alert.Description>
            </Alert.Root>
          {/if}

          {#if task.body_snapshot}
            <h2 class="mb-2 mt-6 text-xs uppercase tracking-wide text-muted-foreground">Issue</h2>
            <div
              class="whitespace-pre-wrap rounded-lg border border-border bg-card p-3 text-sm leading-relaxed text-muted-foreground"
            >
              {task.body_snapshot}
            </div>
          {/if}
        </div>
      </Resizable.Pane>

      <Resizable.Handle
        withHandle
        ondblclick={resetSplit}
        title="Drag to resize · double-click to reset to 50/50"
        class="w-1.5 bg-border transition-colors hover:bg-primary data-[active]:bg-primary"
      />

      <Resizable.Pane defaultSize={50} minSize={30} class="min-w-0">
        <div class="ml-3 flex h-full min-w-0 flex-col rounded-lg border border-border bg-card">
          <header
            class="border-b border-border px-4 py-2.5 text-xs uppercase tracking-wide text-muted-foreground"
          >
            Activity
          </header>
          <div class="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden p-3 font-mono text-sm leading-relaxed">
            {#if events.length === 0}
              <p class="text-muted-foreground">No activity yet.</p>
            {/if}
            {#each events as event, index (index)}
              {@const collapsible = isCollapsible(event.type)}
              {@const open = expanded[index] ?? false}
              {@const indent = event.type === 'tool_result' || event.type === 'system' ? 'pl-4' : ''}
              {#if collapsible}
                <button
                  type="button"
                  onclick={() => toggle(index)}
                  title={open ? 'Click to collapse' : 'Click to expand'}
                  class="flex w-full gap-2 py-0.5 text-left hover:opacity-80 {indent}"
                >
                  <span class="w-[1ch] flex-none {markerColor(event.type)}">{marker(event.type)}</span>
                  <span class={lineClasses(event.type, open)}>{describe(event)}</span>
                </button>
              {:else}
                <div class="flex gap-2 py-0.5 {indent}">
                  <span class="w-[1ch] flex-none {markerColor(event.type)}">{marker(event.type)}</span>
                  <span class={lineClasses(event.type, open)}>{describe(event)}</span>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      </Resizable.Pane>
    </PaneGroup>
  {:else}
    <p class="text-muted-foreground">Loading…</p>
  {/if}
</div>
