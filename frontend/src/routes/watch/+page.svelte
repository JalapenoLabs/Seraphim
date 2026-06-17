<script lang="ts">
  // The "watch" page: a full-screen, kiosk-style monitor of the whole agent.
  // Stats bar on top; a big remaining count, a huge live ring for the task being
  // worked, and a big completed count in the middle; and a combined live activity
  // feed across every task along the bottom. Built to be left running on a TV.
  import type { BoardResponse, Stats, Task, TaskStatus } from '$lib/types'
  import type { RunewoodController, RunewoodHighlight, Hsl } from 'runewood'

  import { onMount, onDestroy, tick } from 'svelte'
  import { browser } from '$app/environment'
  import { Tween } from 'svelte/motion'
  import { cubicOut } from 'svelte/easing'
  import { fly } from 'svelte/transition'

  import { getBoard, getGlobalStats, getRepoTree } from '$lib/api'
  import { STATUS_LABELS } from '$lib/types'
  import { isWithinSchedule } from '$lib/schedule'
  import { describeRateLimit } from '$lib/rateLimit'
  import { mapActivityEvent, DEFAULT_EXCLUDES } from '$lib/runewood/mapEvent'
  import UsageGauges from '$lib/components/stats/UsageGauges.svelte'
  import LifetimeTotals from '$lib/components/stats/LifetimeTotals.svelte'

  let board = $state<BoardResponse | null>(null)
  let boardStream: EventSource | null = null
  let activityStream: EventSource | null = null
  let now = $state(Date.now())
  let clock: ReturnType<typeof setInterval> | null = null

  // Global agent stats flank the hero ring (the kiosk has no separate stats bar).
  // We fetch them here and pass them to the presentational gauge/total components,
  // so the page holds a single stats source rather than two polling widgets.
  let stats = $state<Stats | null>(null)
  let statsFetchedAt = $state(Date.now())
  let statsPoll: ReturnType<typeof setInterval> | null = null

  // A rolling 24h history of the cumulative cost/token totals, sampled every few
  // minutes and persisted to localStorage so the burn-rate sparklines survive a
  // kiosk reload. The sparklines plot the per-interval deltas (the burn), not the
  // ever-climbing totals.
  type StatSample = { at: number; cost: number; tokens: number }
  const HISTORY_KEY = 'seraphim:watch:stat-history'
  const HISTORY_WINDOW_MS = 24 * 60 * 60 * 1000
  const HISTORY_SAMPLE_MS = 5 * 60 * 1000
  let statHistory = $state<StatSample[]>([])

  const costHistory = $derived(
    statHistory.length >= 2
      ? statHistory.slice(1).map((sample, index) => Math.max(0, sample.cost - statHistory[index].cost))
      : []
  )
  const tokenHistory = $derived(
    statHistory.length >= 2
      ? statHistory.slice(1).map((sample, index) => Math.max(0, sample.tokens - statHistory[index].tokens))
      : []
  )

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

  // A task is "waiting" only when the agent has actually stopped to ask the
  // operator a question; a "heart attack" is a task that crashed and has not yet
  // been acknowledged. Both pull the whole page into an attention-grabbing state.
  const hasWaiting = $derived(tasks.some((task) => task.status === 'waiting_for_input'))
  // A task parked after CI gave up needs a human, but it is not an open question
  // for the operator, so it gets its own "Blocked" state rather than reading as
  // "Waiting for input" (which would send the operator hunting the board for a
  // prompt that does not exist).
  const hasBlocked = $derived(tasks.some((task) => task.status === 'ci_blocked'))
  const hasHeartAttack = $derived((board?.heart_attacks?.length ?? 0) > 0)

  // Overall agent state, mirroring the navbar's logic, used to theme the page.
  type StateKey =
    | 'working'
    | 'paused'
    | 'halted'
    | 'heart_attack'
    | 'cooldown'
    | 'waiting'
    | 'blocked'
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
    if (hasBlocked) return { key: 'blocked', label: 'Blocked' }
    if (current) return { key: 'working', label: 'Working' }
    if (settings.availability_enabled && !isWithinSchedule(settings, new Date(now)))
      return { key: 'offhours', label: 'Off hours' }
    return { key: 'idle', label: 'Idle' }
  })

  // Each state maps to an accent color and a "liveliness": how fast the ring and
  // progress bar animate. Working is full-speed green; waiting/cooling is a
  // half-speed amber warning; paused, halted, or a heart attack is a frozen red
  // alert; nothing-to-do (idle/off-hours) is a still, grey "halted" look.
  // `frame` drives the full-bleed state glow around the whole page (issue: TV
  // glanceability): a calm breathing green when working, a steady amber when
  // waiting/blocked, an urgent pulse for red alerts, and a faint wash when idle.
  type Spin = 'full' | 'half' | 'none'
  type Frame = 'breathe' | 'steady' | 'alert' | 'soft'
  const STATE_STYLES: Record<StateKey, { accent: string; spin: Spin; frame: Frame }> = {
    working: { accent: '#3fb950', spin: 'full', frame: 'breathe' },
    paused: { accent: '#f85149', spin: 'none', frame: 'steady' },
    halted: { accent: '#f85149', spin: 'none', frame: 'alert' },
    heart_attack: { accent: '#f85149', spin: 'none', frame: 'alert' },
    cooldown: { accent: '#d29922', spin: 'half', frame: 'steady' },
    waiting: { accent: '#d29922', spin: 'half', frame: 'steady' },
    blocked: { accent: '#d29922', spin: 'half', frame: 'steady' },
    offhours: { accent: '#8b97a6', spin: 'none', frame: 'soft' },
    idle: { accent: '#8b97a6', spin: 'none', frame: 'soft' }
  }
  const accent = $derived(STATE_STYLES[agentState.key].accent)
  const spin = $derived(STATE_STYLES[agentState.key].spin)
  const frame = $derived(STATE_STYLES[agentState.key].frame)
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
    blocked: '⚠',
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
    blocked: 'A task is blocked and needs attention.',
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
    ci: { glyph: '●', color: 'text-info' },
    lifecycle: { glyph: '⬢', color: 'text-primary' }
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

  // Lifecycle glyph color follows the action (#226), since one lifecycle type
  // covers opened / merged / closed: a merge or an issue closed-on-done is
  // progress (green), a PR closed without merging is an abandonment (red), and a
  // freshly opened PR is the neutral primary.
  function lifecycleGlyphColor(action: string | undefined): string {
    if (action === 'pr_merged' || action === 'issue_closed') {
      return 'text-success'
    }
    if (action === 'pr_closed') {
      return 'text-destructive'
    }
    return 'text-primary'
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
      case 'lifecycle':
        return lifecycleLine(payload)
      default:
        return null
    }
  }

  // Formats a PR/issue lifecycle event into its feed line (#226). The repo is
  // named (`repo#number`) only when the backend marked the task multi-repo (it
  // sends an empty `repo` otherwise), matching how CI stays untagged for a single
  // PR. The issue line already carries the number, so it only prefixes the repo.
  function lifecycleLine(payload: Record<string, unknown>): string | null {
    const action = String(payload?.action ?? '')
    const repo = String(payload?.repo ?? '')
    const number = payload?.number
    const title = firstLine(payload?.title)
    switch (action) {
      case 'pr_opened':
        return `${repo ? `${repo}#${number} ` : ''}PR opened: ${title}`
      case 'pr_merged':
        return `${repo ? `${repo}#${number} ` : ''}PR merged: ${title}`
      case 'pr_closed':
        return `${repo ? `${repo}#${number} ` : ''}PR closed: ${title}`
      case 'issue_closed':
        return `${repo ? `${repo} ` : ''}Issue closed: #${number}`
      default:
        return null
    }
  }

  async function pushFeed(taskId: string, type: string, payload: Record<string, unknown>, at: number) {
    const text = summarize(type, payload)
    if (!text) return
    const status =
      type === 'ci'
        ? String(payload?.status ?? '')
        : type === 'lifecycle'
          ? String(payload?.action ?? '')
          : undefined
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

  // Humanize an event's age for the row's hover title, e.g. "3 minutes ago". The
  // exact clock time is appended so the tooltip is both readable and precise.
  function humanizeTime(at: number, ref: number): string {
    const seconds = Math.max(0, Math.round((ref - at) / 1000))
    let relative: string
    if (seconds < 5) {
      relative = 'just now'
    } else if (seconds < 60) {
      relative = `${seconds} seconds ago`
    } else {
      const minutes = Math.round(seconds / 60)
      if (minutes < 60) {
        relative = `${minutes} minute${minutes === 1 ? '' : 's'} ago`
      } else {
        const hours = Math.round(minutes / 60)
        if (hours < 24) {
          relative = `${hours} hour${hours === 1 ? '' : 's'} ago`
        } else {
          const days = Math.round(hours / 24)
          relative = `${days} day${days === 1 ? '' : 's'} ago`
        }
      }
    }
    return `${relative} · ${clockLabel(at)}`
  }

  // --- Activity forest (runewood, issue #180) ---------------------------------

  // The Gource-style live forest. Created on mount (WebGL is browser-only) and
  // fed the same `activity` stream through the pure mapper. `runewood` is imported
  // dynamically so the WebGL bundle never loads during SSR/build.
  let forestEl = $state<HTMLDivElement>()
  let forest: RunewoodController | null = null
  let forestReady = $state(false)
  // Tooltip for the hovered node (driven by runewood's nodeHover event).
  let hoverTip = $state<{ path: string; x: number; y: number } | null>(null)

  // CI highlight, driven off the board's per-task status (issue #180): light up
  // the files a PR touched while its CI runs, and clear them when it settles.
  // Touched files accumulate per task from the live stream; the color tracks the
  // task's status (amber under review/CI, green about to merge, red failing).
  const touchedFiles = new Map<string, Set<string>>()
  const highlightHandles = new Map<string, { handle: RunewoodHighlight; colorKey: string }>()
  const CI_COLORS = {
    running: { h: 38, s: 0.9, l: 0.55 }, // amber
    passing: { h: 140, s: 0.6, l: 0.5 }, // green
    failing: { h: 0, s: 0.75, l: 0.55 } // red
  } as const satisfies Record<string, Hsl>

  // The highlight color for a task's PR, or null when it should not be lit. Only
  // In Review tasks (which have an open PR) are highlighted.
  function ciColor(task: Task): Hsl | null {
    if (task.board_column !== 'in_review') {
      return null
    }
    if (task.status === 'ci_failing' || task.status === 'ci_blocked') {
      return CI_COLORS.failing
    }
    if (task.status === 'merging') {
      return CI_COLORS.passing
    }
    return CI_COLORS.running
  }

  // Reconcile the live highlights with the current board: create/grow/recolor a
  // group per highlightable task, and clear groups whose task has settled.
  function reconcileHighlights() {
    if (!forest) {
      return
    }
    const active = new Set<string>()
    for (const task of tasks) {
      const color = ciColor(task)
      if (!color) {
        continue
      }
      const files = [...(touchedFiles.get(task.id) ?? [])]
      if (files.length === 0) {
        continue
      }
      active.add(task.id)
      const colorKey = `${color.h},${color.s},${color.l}`
      const existing = highlightHandles.get(task.id)
      if (!existing) {
        const handle = forest.highlight(files, { id: task.id, color })
        highlightHandles.set(task.id, { handle, colorKey })
      } else {
        existing.handle.update(files)
        // Re-highlighting the same id replaces the group, which is how a color
        // change (e.g. amber -> red) is encoded.
        if (existing.colorKey !== colorKey) {
          const handle = forest.highlight(files, { id: task.id, color })
          highlightHandles.set(task.id, { handle, colorKey })
        }
      }
    }
    for (const [taskId, entry] of highlightHandles) {
      if (!active.has(taskId)) {
        entry.handle.clear()
        highlightHandles.delete(taskId)
      }
    }
  }

  // Re-reconcile whenever the board changes (status transitions, settled PRs).
  $effect(() => {
    tasks
    reconcileHighlights()
  })

  async function loadBoard() {
    try {
      board = await getBoard()
    } catch {
      // Transient; the next stream tick retries.
    }
  }

  async function loadStats() {
    try {
      stats = await getGlobalStats()
      statsFetchedAt = Date.now()
      recordSample(stats)
    } catch (error) {
      console.debug('failed to load watch stats', error)
    }
  }

  function loadHistory() {
    if (!browser) {
      return
    }
    try {
      const raw = localStorage.getItem(HISTORY_KEY)
      if (!raw) {
        return
      }
      const parsed = JSON.parse(raw) as StatSample[]
      const cutoff = Date.now() - HISTORY_WINDOW_MS
      statHistory = parsed.filter((sample) => sample.at >= cutoff)
    } catch (error) {
      console.debug('failed to load stat history', error)
    }
  }

  function recordSample(snapshot: Stats) {
    if (!browser) {
      return
    }
    const at = Date.now()
    const last = statHistory[statHistory.length - 1]
    // One sample per interval keeps the 24h buffer small and the sparkline legible.
    if (last && at - last.at < HISTORY_SAMPLE_MS) {
      return
    }
    const cutoff = at - HISTORY_WINDOW_MS
    statHistory = [...statHistory, { at, cost: snapshot.cost_usd, tokens: snapshot.total_tokens }].filter(
      (sample) => sample.at >= cutoff
    )
    try {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(statHistory))
    } catch (error) {
      console.debug('failed to persist stat history', error)
    }
  }

  // Worked time counts up live between fetches (mirroring the Stats panel): the
  // server's `worked_ms` already includes each running turn up to the fetch, so we
  // add only the time elapsed since, scaled by the number of running turns.
  const workedMs = $derived(
    stats ? stats.worked_ms + stats.running_turns * Math.max(0, now - statsFetchedAt) : 0
  )

  onMount(() => {
    loadBoard()
    loadHistory()
    loadStats()
    clock = setInterval(() => (now = Date.now()), 1000)
    // A slow baseline poll reconciles with the persisted totals; the live ticking
    // comes from the `usage` SSE nudge below and the per-second clock.
    statsPoll = setInterval(loadStats, 5000)

    boardStream = new EventSource('/api/v1/board/stream')
    boardStream.addEventListener('board', () => loadBoard())
    boardStream.addEventListener('usage', () => loadStats())

    activityStream = new EventSource('/api/v1/activity/stream')
    activityStream.addEventListener('activity', (message) => {
      try {
        const data = JSON.parse((message as MessageEvent).data) as {
          task_id: string
          event: { type: string; payload: Record<string, unknown>; created_at?: string }
        }
        const at = data.event.created_at ? new Date(data.event.created_at).getTime() : Date.now()
        void pushFeed(data.task_id, data.event.type, data.event.payload ?? {}, at)

        // Feed the forest through the pure mapper. A file-touch event also grows
        // its task's CI highlight set.
        const mapped = mapActivityEvent(data, titleById.get(data.task_id))
        if (mapped) {
          forest?.ingest(mapped)
          if (mapped.path) {
            const files = touchedFiles.get(data.task_id) ?? new Set<string>()
            files.add(mapped.path)
            touchedFiles.set(data.task_id, files)
            reconcileHighlights()
          }
        }
      } catch {
        // Ignore malformed frames.
      }
    })

    // Create the forest once the DOM node exists. Browser-only; the dynamic import
    // keeps the WebGL bundle out of SSR/build evaluation.
    if (browser && forestEl) {
      import('runewood')
        .then(({ createRunewood }) => {
          if (!forestEl) {
            return
          }
          forest = createRunewood(forestEl, {
            theme: 'void',
            bloom: 'off',
            cameraMode: 'follow',
            rootLabel: 'workspace',
            exclude: DEFAULT_EXCLUDES,
            // Keep a contributor lingering on its last file across edit gaps so a
            // live agent feed reads as continuous activity, not flicker.
            actorLingerMs: 60 * 60 * 1000
          })
          forest.on('nodeHover', (hit) => {
            hoverTip = hit ? { path: hit.path, x: hit.screen.x, y: hit.screen.y } : null
          })
          forestReady = true
          // Seed the full repo structure so a load/refresh shows the whole forest
          // immediately as dim nodes, instead of building from empty as live events
          // arrive (issue #216). runewood queues the seed until its renderer is ready,
          // and applies the same `exclude` path filter to seeded paths.
          getRepoTree()
            .then(({ paths }) => forest?.seed(paths))
            .catch((error) => console.debug('failed to seed activity forest', error))
          // Any PRs already In Review get lit as soon as their files stream in.
          reconcileHighlights()
        })
        .catch((error) => console.debug('runewood failed to load', error))
    }
  })

  onDestroy(() => {
    if (clock) clearInterval(clock)
    if (statsPoll) clearInterval(statsPoll)
    boardStream?.close()
    activityStream?.close()
    forest?.destroy()
  })
