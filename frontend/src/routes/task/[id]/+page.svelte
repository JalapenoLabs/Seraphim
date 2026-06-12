<script lang="ts">
  import type { AgentEvent, AnswerSubmission, EnvSuggestion, Question, Task } from '$lib/types'

  import { onMount, onDestroy, tick } from 'svelte'
  import { toast } from 'svelte-sonner'
  import { page } from '$app/stores'
  import { goto } from '$app/navigation'
  import { Ban, ChevronDown, NotebookPen, Pause, Play, RotateCcw } from '@lucide/svelte'

  import {
    acknowledgeSuggestion,
    answerQuestion,
    getTask,
    hardResetTask,
    setTaskBlocking,
    setTaskHold,
    setTaskNotes
  } from '$lib/api'
  import { STATUS_BADGE, STATUS_LABELS } from '$lib/types'
  import { PaneGroup, type PaneGroupAPI } from 'paneforge'

  import { Badge } from '$lib/components/ui/badge'
  import { Switch } from '$lib/components/ui/switch'
  import { Textarea } from '$lib/components/ui/textarea'
  import * as Alert from '$lib/components/ui/alert'
  import * as AlertDialog from '$lib/components/ui/alert-dialog'
  import * as Resizable from '$lib/components/ui/resizable'
  import { buttonVariants } from '$lib/components/ui/button'
  import IssueView from '$lib/components/IssueView.svelte'
  import SuggestionCreateButton from '$lib/components/SuggestionCreateButton.svelte'
  import Stats from '$lib/components/Stats.svelte'
  import Markdown from '$lib/components/Markdown.svelte'
  import JsonHighlight from '$lib/components/JsonHighlight.svelte'
  import DiffView from '$lib/components/DiffView.svelte'
  import { editDiff } from '$lib/diff'

  const taskId = $page.params.id ?? ''

  type StreamEvent = Pick<AgentEvent, 'type' | 'payload' | 'created_at'>

  let task = $state<Task | null>(null)
  let events = $state<StreamEvent[]>([])
  let suggestions = $state<EnvSuggestion[]>([])
  let questions = $state<Question[]>([])
  let eventSource: EventSource | null = null

  // A live clock driving the "Running …" timer below the latest event; ticks
  // once a second while a turn is in flight.
  let now = $state(Date.now())
  let timer: ReturnType<typeof setInterval> | null = null

  const lastEvent = $derived(events.at(-1))
  // Show the live timer only while the agent is mid-turn, i.e. its latest event
  // isn't the turn's terminal `result`.
  const running = $derived(task?.status === 'working' && !!lastEvent && lastEvent.type !== 'result')

  async function toggleSuggestion(suggestion: EnvSuggestion) {
    // Optimistically flip so the checkbox feels instant, then persist; revert on
    // failure so the UI never lies about what was actually saved.
    const next = !suggestion.acknowledged
    suggestion.acknowledged = next
    try {
      await acknowledgeSuggestion(suggestion.id, next)
    } catch (error) {
      console.debug('failed to update suggestion, reverting', error)
      suggestion.acknowledged = !next
    }
  }

  // After a recommendation is turned into an issue, the server marks it done;
  // reflect that so it checks off and the create button drops away.
  function onSuggestionCreated(updated: EnvSuggestion) {
    const index = suggestions.findIndex((entry) => entry.id === updated.id)
    if (index !== -1) {
      suggestions[index] = updated
    }
  }

  // Persist every answer from the wizard's review step, then reload once. The
  // agent only resumes when no pending question remains, so submitting them
  // together (in order) is safe.
  async function submitAnswers(answers: AnswerSubmission[]) {
    for (const answer of answers) {
      await answerQuestion(answer.questionId, answer.kind, answer.text)
    }
    await load()
  }

  // Private per-task notepad. Initialized once from the loaded task (not on every
  // SSE-driven reload, which would clobber in-progress edits) and auto-saved.
  let notes = $state('')
  let notesInitialized = false
  let notesOpen = $state(false)
  let notesStatus = $state<'idle' | 'saving' | 'saved'>('idle')
  let notesTimer: ReturnType<typeof setTimeout> | null = null

  function scheduleNotesSave() {
    notesStatus = 'saving'
    if (notesTimer) {
      clearTimeout(notesTimer)
    }
    notesTimer = setTimeout(saveNotes, 700)
  }

  async function saveNotes() {
    if (notesTimer) {
      clearTimeout(notesTimer)
      notesTimer = null
    }
    try {
      await setTaskNotes(taskId, notes)
      notesStatus = 'saved'
    } catch (error) {
      console.debug('failed to save notes', error)
      notesStatus = 'idle'
    }
  }

  // Tool use/results/thinking start collapsed; any number can be open at once.
  let expanded = $state<Record<number, boolean>>({})

  // The resizable split's imperative handle, so double-clicking the divider can
  // snap the panes back to an even 50/50.
  let paneGroup = $state<PaneGroupAPI>()

  function resetSplit() {
    paneGroup?.setLayout([50, 50])
  }

  // Activity log autoscroll: follow new events only while the user is parked at
  // the bottom. Scrolling up pauses it; returning to the bottom re-engages it.
  const STICK_THRESHOLD_PX = 48
  let logEl = $state<HTMLDivElement>()
  let stickToBottom = $state(true)

  function onLogScroll() {
    if (!logEl) {
      return
    }
    const distanceFromBottom = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight
    stickToBottom = distanceFromBottom < STICK_THRESHOLD_PX
  }

  $effect(() => {
    // Re-run whenever an event arrives; scroll only if we're still following.
    events.length
    if (stickToBottom && logEl) {
      tick().then(() => {
        if (logEl) {
          logEl.scrollTop = logEl.scrollHeight
        }
      })
    }
  })

  // Leading glyph per event type, modeled on Claude Code's transcript: a filled
  // dot for the agent's own actions, a corner connector for their output.
  const MARKERS = {
    prompt: '●',
    assistant_text: '●',
    tool_use: '●',
    result: '●',
    thinking: '✻',
    tool_result: '⎿',
    system: '⎿',
    rate_limit: '◆'
  } as const satisfies Record<string, string>

  const MARKER_COLORS = {
    prompt: 'text-prompt',
    assistant_text: 'text-foreground',
    tool_use: 'text-primary',
    result: 'text-success',
    thinking: 'text-warning',
    tool_result: 'text-muted-foreground',
    system: 'text-muted-foreground',
    rate_limit: 'text-info'
  } as const satisfies Record<string, string>

  function marker(type: string): string {
    return MARKERS[type as keyof typeof MARKERS] ?? '●'
  }

  function markerColor(type: string): string {
    return MARKER_COLORS[type as keyof typeof MARKER_COLORS] ?? 'text-foreground'
  }

  // Some tool results are noise once the tool-use line above is shown: Read
  // dumps the whole file, and Write/Edit just echo "file updated" while the diff
  // we render says far more. Collect the ids of those calls so we can hide their
  // (successful) result bodies. Errors always stay visible.
  const QUIET_RESULT_TOOLS = new Set(['Read', 'Write', 'Edit', 'MultiEdit'])

  const quietResultToolIds = $derived.by(() => {
    const ids = new Set<string>()
    for (const event of events) {
      if (event.type === 'tool_use') {
        const payload = event.payload as Record<string, unknown>
        if (QUIET_RESULT_TOOLS.has(String(payload?.name)) && typeof payload?.id === 'string') {
          ids.add(payload.id)
        }
      }
    }
    return ids
  })

  function isHiddenToolResult(event: StreamEvent): boolean {
    if (event.type !== 'tool_result') {
      return false
    }
    const payload = event.payload as Record<string, unknown>
    // Keep failures visible (e.g. "file not found", a rejected edit).
    if (payload?.is_error === true) {
      return false
    }
    const toolUseId = payload?.tool_use_id
    return typeof toolUseId === 'string' && quietResultToolIds.has(toolUseId)
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
    } else if (type === 'rate_limit') {
      color = 'font-medium text-info'
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
    events = detail.events.map((event) => ({
      type: event.type,
      payload: event.payload,
      created_at: event.created_at
    }))
    suggestions = detail.suggestions
    questions = detail.questions
    // Seed the notepad once, and open it if there is already something to read.
    if (!notesInitialized) {
      notes = detail.task.notes
      notesOpen = notes.trim().length > 0
      notesInitialized = true
    }
  }

  // Hold toggle, behind a confirmation so it's a deliberate action.
  async function confirmHold() {
    if (!task) {
      return
    }
    const held = !task.hold
    await setTaskHold(task.id, held)
    await load()
    toast.success(held ? 'Task held — the agent will skip it' : 'Hold released')
  }

  // Hard reset: a destructive, irreversible action, so behind a confirmation. On
  // success the card has moved to Available, so return to the board and report
  // exactly which side effects ran.
  let resetting = $state(false)
  async function confirmReset() {
    if (!task || resetting) {
      return
    }
    resetting = true
    try {
      const summary = await hardResetTask(task.id)
      const done = [
        summary.interrupted_agent && 'stopped the agent',
        summary.pr_closed && 'closed the PR',
        summary.branch_deleted && 'deleted the branch',
        summary.issue_reopened && 'reopened the issue'
      ].filter(Boolean)
      const detail = done.length ? ` (${done.join(', ')})` : ''
      toast.success(`Task reset to Available${detail}`)
      goto('/')
    } catch (error) {
      console.debug('hard reset failed', error)
      toast.error('Hard reset failed. See the server logs.')
      resetting = false
    }
  }

  // Blocking toggle: a quick, reversible flag, so no confirmation dialog.
  async function toggleBlocking() {
    if (!task) {
      return
    }
    const blocking = !task.blocking
    await setTaskBlocking(task.id, blocking)
    await load()
    toast.success(
      blocking
        ? 'Marked blocking — the agent starts nothing new while this is in progress'
        : 'No longer blocking'
    )
  }

  // Human labels for the rate-limit window and status codes Claude emits.
  const RATE_LIMIT_WINDOWS: Record<string, string> = {
    five_hour: '5-hour limit',
    weekly: 'weekly limit'
  }
  const RATE_LIMIT_STATUSES: Record<string, string> = {
    allowed: 'allowed',
    allowed_warning: 'approaching limit',
    rejected: 'limit reached'
  }

  function humanize(value: string): string {
    return value.replace(/_/g, ' ')
  }

  // A reset moment as "3:40 PM (in 2h 14m)", or just the clock time once it's past.
  function formatReset(unixSeconds: number): string {
    const date = new Date(unixSeconds * 1000)
    const time = date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
    const diffMinutes = Math.round((date.getTime() - Date.now()) / 60000)
    if (diffMinutes <= 0) {
      return time
    }
    const relative =
      diffMinutes < 60 ? `${diffMinutes}m` : `${Math.floor(diffMinutes / 60)}h ${diffMinutes % 60}m`
    return `${time} (in ${relative})`
  }

  // Turns a rate_limit_event payload into one clean line instead of raw JSON.
  function describeRateLimit(payload: Record<string, unknown>): string {
    const info = payload?.rate_limit_info as Record<string, unknown> | undefined
    if (!info) {
      return 'Rate limit update'
    }
    const window = RATE_LIMIT_WINDOWS[String(info.rateLimitType)] ?? humanize(String(info.rateLimitType ?? 'usage'))
    const status = RATE_LIMIT_STATUSES[String(info.status)] ?? humanize(String(info.status ?? ''))
    let line = `Rate limit · ${window}: ${status}`
    if (typeof info.resetsAt === 'number') {
      line += `, resets ${formatReset(info.resetsAt)}`
    }
    if (info.isUsingOverage) {
      const overage = RATE_LIMIT_STATUSES[String(info.overageStatus)] ?? humanize(String(info.overageStatus ?? ''))
      line += ` · overage ${overage}`
      if (typeof info.overageResetsAt === 'number') {
        line += `, resets ${formatReset(info.overageResetsAt)}`
      }
    }
    return line
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

  // "1h 2m 3s" / "2m 3s" / "3s" from a millisecond span.
  function formatDuration(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000))
    const hours = Math.floor(totalSeconds / 3600)
    const minutes = Math.floor((totalSeconds % 3600) / 60)
    const seconds = totalSeconds % 60
    if (hours > 0) {
      return `${hours}h ${minutes}m ${seconds}s`
    }
    if (minutes > 0) {
      return `${minutes}m ${seconds}s`
    }
    return `${seconds}s`
  }

  function describe(event: StreamEvent): string {
    const payload = event.payload as Record<string, unknown>
    if (event.type === 'prompt') {
      return String(payload?.text ?? '')
    }
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
      const durationMs = typeof payload?.duration_ms === 'number' ? payload.duration_ms : null
      const parts = ['turn complete']
      if (cost) parts.push(`$${cost}`)
      if (durationMs !== null) parts.push(formatDuration(durationMs))
      return parts.join(' · ')
    }
    if (event.type === 'rate_limit') {
      return describeRateLimit(payload)
    }
    return JSON.stringify(event.payload)
  }

  onMount(() => {
    load()
    timer = setInterval(() => (now = Date.now()), 1000)
    eventSource = new EventSource(`/api/v1/tasks/${taskId}/stream`)
    eventSource.addEventListener('task', (message) => {
      const envelope = JSON.parse(message.data) as StreamEvent
      events = [...events, { ...envelope, created_at: envelope.created_at ?? new Date().toISOString() }]
      load()
    })
  })

  onDestroy(() => {
    eventSource?.close()
    if (timer) {
      clearInterval(timer)
    }
    // Flush a pending notes edit so leaving the page doesn't drop it.
    if (notesTimer) {
      void saveNotes()
    }
  })
