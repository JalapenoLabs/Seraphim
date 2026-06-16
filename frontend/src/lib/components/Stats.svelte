<script lang="ts">
  // Live agent statistics, shown per task (a `taskId` is given) or globally (the
  // board banner). Polls the stats endpoint as a baseline and ticks the time
  // display every second; during a turn it also refetches on the throttled
  // `usage` SSE tick, so the token counter ticks up live mid-generation instead
  // of only at message/turn boundaries.
  import type { Stats } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { ChevronDown, ArrowUp, ArrowDown } from '@lucide/svelte'

  import { getComposeStats, getGlobalStats, getTaskStats } from '$lib/api'
  import { subscribeBoardStream } from '$lib/boardStream'

  // Scope: a task's stats (`taskId`), the compose assistant's (`compose`), or the
  // global board totals (neither set).
  let { taskId = null, compose = false }: { taskId?: string | null; compose?: boolean } = $props()

  let stats = $state<Stats | null>(null)
  // When the current `stats` was fetched. The server's `worked_ms` already counts
  // running turns up to that instant, so the live tick only adds the time elapsed
  // since, scaled by how many turns are running.
  let fetchedAt = $state(Date.now())
  let open = $state(true)
  let now = $state(Date.now())
  let poll: ReturnType<typeof setInterval> | null = null
  let ticker: ReturnType<typeof setInterval> | null = null
  // The task and compose scopes each open their own dedicated stream (those are
  // per-scope endpoints, not the board stream); the board scope instead shares the
  // single board-stream subscription so the board page holds one connection total.
  let usageStream: EventSource | null = null
  let unsubscribe: (() => void) | null = null

  async function refresh() {
    try {
      let next: Stats
      if (compose) {
        next = await getComposeStats()
      } else if (taskId) {
        next = await getTaskStats(taskId)
      } else {
        next = await getGlobalStats()
      }
      stats = next
      fetchedAt = Date.now()
    } catch (error) {
      console.debug('failed to load stats', error)
    }
  }

  onMount(() => {
    refresh()
    // A slow baseline poll reconciles with the persisted totals; the live ticking
    // comes from the SSE nudges below.
    poll = setInterval(refresh, 5000)
    ticker = setInterval(() => (now = Date.now()), 1000)

    // Each scope refetches off the right stream: the compose stream nudges on
    // `compose_changed`, the task stream on its `usage` tick, and the global board
    // scope on the shared board-stream `usage` tick (one connection for the page).
    if (compose) {
      usageStream = new EventSource('/api/v1/compose/stream')
      usageStream.addEventListener('compose_changed', () => refresh())
    } else if (taskId) {
      usageStream = new EventSource(`/api/v1/tasks/${taskId}/stream`)
      usageStream.addEventListener('usage', () => refresh())
    } else {
      unsubscribe = subscribeBoardStream({ usage: refresh })
    }
  })

  onDestroy(() => {
    if (poll) clearInterval(poll)
    if (ticker) clearInterval(ticker)
    usageStream?.close()
    unsubscribe?.()
  })

  // Worked time counts up live: the server's `worked_ms` already includes each
  // running turn's elapsed time at fetch, so we add only the time since the fetch,
  // multiplied by the number of running turns. Parallel railway lanes therefore
  // advance the clock at the correct combined rate.
  const workedMs = $derived.by(() => {
    if (!stats) return 0
    const sinceFetch = Math.max(0, now - fetchedAt)
    return stats.worked_ms + stats.running_turns * sinceFetch
  })

  const contextPct = $derived(
    stats && stats.context_window > 0
      ? Math.min(100, Math.max(0, (stats.context_tokens / stats.context_window) * 100))
      : 0
  )
  const usagePct = $derived(stats?.usage_utilization ?? 0)
  const sevenDayPct = $derived(stats?.usage_seven_day_utilization ?? 0)
  // When the headless stream reports no numeric utilization, fall back to its
  // categorical status (e.g. "allowed") rather than a misleading 0%.
  const usageStatusLabel = $derived(stats?.usage_status?.replace(/_/g, ' ') ?? 'Unknown')

  // SVG donut geometry (radius 42 in a 0..100 viewBox).
  const RADIUS = 42
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS

  function pctLabel(value: number): string {
    return `${value.toFixed(1)}%`
  }

  function tokens(value: number): string {
    return value.toLocaleString()
  }

  function cost(value: number): string {
    return `$${value.toFixed(2)}`
  }

  // "2d 3h", "3h 12m", "12m 4s", "4s" - the two most significant units.
  function duration(ms: number): string {
    const total = Math.max(0, Math.floor(ms / 1000))
    const days = Math.floor(total / 86400)
    const hours = Math.floor((total % 86400) / 3600)
    const minutes = Math.floor((total % 3600) / 60)
    const seconds = total % 60
    if (days > 0) return `${days}d ${hours}h`
    if (hours > 0) return `${hours}h ${minutes}m`
    if (minutes > 0) return `${minutes}m ${seconds}s`
    return `${seconds}s`
  }

  // Like `duration`, but always carries down to whole seconds so the headline
  // Time stat visibly ticks up second-by-second while the agent works, instead
  // of freezing at "3h 12m" for a minute at a time (issue #173).
  function durationPrecise(ms: number): string {
    const total = Math.max(0, Math.floor(ms / 1000))
    const days = Math.floor(total / 86400)
    const hours = Math.floor((total % 86400) / 3600)
    const minutes = Math.floor((total % 3600) / 60)
    const seconds = total % 60
    const parts: string[] = []
    if (days > 0) parts.push(`${days}d`)
    if (days > 0 || hours > 0) parts.push(`${hours}h`)
    if (days > 0 || hours > 0 || minutes > 0) parts.push(`${minutes}m`)
    parts.push(`${seconds}s`)
    return parts.join(' ')
  }

  function resetsLabel(unix: number | null): string {
    if (!unix) return ''
    const date = new Date(unix * 1000)
    return `, resets ${date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`
  }

  // A traffic-light hue for the usage gauge; context stays a calm primary.
  function usageColor(value: number): string {
    if (value >= 90) return 'var(--destructive)'
    if (value >= 75) return 'var(--warning)'
    return 'var(--success)'
  }
