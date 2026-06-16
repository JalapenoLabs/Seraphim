<script lang="ts">
  // A compact, single-row stats strip for one railway swimlane: context fill,
  // cost, total tokens, and time worked. Polls the railway stats endpoint as a
  // baseline and refetches on the throttled board `usage` SSE tick so the numbers
  // tick up live during a turn. The shared subscription usage gauge is NOT shown
  // here (it is global, rendered once in the board's top bar); this strip is only
  // the per-railway figures.
  import type { Stats } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'

  import { getRailwayStats } from '$lib/api'

  let { railwayId }: { railwayId: string } = $props()

  let stats = $state<Stats | null>(null)
  let now = $state(Date.now())
  let poll: ReturnType<typeof setInterval> | null = null
  let ticker: ReturnType<typeof setInterval> | null = null
  let usageStream: EventSource | null = null

  async function refresh() {
    try {
      stats = await getRailwayStats(railwayId)
    } catch (error) {
      console.debug('failed to load railway stats', error)
    }
  }

  onMount(() => {
    refresh()
    poll = setInterval(refresh, 5000)
    ticker = setInterval(() => (now = Date.now()), 1000)
    // The board stream nudges `usage` on the throttled mid-turn tick; refetch so
    // the active lane's counters move while the agent generates.
    usageStream = new EventSource('/api/v1/board/stream')
    usageStream.addEventListener('usage', () => refresh())
  })

  onDestroy(() => {
    if (poll) clearInterval(poll)
    if (ticker) clearInterval(ticker)
    usageStream?.close()
  })

  // Worked time counts up live: the persisted total plus any in-progress turn.
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

  function cost(value: number): string {
    return `$${value.toFixed(2)}`
  }

  function tokens(value: number): string {
    return value.toLocaleString()
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
</script>

{#if stats}
  <div class="flex items-center gap-3 text-xs tabular-nums text-muted-foreground">
    <span title="Context window fill for this railway's latest turn">
      <span class="font-semibold text-foreground">{contextPct.toFixed(0)}%</span> ctx
    </span>
    <span class="text-border">·</span>
    <span title="Cost on this railway since the last stats reset">
      <span class="font-semibold text-foreground">{cost(stats.cost_usd)}</span>
    </span>
    <span class="text-border">·</span>
    <span title="Total tokens on this railway">
      <span class="font-semibold text-foreground">{tokens(stats.total_tokens)}</span> tok
    </span>
    <span class="text-border">·</span>
    <span title="Time worked on this railway">
      <span class="font-semibold text-foreground">{duration(workedMs)}</span>
    </span>
  </div>
{/if}