</script>

<div class="flex h-full flex-col gap-3 p-4">
  <a href="/" class="text-sm text-muted-foreground hover:text-foreground">← Board</a>

  {#if task}
    <Stats taskId={taskId} />

    {#if suggestions.length}
      <!-- Loud on the task too: the checkboxes here are what clear the board badge. -->
      <section class="rounded-lg border border-warning/50 bg-card p-3">
        <h2 class="text-sm font-semibold">💡 Environment recommendations</h2>
        <p class="mt-0.5 text-xs text-muted-foreground">
          Things the agent thinks would make future runs smoother. Check one off once you have
          handled it; unchecked ones stay loud on the board.
        </p>
        <ul class="mt-2 divide-y divide-border">
          {#each suggestions as suggestion (suggestion.id)}
            <li class="flex items-start justify-between gap-3 py-2">
              <button
                type="button"
                role="switch"
                aria-checked={suggestion.acknowledged}
                onclick={() => toggleSuggestion(suggestion)}
                class="flex min-w-0 flex-1 cursor-pointer items-start gap-2 text-left"
              >
                <Switch
                  checked={suggestion.acknowledged}
                  tabindex={-1}
                  aria-hidden="true"
                  class="mt-0.5 pointer-events-none"
                />
                <span class="flex min-w-0 flex-col gap-0.5">
                  <span
                    class="text-sm font-medium {suggestion.acknowledged
                      ? 'text-muted-foreground line-through'
                      : ''}"
                  >
                    {suggestion.title}
                  </span>
                  {#if suggestion.detail}
                    <span class="whitespace-pre-wrap text-xs text-muted-foreground"
                      >{suggestion.detail}</span
                    >
                  {/if}
                </span>
              </button>
              {#if !suggestion.acknowledged && task}
                <SuggestionCreateButton
                  {suggestion}
                  source={task.source_kind}
                  repoLinked={!!task.repo_id}
                  oncreated={onSuggestionCreated}
                />
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <!-- Private scratchpad: stored only here, never written to the source ticket. -->
    <section class="flex-none rounded-lg border border-border bg-card">
      <button
        type="button"
        onclick={() => (notesOpen = !notesOpen)}
        class="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-secondary/40"
      >
        <NotebookPen class="size-4 flex-none text-muted-foreground" />
        <span class="font-semibold">Private notes</span>
        {#if notes.trim()}
          <span class="size-1.5 flex-none rounded-full bg-primary" title="This task has notes"></span>
        {/if}
        <span class="ml-auto text-xs text-muted-foreground">
          {#if notesStatus === 'saving'}Saving…{:else if notesStatus === 'saved'}Saved{/if}
        </span>
        <ChevronDown
          class="size-4 flex-none text-muted-foreground transition-transform {notesOpen
            ? 'rotate-180'
            : ''}"
        />
      </button>
      {#if notesOpen}
        <div class="space-y-1.5 border-t border-border p-3">
          <Textarea
            rows={8}
            placeholder="Scratchpad for your own notes on this task…"
            bind:value={notes}
            oninput={scheduleNotesSave}
            onblur={saveNotes}
            class="resize-y text-sm"
          />
          <p class="text-xs text-muted-foreground">
            Only you can see these. They are stored privately and never sent to GitHub or Jira.
          </p>
        </div>
      {/if}
    </section>

    <PaneGroup
      bind:this={paneGroup}
      direction="horizontal"
      autoSaveId="seraphim-task-split-v2"
      class="flex min-h-0 w-full flex-1 overflow-hidden"
    >
      <Resizable.Pane defaultSize={55} minSize={30} class="min-w-0">
        <div class="h-full min-w-0 pr-3">
          <IssueView {task} {questions} onSubmit={submitAnswers} />
        </div>
      </Resizable.Pane>

      <Resizable.Handle
        withHandle
        ondblclick={resetSplit}
        title="Drag to resize · double-click to reset to 50/50"
        class="w-1.5 bg-border transition-colors hover:bg-primary data-[active]:bg-primary"
      />

      <Resizable.Pane defaultSize={45} minSize={25} class="min-w-0">
        <div class="ml-3 flex h-full min-w-0 flex-col rounded-lg border border-border bg-card">
          <header class="flex items-center gap-2 border-b border-border px-4 py-2.5">
            <span class="text-xs uppercase tracking-wide text-muted-foreground">Agent activity</span>
            <div class="ml-auto flex items-center gap-2">
              <Badge variant="outline" class={STATUS_BADGE[task.status]}>
                {STATUS_LABELS[task.status] ?? task.status}
              </Badge>
              <button
                type="button"
                onclick={toggleBlocking}
                title="While in progress, the agent starts no new work until this task finishes"
                class={buttonVariants({
                  variant: task.blocking ? 'default' : 'outline',
                  size: 'sm'
                })}
              >
                <Ban class="size-3.5" />
                {task.blocking ? 'Blocking' : 'Make blocking'}
              </button>
              <AlertDialog.Root>
                <AlertDialog.Trigger class={buttonVariants({ variant: 'outline', size: 'sm' })}>
                  {#if task.hold}
                    <Play class="size-3.5" /> Release
                  {:else}
                    <Pause class="size-3.5" /> Hold
                  {/if}
                </AlertDialog.Trigger>
                <AlertDialog.Content>
                  <AlertDialog.Header>
                    <AlertDialog.Title>
                      {task.hold ? 'Release this hold?' : 'Hold this task?'}
                    </AlertDialog.Title>
                    <AlertDialog.Description>
                      {#if task.hold}
                        The agent will be able to pick this card up again from its current position
                        in the queue.
                      {:else}
                        Holding parks this card in place. The agent will skip it when pulling work
                        (the To Do queue, CI fixes, and idle revisits) and move on to the next
                        eligible card. A task already in progress isn't interrupted, and you can
                        release the hold anytime.
                      {/if}
                    </AlertDialog.Description>
                  </AlertDialog.Header>
                  <AlertDialog.Footer>
                    <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
                    <AlertDialog.Action onclick={confirmHold}>
                      {task.hold ? 'Release hold' : 'Hold task'}
                    </AlertDialog.Action>
                  </AlertDialog.Footer>
                </AlertDialog.Content>
              </AlertDialog.Root>
              <AlertDialog.Root>
                <AlertDialog.Trigger
                  class={buttonVariants({ variant: 'destructive', size: 'sm' })}
                  disabled={resetting}
                  title="Abandon this attempt and start the task over from scratch"
                >
                  <RotateCcw class="size-3.5" />
                  Hard reset
                </AlertDialog.Trigger>
                <AlertDialog.Content>
                  <AlertDialog.Header>
                    <AlertDialog.Title>Hard reset this task?</AlertDialog.Title>
                    <AlertDialog.Description>
                      This abandons the current attempt and starts the task over. It will:
                      stop the agent if it is working this task right now, close its pull request,
                      delete its branch (from GitHub and the workspace), reopen the source issue if
                      it was closed, and move the card back to Available. This cannot be undone.
                    </AlertDialog.Description>
                  </AlertDialog.Header>
                  <AlertDialog.Footer>
                    <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
                    <AlertDialog.Action
                      class={buttonVariants({ variant: 'destructive' })}
                      onclick={confirmReset}
                    >
                      Hard reset
                    </AlertDialog.Action>
                  </AlertDialog.Footer>
                </AlertDialog.Content>
              </AlertDialog.Root>
            </div>
          </header>
          {#if task.error}
            <Alert.Root variant="destructive" class="m-3 mb-0">
              <Alert.Description class="break-words text-xs">{task.error}</Alert.Description>
            </Alert.Root>
          {/if}
          <div
            bind:this={logEl}
            onscroll={onLogScroll}
            class="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden p-3 pb-[25vh] font-mono text-sm leading-relaxed"
          >
            {#if events.length === 0}
              <p class="text-muted-foreground">No activity yet.</p>
            {/if}
            {#each events as event, index (index)}
              {#if !isHiddenToolResult(event)}
                {@const collapsible = isCollapsible(event.type)}
                {@const open = expanded[index] ?? false}
                {@const indent = event.type === 'tool_result' || event.type === 'system' ? 'pl-4' : ''}
                {@const diff =
                  event.type === 'tool_use'
                    ? editDiff(event.payload as Record<string, unknown>)
                    : null}
                {#if diff}
                  <!-- Write/Edit calls render as a red/green patch, not a bare summary line. -->
                  <div class="flex items-start gap-2 py-0.5">
                    <span class="w-[1ch] flex-none text-primary">●</span>
                    <div class="min-w-0 flex-1">
                      <div class="truncate text-primary">{diff.verb}({diff.path})</div>
                      <DiffView lines={diff.lines} added={diff.added} removed={diff.removed} />
                    </div>
                  </div>
                {:else if event.type === 'prompt'}
                  <!-- Our own brief to the agent: a violet dot + light wash, with
                       up to ~3 lines shown until clicked open, for transparency. -->
                  <button
                    type="button"
                    onclick={() => toggle(index)}
                    title={open ? 'Click to collapse' : 'Click to expand'}
                    class="my-0.5 flex w-full gap-2 rounded-md border border-prompt/30 bg-prompt/5 px-2 py-1.5 text-left transition-colors hover:bg-prompt/10"
                  >
                    <span class="w-[1ch] flex-none text-prompt">●</span>
                    <span class="min-w-0 flex-1">
                      <span class="mb-0.5 block text-[10px] font-semibold uppercase tracking-wide text-prompt">
                        Seraphim prompt
                      </span>
                      <span
                        class="block whitespace-pre-wrap break-words text-foreground/90 {open ? '' : 'line-clamp-3'}"
                      >{describe(event)}</span>
                    </span>
                  </button>
                {:else if collapsible}
                  <button
                    type="button"
                    onclick={() => toggle(index)}
                    title={open ? 'Click to collapse' : 'Click to expand'}
                    class="flex w-full gap-2 py-0.5 text-left hover:opacity-80 {indent}"
                  >
                    <span class="w-[1ch] flex-none {markerColor(event.type)}">{marker(event.type)}</span>
                    <span class={lineClasses(event.type, open)}><JsonHighlight text={describe(event)} /></span>
                  </button>
                {:else}
                  <div class="flex items-start gap-2 py-0.5 {indent}">
                    <span class="w-[1ch] flex-none {markerColor(event.type)}">{marker(event.type)}</span>
                    {#if event.type === 'assistant_text'}
                      <!-- Render the agent's prose as full markdown. -->
                      <div class="min-w-0 flex-1"><Markdown source={describe(event)} /></div>
                    {:else}
                      <span class={lineClasses(event.type, open)}><JsonHighlight text={describe(event)} /></span>
                    {/if}
                  </div>
                {/if}
              {/if}
            {/each}
            {#if running && lastEvent}
              <div class="flex items-start gap-2 py-0.5 text-muted-foreground">
                <span class="w-[1ch] flex-none"></span>
                <span>Running {formatDuration(now - new Date(lastEvent.created_at).getTime())}</span>
              </div>
            {/if}
          </div>
        </div>
      </Resizable.Pane>
    </PaneGroup>
  {:else}
    <p class="text-muted-foreground">Loading…</p>
  {/if}
</div>
