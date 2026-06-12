<script lang="ts">
  // Live agent statistics, shown per task (a `taskId` is given) or globally (the
  // board banner). Polls the stats endpoint as a baseline and ticks the time
  // display every second; during a turn it also refetches on the throttled
  // `usage` SSE tick, so the token counter ticks up live mid-generation instead
  // of only at message/turn boundaries.
  import type { Stats } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { ChevronDown, ArrowUp, ArrowDown } from '@lucide/svelte'

  import { getGlobalStats, getTaskStats } from '$lib/api'

  let { taskId = null }: { taskId?: string | null } = $props()

  let stats = $state<Stats | null>(null)
  let open = $state(true)
  let now = $state(Date.now())
  let poll: ReturnType<typeof setInterval> | null = null
  let ticker: ReturnType<typeof setInterval> | null = null
  let usageStream: EventSource | null = null

  async function refresh() {
    try {
      stats = taskId ? await getTaskStats(taskId) : await getGlobalStats()
    } catch (error) {
      console.debug('failed to load stats', error)
    }
  }

  onMount(() => {
    refresh()
    // A slow baseline poll reconciles with the persisted totals; the live ticking
    // comes from the SSE `usage` nudges below.
    poll = setInterval(refresh, 5000)
    ticker = setInterval(() => (now = Date.now()), 1000)

    // The board stream carries global usage ticks; a task's stream carries its
    // own. The server already throttles these, so refetching on each is smooth.
    const streamUrl = taskId ? `/api/v1/tasks/${taskId}/stream` : '/api/v1/board/stream'
    usageStream = new EventSource(streamUrl)
    usageStream.addEventListener('usage', () => refresh())
  })

  onDestroy(() => {
    if (poll) clearInterval(poll)
    if (ticker) clearInterval(ticker)
    usageStream?.close()
  })

  // Worked time counts up live: the persisted total plus the elapsed time of any
  // turn currently in progress.
  const workedMs = $derived.by(() => {
    if (!stats) return 0
    let ms = stats.worked_ms
    if (stats.running_since) {
      ms += Math.max(0, now - new Date(stats.running_since).getTime())
    }
    return ms
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
        <span class="text-xl font-semibold tabular-nums">{duration(workedMs)}</span>
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
