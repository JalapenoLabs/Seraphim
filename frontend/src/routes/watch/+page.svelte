<script lang="ts">
  // The "watch" page: a full-screen, kiosk-style monitor of the whole agent.
  // Stats bar on top; a big remaining count, a huge live ring for the task being
  // worked, and a big completed count in the middle; and a combined live activity
  // feed across every task along the bottom. Built to be left running on a TV.
  import type { BoardResponse, Task, TaskStatus } from '$lib/types'

  import { onMount, onDestroy, tick } from 'svelte'
  import { Tween } from 'svelte/motion'
  import { cubicOut } from 'svelte/easing'
  import { fly } from 'svelte/transition'

  import { getBoard } from '$lib/api'
  import { STATUS_LABELS } from '$lib/types'
  import { isWithinSchedule } from '$lib/schedule'
  import { describeRateLimit } from '$lib/rateLimit'
  import Stats from '$lib/components/Stats.svelte'

  let board = $state<BoardResponse | null>(null)
  let boardStream: EventSource | null = null
  let activityStream: EventSource | null = null
  let now = $state(Date.now())
  let clock: ReturnType<typeof setInterval> | null = null

  // The rolling activity feed: one entry per meaningful agent action, newest last.
  type FeedEntry = {
    id: number
    taskId: string
    type: string
    text: string
    at: number
    // For CI events, the step status ('step_passed' | 'step_failed' | ...) so the
    // glyph can be colored green/red rather than the type's single color.
    status?: string
  }
  let feed = $state<FeedEntry[]>([])
  let feedSeq = 0
  const MAX_FEED = 60
  let feedEl = $state<HTMLDivElement>()

  const tasks = $derived(board?.tasks ?? [])
  const titleById = $derived(new Map(tasks.map((task) => [task.id, task.title])))

  // Remaining = work the operator has actually committed to (queued through
  // review); completed = the Done lane. "Available" tasks are merely suggestions
  // the operator hasn't picked up yet, so they count toward neither the big
  // number nor the progress bar. Ignored tasks count as neither too.
  const remainingCount = $derived(
    tasks.filter((task) =>
      ['todo', 'in_progress', 'in_review'].includes(task.board_column)
    ).length
  )
  const completedCount = $derived(tasks.filter((task) => task.board_column === 'done').length)
  const inReviewCount = $derived(tasks.filter((task) => task.board_column === 'in_review').length)
  const queuedCount = $derived(tasks.filter((task) => task.board_column === 'todo').length)
  const trackedCount = $derived(remainingCount + completedCount)
  const completionPct = $derived(trackedCount ? (completedCount / trackedCount) * 100 : 0)

  // Smoothly animate the big numbers when they change.
  const remainingShown = new Tween(0, { duration: 800, easing: cubicOut })
  const completedShown = new Tween(0, { duration: 800, easing: cubicOut })
  $effect(() => {
    remainingShown.target = remainingCount
  })
  $effect(() => {
    completedShown.target = completedCount
  })

  // The single task the agent is actively working (single-threaded), if any.
  const WORKING_STATUSES: TaskStatus[] = ['preparing', 'working', 'opening_pr', 'merging']
  const current = $derived(
    tasks.find(
      (task) => task.board_column === 'in_progress' || WORKING_STATUSES.includes(task.status)
    ) ?? null
  )

  // A task is "waiting" when the agent has stopped to ask for input or is blocked
  // on CI, and "heart attack" when a task has crashed and not yet been
  // acknowledged. Both pull the whole page into an attention-grabbing state.
  const hasWaiting = $derived(
    tasks.some((task) => task.status === 'waiting_for_input' || task.status === 'ci_blocked')
  )
  const hasHeartAttack = $derived((board?.heart_attacks?.length ?? 0) > 0)

  // Overall agent state, mirroring the navbar's logic, used to theme the page.
  type StateKey =
    | 'working'
    | 'paused'
    | 'halted'
    | 'heart_attack'
    | 'cooldown'
    | 'waiting'
    | 'offhours'
    | 'idle'
  const agentState = $derived.by<{ key: StateKey; label: string }>(() => {
    const settings = board?.settings
    if (!settings) return { key: 'idle', label: 'Connecting' }
    if (settings.agent_paused) return { key: 'paused', label: 'Paused' }
    if (settings.config_repo_url && settings.config_repo_error)
      return { key: 'halted', label: 'Halted' }
    if (hasHeartAttack) return { key: 'heart_attack', label: 'Heart attack' }
    if (settings.cooldown_until && new Date(settings.cooldown_until) > new Date(now))
      return { key: 'cooldown', label: 'Cooling down' }
    if (hasWaiting) return { key: 'waiting', label: 'Waiting for input' }
    if (current) return { key: 'working', label: 'Working' }
    if (settings.availability_enabled && !isWithinSchedule(settings, new Date(now)))
      return { key: 'offhours', label: 'Off hours' }
    return { key: 'idle', label: 'Idle' }
  })

  // Each state maps to an accent color and a "liveliness": how fast the ring and
  // progress bar animate. Working is full-speed green; waiting/cooling is a
  // half-speed amber warning; paused, halted, or a heart attack is a frozen red
  // alert; nothing-to-do (idle/off-hours) is a still, grey "halted" look.
  type Spin = 'full' | 'half' | 'none'
  const STATE_STYLES: Record<StateKey, { accent: string; spin: Spin }> = {
    working: { accent: '#3fb950', spin: 'full' },
    paused: { accent: '#f85149', spin: 'none' },
    halted: { accent: '#f85149', spin: 'none' },
    heart_attack: { accent: '#f85149', spin: 'none' },
    cooldown: { accent: '#d29922', spin: 'half' },
    waiting: { accent: '#d29922', spin: 'half' },
    offhours: { accent: '#8b97a6', spin: 'none' },
    idle: { accent: '#8b97a6', spin: 'none' }
  }
  const accent = $derived(STATE_STYLES[agentState.key].accent)
  const spin = $derived(STATE_STYLES[agentState.key].spin)
  const isWorking = $derived(agentState.key === 'working')

  // Animation class for the rotating ring, scaled to the current liveliness.
  const ringSpinClass = $derived(
    spin === 'full' ? 'animate-spin-slow' : spin === 'half' ? 'animate-spin-half' : ''
  )
  // The progress bar shares the same liveliness via a flowing sheen.
  const barFlowClass = $derived(
    spin === 'full' ? 'bar-flow' : spin === 'half' ? 'bar-flow bar-flow-half' : ''
  )

  // The standby glyph and message shown in the ring when no task is in flight.
  const STANDBY_GLYPH: Record<StateKey, string> = {
    working: '✦',
    paused: '⏸',
    halted: '✕',
    heart_attack: '✕',
    cooldown: '◷',
    waiting: '?',
    offhours: '☾',
    idle: '✦'
  }
  const STANDBY_MESSAGE: Record<StateKey, string> = {
    working: 'Standing by for the next task.',
    paused: 'The agent is paused.',
    halted: 'Needs attention to resume.',
    heart_attack: 'A task crashed and needs attention.',
    cooldown: 'Cooling down before the next task.',
    waiting: 'Waiting on your input to continue.',
    offhours: 'Outside working hours.',
    idle: 'Standing by for the next task.'
  }

  // --- Activity feed wiring ---------------------------------------------------

  // Only the "pure agentic" signals make the cut; tool results and infra noise
  // are dropped so the feed reads like a clean session transcript.
  const GLYPHS: Record<string, { glyph: string; color: string }> = {
    prompt: { glyph: '◆', color: 'text-prompt' },
    thinking: { glyph: '✻', color: 'text-warning' },
    assistant_text: { glyph: '▸', color: 'text-foreground' },
    tool_use: { glyph: '⚙', color: 'text-primary' },
    result: { glyph: '✓', color: 'text-success' },
    rate_limit: { glyph: '◷', color: 'text-info' },
    ci: { glyph: '●', color: 'text-info' }
  }

  // CI glyph color follows the step status (green pass / red fail / info running),
  // since one CI type covers all three outcomes.
  function ciGlyphColor(status: string | undefined): string {
    if (status === 'step_failed') {
      return 'text-destructive'
    }
    if (status === 'step_passed' || status === 'job_passed') {
      return 'text-success'
    }
    return 'text-info'
  }

  function firstLine(text: unknown, max = 120): string {
    const line = String(text ?? '')
      .split('\n')
      .map((part) => part.trim())
      .find((part) => part.length > 0)
    if (!line) return ''
    return line.length > max ? `${line.slice(0, max - 1)}…` : line
  }

  function toolLine(payload: Record<string, unknown>): string {
    const name = String(payload?.name ?? 'tool')
    const input = (payload?.input ?? {}) as Record<string, unknown>
    const arg =
      input.path ?? input.file_path ?? input.command ?? input.pattern ?? input.url ?? input.description
    return arg ? `${name} · ${firstLine(arg, 64)}` : name
  }

  // Turns one streamed event into a feed line, or null to drop it.
  function summarize(type: string, payload: Record<string, unknown>): string | null {
    switch (type) {
      case 'prompt':
        return 'new briefing'
      case 'thinking':
        return firstLine(payload?.thinking) || 'thinking…'
      case 'assistant_text':
        return firstLine(payload?.text)
      case 'tool_use':
        return toolLine(payload)
      case 'result':
        return 'turn complete'
      case 'rate_limit':
        return describeRateLimit(payload)
      case 'ci':
        return firstLine(payload?.text)
      default:
        return null
    }
  }

  async function pushFeed(taskId: string, type: string, payload: Record<string, unknown>, at: number) {
    const text = summarize(type, payload)
    if (!text) return
    const status = type === 'ci' ? String(payload?.status ?? '') : undefined
    const entry: FeedEntry = { id: feedSeq++, taskId, type, text, at, status }
    feed = [...feed, entry].slice(-MAX_FEED)
    // Keep the newest line in view (terminal-style autoscroll).
    await tick()
    feedEl?.scrollTo({ top: feedEl.scrollHeight, behavior: 'smooth' })
  }

  // A stable hue per task, so each task's lines share a recognizable color.
  function taskHue(taskId: string): number {
    let hash = 0
    for (let i = 0; i < taskId.length; i++) {
      hash = (hash * 31 + taskId.charCodeAt(i)) % 360
    }
    return hash
  }

  function taskLabel(taskId: string): string {
    const title = titleById.get(taskId)
    if (!title) return 'task'
    return title.length > 28 ? `${title.slice(0, 27)}…` : title
  }

  function clockLabel(at: number): string {
    return new Date(at).toLocaleTimeString([], { hour12: false })
  }

  async function loadBoard() {
    try {
      board = await getBoard()
    } catch {
      // Transient; the next stream tick retries.
    }
  }

  onMount(() => {
    loadBoard()
    clock = setInterval(() => (now = Date.now()), 1000)

    boardStream = new EventSource('/api/v1/board/stream')
    boardStream.addEventListener('board', () => loadBoard())

    activityStream = new EventSource('/api/v1/activity/stream')
    activityStream.addEventListener('activity', (message) => {
      try {
        const data = JSON.parse((message as MessageEvent).data) as {
          task_id: string
          event: { type: string; payload: Record<string, unknown>; created_at?: string }
        }
        const at = data.event.created_at ? new Date(data.event.created_at).getTime() : Date.now()
        void pushFeed(data.task_id, data.event.type, data.event.payload ?? {}, at)
      } catch {
        // Ignore malformed frames.
      }
    })
  })

  onDestroy(() => {
    if (clock) clearInterval(clock)
    boardStream?.close()
    activityStream?.close()
  })
