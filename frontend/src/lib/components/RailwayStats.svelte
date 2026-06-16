<script lang="ts">
  // A compact, single-row stats strip for one railway swimlane: context fill,
  // cost, total tokens, and time worked. Polls the railway stats endpoint as a
  // baseline and refetches on the throttled board `usage` SSE tick so the numbers
  // tick up live during a turn. The board `usage` nudge comes from the single,
  // shared board-stream subscription (so a board with many lanes holds one
  // connection, not one per lane). The shared subscription usage gauge is NOT shown
  // here (it is global, rendered once in the board's top bar); this strip is only
  // the per-railway figures.
  import type { Stats } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'

  import { getRailwayStats } from '$lib/api'
  import { subscribeBoardStream } from '$lib/boardStream'

  let { railwayId }: { railwayId: string } = $props()

  let stats = $state<Stats | null>(null)
  // When the current `stats` was fetched; the live worked-time tick adds only the
  // time elapsed since, scaled by the lane's running-turn count (0 or 1).
  let fetchedAt = $state(Date.now())
  let now = $state(Date.now())
  let poll: ReturnType<typeof setInterval> | null = null
  let ticker: ReturnType<typeof setInterval> | null = null
  let unsubscribe: (() => void) | null = null

  async function refresh() {
    try {
      stats = await getRailwayStats(railwayId)
      fetchedAt = Date.now()
    } catch (error) {
      console.debug('failed to load railway stats', error)
    }
  }

  onMount(() => {
    refresh()
    poll = setInterval(refresh, 5000)
    ticker = setInterval(() => (now = Date.now()), 1000)
    // The board stream nudges `usage` on the throttled mid-turn tick; refetch so
    // the active lane's counters move while the agent generates. One shared
    // connection fans this out to every lane.
    unsubscribe = subscribeBoardStream({ usage: refresh })
  })

  onDestroy(() => {
    if (poll) clearInterval(poll)
    if (ticker) clearInterval(ticker)
    unsubscribe?.()
  })

  // Worked time counts up live: the server's `worked_ms` already includes the
  // running turn's elapsed time at fetch, so we add only the time since the fetch
  // (this lane runs at most one turn, so `running_turns` is 0 or 1).
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