</script>

<div class="watch relative h-full w-full overflow-hidden bg-[#070a12] text-foreground" style="--accent: {accent}">
  <!-- Drifting aura + grid give the static page some life. -->
  <div class="aura aura-1" aria-hidden="true"></div>
  <div class="aura aura-2" aria-hidden="true"></div>
  <div class="grid-overlay pointer-events-none absolute inset-0" aria-hidden="true"></div>
  <!-- State frame: a full-bleed inner glow tinted by the agent state, so the page
       reads as healthy / waiting / alert from across the room. -->
  <div class="state-frame state-frame-{frame}" aria-hidden="true"></div>

  <div class="relative z-10 flex h-full flex-col gap-5 p-6 xl:p-8">
    <!-- Title + live agent-state pill -->
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

    <!-- Hero band: usage gauges + Remaining on the left, the live ring in the
         center, Completed + lifetime totals on the right. Filling the flanks with
         the stats kills the dead space and keeps everything glanceable at once. -->
    <section class="flex flex-wrap items-center justify-between gap-x-8 gap-y-6 lg:flex-nowrap">
      <!-- Left flank: the three usage gauges, then Remaining nearest the ring. -->
      <div class="flex flex-1 items-center justify-center gap-6 lg:justify-end lg:gap-8">
        {#if stats}
          <UsageGauges {stats} gaugeSize="size-[clamp(4.5rem,6vw,6rem)]" />
        {/if}
        <div class="flex flex-col items-center lg:items-end lg:text-right">
          <div
            class="text-[clamp(2.1rem,5.4vw,4.8rem)] font-black leading-none tabular-nums text-warning [text-shadow:0_0_40px_color-mix(in_srgb,var(--warning)_45%,transparent)]"
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
      </div>

      <!-- Live ring (center) -->
      <div class="grid shrink-0 place-items-center py-2">
        <div class="relative grid size-[clamp(9.5rem,16vw,15rem)] place-items-center">
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
            class="absolute inset-[12px] grid place-items-center rounded-full border border-white/5 bg-[#0a0e1a]/90 p-4 text-center backdrop-blur"
          >
            {#if current}
              {#key current.id}
                <div class="flex flex-col items-center gap-1.5" in:fly={{ y: 10, duration: 350 }}>
                  <span class="text-[0.5rem] font-semibold uppercase tracking-[0.3em] text-success">
                    Now working
                  </span>
                  <span class="line-clamp-3 text-balance text-sm font-semibold leading-snug xl:text-base">
                    {current.title}
                  </span>
                  <span class="text-[0.7rem] text-muted-foreground">#{current.external_id}</span>
                  <span
                    class="mt-0.5 inline-flex items-center gap-1.5 rounded-full bg-success/10 px-2.5 py-0.5 text-[0.7rem] font-medium text-success"
                  >
                    <span class="size-1.5 animate-pulse rounded-full bg-success"></span>
                    {STATUS_LABELS[current.status] ?? current.status}
                  </span>
                </div>
              {/key}
            {:else}
              <div class="flex flex-col items-center gap-1.5 text-muted-foreground">
                <span class="text-3xl">{STANDBY_GLYPH[agentState.key]}</span>
                <span class="text-xs font-semibold uppercase tracking-[0.3em]">{agentState.label}</span>
                <span class="max-w-[9rem] text-[0.7rem] leading-snug">{STANDBY_MESSAGE[agentState.key]}</span>
              </div>
            {/if}
          </div>
        </div>
      </div>

      <!-- Right flank: Completed nearest the ring, then the lifetime totals. -->
      <div class="flex flex-1 items-center justify-center gap-6 lg:justify-start lg:gap-8">
        <div class="flex flex-col items-center lg:items-start lg:text-left">
          <div
            class="text-[clamp(2.1rem,5.4vw,4.8rem)] font-black leading-none tabular-nums text-success [text-shadow:0_0_40px_color-mix(in_srgb,var(--success)_45%,transparent)]"
          >
            {Math.round(completedShown.current)}
          </div>
          <div class="mt-1 text-sm font-semibold uppercase tracking-[0.2em] text-muted-foreground">
            Completed
          </div>
          <div class="mt-1 text-xs text-muted-foreground">shipped to done</div>
        </div>
        {#if stats}
          <LifetimeTotals {stats} {workedMs} {costHistory} {tokenHistory} />
        {/if}
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

    <!-- Bottom: live activity log (1/3) beside the live activity forest (2/3). -->
    <section class="flex min-h-0 flex-1 gap-5">
      <!-- Live activity log -->
      <div class="flex w-1/3 min-w-0 flex-col overflow-hidden rounded-xl border border-border/60 bg-black/30 backdrop-blur">
        <div bind:this={feedEl} class="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-3 py-3 font-mono text-xs">
          {#if feed.length === 0}
            <p class="text-muted-foreground">Waiting for the agent to act…</p>
          {/if}
          {#each feed as entry (entry.id)}
            {@const meta = GLYPHS[entry.type] ?? { glyph: '·', color: 'text-muted-foreground' }}
            {@const glyphColor =
              entry.type === 'ci'
                ? ciGlyphColor(entry.status)
                : entry.type === 'lifecycle'
                  ? lifecycleGlyphColor(entry.status)
                  : meta.color}
            {@const hue = taskHue(entry.taskId)}
            <div
              class="flex items-center gap-2 py-0.5"
              title={humanizeTime(entry.at, now)}
              in:fly={{ y: 8, duration: 250, easing: cubicOut }}
            >
              <span
                class="w-28 shrink-0 truncate rounded px-1.5 py-0.5 font-medium"
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
      </div>

      <!-- Live activity forest (runewood): a Gource-style view of every file the
           agents touch, lit up per PR while its CI runs. -->
      <div class="relative w-2/3 min-w-0 overflow-hidden rounded-xl border border-border/60 bg-black/30 backdrop-blur">
        <div bind:this={forestEl} class="absolute inset-0"></div>
        {#if !forestReady}
          <div class="pointer-events-none absolute inset-0 grid place-items-center text-xs text-muted-foreground">
            Spinning up the activity forest…
          </div>
        {/if}
        {#if hoverTip}
          <div
            class="pointer-events-none absolute z-20 max-w-xs -translate-y-full truncate rounded bg-black/80 px-2 py-1 font-mono text-xs text-foreground shadow"
            style="left: {hoverTip.x}px; top: {hoverTip.y}px"
          >
            {hoverTip.path}
          </div>
        {/if}
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

  /* The whole-page state glow. Color comes from --accent; each variant differs in
     intensity and motion. The breathe/alert variants carry a static base glow so
     reduced-motion users still get the full-strength tint. */
  .state-frame {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 5;
    transition: box-shadow 600ms ease;
  }
  .state-frame-soft {
    box-shadow: inset 0 0 100px 4px color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .state-frame-steady {
    box-shadow: inset 0 0 130px 8px color-mix(in srgb, var(--accent) 26%, transparent);
  }
  .state-frame-breathe {
    box-shadow: inset 0 0 130px 8px color-mix(in srgb, var(--accent) 22%, transparent);
    animation: frame-breathe 4s ease-in-out infinite;
  }
  .state-frame-alert {
    box-shadow: inset 0 0 150px 10px color-mix(in srgb, var(--accent) 40%, transparent);
    animation: frame-alert 1.6s ease-in-out infinite;
  }
  @keyframes frame-breathe {
    0%,
    100% {
      box-shadow: inset 0 0 110px 6px color-mix(in srgb, var(--accent) 16%, transparent);
    }
    50% {
      box-shadow: inset 0 0 150px 12px color-mix(in srgb, var(--accent) 32%, transparent);
    }
  }
  @keyframes frame-alert {
    0%,
    100% {
      box-shadow: inset 0 0 120px 6px color-mix(in srgb, var(--accent) 30%, transparent);
    }
    50% {
      box-shadow: inset 0 0 190px 20px color-mix(in srgb, var(--accent) 55%, transparent);
    }
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
    .state-frame-breathe,
    .state-frame-alert,
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