</script>

<div class="watch relative h-full w-full overflow-hidden bg-[#070a12] text-foreground" style="--accent: {accent}">
  <!-- Drifting aura + grid give the static page some life. -->
  <div class="aura aura-1" aria-hidden="true"></div>
  <div class="aura aura-2" aria-hidden="true"></div>
  <div class="grid-overlay pointer-events-none absolute inset-0" aria-hidden="true"></div>

  <div class="relative z-10 flex h-full flex-col gap-5 p-6 xl:p-8">
    <!-- Stats bar -->
    <header class="flex items-center justify-between gap-4">
      <a href="/" class="flex items-center gap-2 text-lg font-bold tracking-tight">
        <img
          src="/favicon.png"
          alt=""
          class="h-7 w-7"
          onerror={(event) => ((event.currentTarget as HTMLImageElement).style.display = 'none')}
        />
        Seraphim <span class="text-muted-foreground">· Watch</span>
      </a>
      <span
        class="status-pill inline-flex items-center gap-2 rounded-full border px-4 py-1.5 text-sm font-semibold"
        style="border-color: color-mix(in srgb, var(--accent) 45%, transparent); color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, transparent)"
      >
        <span class="size-2.5 rounded-full {spin !== 'none' ? 'animate-ping-slow' : ''}" style="background: var(--accent)"></span>
        {agentState.label}
      </span>
    </header>

    <div class="shrink-0"><Stats /></div>

    <!-- Big trio: remaining · live ring · completed -->
    <section class="grid grid-cols-1 items-center gap-6 lg:grid-cols-3">
      <!-- Remaining -->
      <div class="flex flex-col items-center lg:items-end lg:pr-4 lg:text-right">
        <div
          class="text-[clamp(3.5rem,9vw,8rem)] font-black leading-none tabular-nums text-warning [text-shadow:0_0_40px_color-mix(in_srgb,var(--warning)_45%,transparent)]"
        >
          {Math.round(remainingShown.current)}
        </div>
        <div class="mt-1 text-sm font-semibold uppercase tracking-[0.2em] text-muted-foreground">
          Remaining
        </div>
        <div class="mt-1 text-xs text-muted-foreground">
          {queuedCount} queued · {inReviewCount} in review
        </div>
      </div>

      <!-- Live ring -->
      <div class="grid place-items-center py-2">
        <div class="relative grid size-[clamp(15rem,26vw,24rem)] place-items-center">
          <!-- pulsing aura behind the ring -->
          <div
            class="absolute inset-0 rounded-full blur-2xl {spin !== 'none' ? 'animate-breathe' : 'opacity-20'}"
            style="background: radial-gradient(circle, color-mix(in srgb, var(--accent) 55%, transparent), transparent 70%)"
          ></div>
          <!-- rotating gradient ring -->
          <div
            class="absolute inset-0 rounded-full {ringSpinClass}"
            style="background: conic-gradient(from 0deg, transparent 0%, var(--accent) 18%, transparent 38%, color-mix(in srgb, var(--accent) 60%, transparent) 60%, transparent 80%, var(--accent) 100%)"
          ></div>
          <!-- a comet dot orbiting the ring while working -->
          {#if isWorking}
            <div class="animate-orbit absolute inset-0" aria-hidden="true">
              <span
                class="absolute left-1/2 top-0 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full"
                style="background: var(--accent); box-shadow: 0 0 14px 2px var(--accent)"
              ></span>
            </div>
          {/if}
          <!-- inner face -->
          <div
            class="absolute inset-[14px] grid place-items-center rounded-full border border-white/5 bg-[#0a0e1a]/90 p-6 text-center backdrop-blur"
          >
            {#if current}
              {#key current.id}
                <div class="flex flex-col items-center gap-2" in:fly={{ y: 10, duration: 350 }}>
                  <span class="text-[0.65rem] font-semibold uppercase tracking-[0.3em] text-success">
                    Now working
                  </span>
                  <span class="line-clamp-3 text-balance text-lg font-semibold leading-snug xl:text-xl">
                    {current.title}
                  </span>
                  <span class="text-xs text-muted-foreground">#{current.external_id}</span>
                  <span
                    class="mt-1 inline-flex items-center gap-1.5 rounded-full bg-success/10 px-3 py-1 text-xs font-medium text-success"
                  >
                    <span class="size-1.5 animate-pulse rounded-full bg-success"></span>
                    {STATUS_LABELS[current.status] ?? current.status}
                  </span>
                </div>
              {/key}
            {:else}
              <div class="flex flex-col items-center gap-2 text-muted-foreground">
                <span class="text-4xl">{STANDBY_GLYPH[agentState.key]}</span>
                <span class="text-sm font-semibold uppercase tracking-[0.3em]">{agentState.label}</span>
                <span class="max-w-[12rem] text-xs">{STANDBY_MESSAGE[agentState.key]}</span>
              </div>
            {/if}
          </div>
        </div>
      </div>

      <!-- Completed -->
      <div class="flex flex-col items-center lg:items-start lg:pl-4 lg:text-left">
        <div
          class="text-[clamp(3.5rem,9vw,8rem)] font-black leading-none tabular-nums text-success [text-shadow:0_0_40px_color-mix(in_srgb,var(--success)_45%,transparent)]"
        >
          {Math.round(completedShown.current)}
        </div>
        <div class="mt-1 text-sm font-semibold uppercase tracking-[0.2em] text-muted-foreground">
          Completed
        </div>
        <div class="mt-1 text-xs text-muted-foreground">shipped to done</div>
      </div>
    </section>

    <!-- Overall completion progress -->
    <div class="flex items-center gap-3 px-1">
      <span class="w-16 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
        {completedCount}/{trackedCount}
      </span>
      <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-white/5">
        <div
          class="h-full rounded-full transition-[width] duration-700 ease-out {barFlowClass}"
          style="width: {completionPct}%; background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 40%, transparent), var(--accent), color-mix(in srgb, var(--accent) 40%, transparent)); background-size: 200% 100%; box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 55%, transparent)"
        ></div>
      </div>
      <span class="w-10 shrink-0 text-xs tabular-nums text-muted-foreground">
        {Math.round(completionPct)}%
      </span>
    </div>

    <!-- Live activity across every task -->
    <section class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-border/60 bg-black/30 backdrop-blur">
      <div class="flex items-center gap-2 border-b border-border/60 px-4 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-muted-foreground">
        <span class="size-2 rounded-full bg-primary {feed.length ? 'animate-pulse' : ''}"></span>
        Live activity
      </div>
      <div bind:this={feedEl} class="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-4 py-3 font-mono text-sm">
        {#if feed.length === 0}
          <p class="text-muted-foreground">Waiting for the agent to act…</p>
        {/if}
        {#each feed as entry (entry.id)}
          {@const meta = GLYPHS[entry.type] ?? { glyph: '·', color: 'text-muted-foreground' }}
          {@const glyphColor = entry.type === 'ci' ? ciGlyphColor(entry.status) : meta.color}
          {@const hue = taskHue(entry.taskId)}
          <div class="flex items-center gap-3 py-0.5" in:fly={{ y: 8, duration: 250, easing: cubicOut }}>
            <span class="w-[4.5rem] shrink-0 text-xs tabular-nums text-muted-foreground/60">
              {clockLabel(entry.at)}
            </span>
            <span
              class="w-44 shrink-0 truncate rounded px-1.5 py-0.5 text-xs font-medium"
              style="color: hsl({hue} 75% 72%); background: hsl({hue} 75% 55% / 0.12)"
              title={titleById.get(entry.taskId) ?? entry.taskId}
            >
              {taskLabel(entry.taskId)}
            </span>
            <span class="w-[1ch] shrink-0 text-center {glyphColor}">{meta.glyph}</span>
            <span class="min-w-0 flex-1 truncate text-foreground/90">{entry.text}</span>
          </div>
        {/each}
      </div>
    </section>
  </div>
</div>

<style>
  .aura {
    position: absolute;
    border-radius: 9999px;
    filter: blur(90px);
    opacity: 0.22;
    pointer-events: none;
  }
  .aura-1 {
    width: 42vw;
    height: 42vw;
    left: -6vw;
    top: -8vw;
    background: radial-gradient(circle, var(--accent), transparent 70%);
    animation: drift1 20s ease-in-out infinite;
  }
  .aura-2 {
    width: 38vw;
    height: 38vw;
    right: -8vw;
    bottom: -12vw;
    background: radial-gradient(circle, var(--primary), transparent 70%);
    animation: drift2 26s ease-in-out infinite;
  }
  .grid-overlay {
    opacity: 0.05;
    background-image:
      linear-gradient(var(--foreground) 1px, transparent 1px),
      linear-gradient(90deg, var(--foreground) 1px, transparent 1px);
    background-size: 44px 44px;
    mask-image: radial-gradient(ellipse at center, black 30%, transparent 85%);
  }

  :global(.watch) .animate-spin-slow {
    animation: spin 5s linear infinite;
  }
  /* Half-speed spin for the amber "waiting"/"cooling down" warning state. */
  :global(.watch) .animate-spin-half {
    animation: spin 10s linear infinite;
  }
  /* The progress bar's sheen flows at the same cadence as the ring spins. */
  :global(.watch) .bar-flow {
    animation: barflow 2.5s linear infinite;
  }
  :global(.watch) .bar-flow-half {
    animation-duration: 5s;
  }
  :global(.watch) .animate-breathe {
    animation: breathe 3.5s ease-in-out infinite;
  }
  :global(.watch) .animate-ping-slow {
    animation: ping-slow 2s cubic-bezier(0, 0, 0.2, 1) infinite;
  }
  :global(.watch) .animate-orbit {
    animation: spin 8s linear infinite;
  }

  @keyframes drift1 {
    0%,
    100% {
      transform: translate(0, 0);
    }
    50% {
      transform: translate(6vw, 4vh);
    }
  }
  @keyframes drift2 {
    0%,
    100% {
      transform: translate(0, 0);
    }
    50% {
      transform: translate(-5vw, -3vh);
    }
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes barflow {
    to {
      background-position: -200% 0;
    }
  }
  @keyframes breathe {
    0%,
    100% {
      opacity: 0.3;
      transform: scale(0.96);
    }
    50% {
      opacity: 0.6;
      transform: scale(1.04);
    }
  }
  @keyframes ping-slow {
    0% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 60%, transparent);
    }
    70%,
    100% {
      box-shadow: 0 0 0 8px transparent;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .aura,
    :global(.watch) .animate-spin-slow,
    :global(.watch) .animate-spin-half,
    :global(.watch) .bar-flow,
    :global(.watch) .animate-breathe,
    :global(.watch) .animate-ping-slow,
    :global(.watch) .animate-orbit {
      animation: none;
    }
  }
</style>
