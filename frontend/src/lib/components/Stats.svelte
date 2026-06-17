<script lang="ts">
  // Live agent statistics, shown per task (a `taskId` is given) or globally (the
  // board banner). Polls the stats endpoint as a baseline and ticks the time
  // display every second; during a turn it also refetches on the throttled
  // `usage` SSE tick, so the token counter ticks up live mid-generation instead
  // of only at message/turn boundaries.
  import type { Stats } from '$lib/types'

  import { onMount, onDestroy } from 'svelte'
  import { ChevronDown } from '@lucide/svelte'

  import { getComposeStats, getGlobalStats, getTaskStats } from '$lib/api'
  import { subscribeBoardStream } from '$lib/boardStream'
  import { cost, duration, tokens } from '$lib/components/stats/format'
  import UsageGauges from '$lib/components/stats/UsageGauges.svelte'
  import LifetimeTotals from '$lib/components/stats/LifetimeTotals.svelte'

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

</script>

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
    <div class="flex flex-wrap items-center justify-around gap-x-8 gap-y-4 border-t border-border px-4 py-4">
      <UsageGauges {stats} class="contents" />
      <LifetimeTotals {stats} {workedMs} {taskId} class="contents" />
    </div>
  {/if}
</section>