</script>

{#snippet gauge(pct: number, label: string, color: string, tip: string)}
  <div class="flex flex-col items-center gap-1.5" title={tip}>
    <div class="relative size-20">
      <svg viewBox="0 0 100 100" class="size-full">
        <circle cx="50" cy="50" r={RADIUS} fill="none" stroke="var(--border)" stroke-width="9" />
        <circle
          cx="50"
          cy="50"
          r={RADIUS}
          fill="none"
          stroke={color}
          stroke-width="9"
          stroke-linecap="round"
          transform="rotate(-90 50 50)"
          stroke-dasharray={CIRCUMFERENCE}
          stroke-dashoffset={CIRCUMFERENCE * (1 - Math.min(100, Math.max(0, pct)) / 100)}
        />
      </svg>
      <span class="absolute inset-0 flex items-center justify-center text-sm font-semibold tabular-nums">
        {pctLabel(pct)}
      </span>
    </div>
    <span class="text-xs text-muted-foreground">{label}</span>
  </div>
{/snippet}

<section class="rounded-lg border border-border bg-card">
  <button
    type="button"
    onclick={() => (open = !open)}
    class="flex w-full items-center gap-1.5 rounded-lg px-3 py-2 text-left text-sm font-semibold hover:bg-secondary/40"
  >
    <ChevronDown class="size-4 text-muted-foreground transition-transform {open ? '' : '-rotate-90'}" />
    Statistics
    {#if stats && !open}
      <span class="ml-2 font-normal text-muted-foreground">
        {cost(stats.cost_usd)} · {duration(workedMs)} · {tokens(stats.total_tokens)} tokens
      </span>
    {/if}
  </button>

  {#if open && stats}
    <div
      class="flex flex-wrap items-center justify-around gap-x-8 gap-y-4 border-t border-border px-4 py-4"
    >
      {#if stats.usage_utilization != null}
        {@render gauge(
          usagePct,
          stats.usage_seven_day_utilization != null ? '5-hour limit' : 'Usage limit',
          usageColor(usagePct),
          `Subscription 5-hour usage limit: ${pctLabel(usagePct)} used${resetsLabel(stats.usage_resets_at)}. This is the whole subscription (all terminals), not just Seraphim.`
        )}
      {:else}
        <!-- The headless agent stream reports a status, not a percentage. -->
        <div
          class="flex flex-col items-center gap-1.5"
          title={`Subscription rate-limit status from the agent stream${resetsLabel(stats.usage_resets_at)}. The headless stream reports a status, not a percentage.`}
        >
          <div class="flex size-20 items-center justify-center rounded-full border-[9px] border-border">
            <span class="px-1 text-center text-xs font-semibold capitalize leading-tight">
              {usageStatusLabel}
            </span>
          </div>
          <span class="text-xs text-muted-foreground">Usage limit</span>
        </div>
      {/if}

      {#if stats.usage_seven_day_utilization != null}
        {@render gauge(
          sevenDayPct,
          'Weekly limit',
          usageColor(sevenDayPct),
          `Subscription 7-day usage limit: ${pctLabel(sevenDayPct)} used${resetsLabel(stats.usage_seven_day_resets_at)}. This is the whole subscription (all terminals), not just Seraphim.`
        )}
      {/if}

      {@render gauge(
        contextPct,
        'Context',
        'var(--primary)',
        `Context window: ${tokens(stats.context_tokens)} / ${tokens(stats.context_window)} tokens (${pctLabel(contextPct)}) used before auto-compaction.`
      )}

      <!-- Time -->
      <div class="flex flex-col items-center">
        <span class="text-xl font-semibold tabular-nums">{durationPrecise(workedMs)}</span>
        <span class="text-xs text-muted-foreground">{taskId ? 'Time on task' : 'Lifetime'}</span>
      </div>

      <!-- Cost -->
      <div class="flex flex-col items-center">
        <span class="text-xl font-semibold tabular-nums">{cost(stats.cost_usd)}</span>
        <span class="text-xs text-muted-foreground">{taskId ? 'Task cost' : 'Lifetime cost'}</span>
      </div>

      <!-- Tokens: input over total over output, with thin dividers. -->
      <div class="flex flex-col items-center leading-tight" title="Tokens (input / total / output)">
        <span class="flex items-center gap-1 text-sm tabular-nums text-muted-foreground/70">
          <ArrowUp class="size-3" />{tokens(stats.input_tokens)}
        </span>
        <hr class="my-1 w-10 border-border" />
        <span class="text-lg font-semibold tabular-nums">{tokens(stats.total_tokens)}</span>
        <hr class="my-1 w-10 border-border" />
        <span class="flex items-center gap-1 text-sm tabular-nums text-muted-foreground/70">
          <ArrowDown class="size-3" />{tokens(stats.output_tokens)}
        </span>
      </div>
    </div>
  {/if}
</section>
