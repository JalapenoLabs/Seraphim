<script lang="ts">
  import type { AgentEvent, AnswerKind, Question, Task } from '$lib/types'

  import { onMount, onDestroy, tick } from 'svelte'
  import { toast } from 'svelte-sonner'
  import { page } from '$app/stores'
  import { Pause, Play } from '@lucide/svelte'

  import { answerQuestion, getTask, setTaskHold } from '$lib/api'
  import { STATUS_BADGE, STATUS_LABELS } from '$lib/types'
  import { PaneGroup, type PaneGroupAPI } from 'paneforge'

  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import * as Alert from '$lib/components/ui/alert'
  import * as AlertDialog from '$lib/components/ui/alert-dialog'
  import * as Resizable from '$lib/components/ui/resizable'
  import { buttonVariants } from '$lib/components/ui/button'
  import IssueView from '$lib/components/IssueView.svelte'
  import Markdown from '$lib/components/Markdown.svelte'

  const taskId = $page.params.id ?? ''

  type StreamEvent = Pick<AgentEvent, 'type' | 'payload'>

  let task = $state<Task | null>(null)
  let events = $state<StreamEvent[]>([])
  let questions = $state<Question[]>([])
  let eventSource: EventSource | null = null

  // Per-question free-text inputs for the "something else" and "decline" choices.
  let customText = $state<Record<string, string>>({})
  let declineText = $state<Record<string, string>>({})

  const pendingQuestions = $derived(questions.filter((question) => question.status === 'pending'))
  const answeredQuestions = $derived(questions.filter((question) => question.status !== 'pending'))

  async function submitAnswer(questionId: string, kind: AnswerKind, text: string) {
    await answerQuestion(questionId, kind, text)
    await load()
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
    questions = detail.questions
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
    {#if pendingQuestions.length}
      <div class="rounded-lg border border-warning/40 bg-card p-4">
        <h2 class="mb-3 text-sm font-semibold">The agent needs your input</h2>
        {#each pendingQuestions as question (question.id)}
          <div class="mb-3 rounded-md border border-border p-3 last:mb-0">
            <p class="mb-3 font-medium">{question.prompt}</p>
            <div class="mb-3 flex flex-col gap-2">
              {#each question.options as option}
                <Button
                  variant="outline"
                  class="h-auto flex-col items-start whitespace-normal py-2 text-left"
                  onclick={() => submitAnswer(question.id, 'option', option.title)}
                >
                  <span class="font-semibold">{option.title}</span>
                  {#if option.description}
                    <span class="text-xs font-normal text-muted-foreground">{option.description}</span>
                  {/if}
                </Button>
              {/each}
            </div>
            <div class="mb-2">
              <label class="mb-1 block text-xs text-muted-foreground" for={`custom-${question.id}`}>
                Something else
              </label>
              <div class="flex gap-2">
                <Input id={`custom-${question.id}`} placeholder="Type your own answer" bind:value={customText[question.id]} />
                <Button
                  disabled={!customText[question.id]?.trim()}
                  onclick={() => submitAnswer(question.id, 'custom', customText[question.id]?.trim() ?? '')}
                >
                  Send
                </Button>
              </div>
            </div>
            <div>
              <label class="mb-1 block text-xs text-muted-foreground" for={`decline-${question.id}`}>
                Decline and chat about this
              </label>
              <div class="flex gap-2">
                <Input id={`decline-${question.id}`} placeholder="Optional note for the agent" bind:value={declineText[question.id]} />
                <Button variant="secondary" onclick={() => submitAnswer(question.id, 'declined', declineText[question.id]?.trim() ?? '')}>
                  Decline
                </Button>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if answeredQuestions.length}
      <div class="rounded-lg border border-border bg-card p-4">
        <h2 class="mb-2 text-sm font-semibold">Decisions</h2>
        {#each answeredQuestions as question (question.id)}
          <div class="mb-2 border-l-2 border-border pl-3 last:mb-0">
            <p class="text-sm font-medium">{question.prompt}</p>
            <p class="flex items-center gap-2 text-sm text-muted-foreground">
              <Badge variant="outline">{question.status}</Badge>
              {question.answer || (question.status === 'declined' ? 'Declined to choose' : '')}
            </p>
          </div>
        {/each}
      </div>
    {/if}

    <PaneGroup
      bind:this={paneGroup}
      direction="horizontal"
      autoSaveId="seraphim-task-split-v2"
      class="flex min-h-0 w-full flex-1 overflow-hidden"
    >
      <Resizable.Pane defaultSize={55} minSize={30} class="min-w-0">
        <div class="h-full min-w-0 pr-3">
          <IssueView {task} />
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
                <div class="flex items-start gap-2 py-0.5 {indent}">
                  <span class="w-[1ch] flex-none {markerColor(event.type)}">{marker(event.type)}</span>
                  {#if event.type === 'assistant_text'}
                    <!-- Render the agent's prose as full markdown. -->
                    <div class="min-w-0 flex-1"><Markdown source={describe(event)} /></div>
                  {:else}
                    <span class={lineClasses(event.type, open)}>{describe(event)}</span>
                  {/if}
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
