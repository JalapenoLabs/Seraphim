<script lang="ts">
  // The three lifetime/total stats: worked time (which ticks up live), cost, and a
  // token breakdown (input over total over output). Pure presentational; the
  // parent owns fetching and the live `workedMs` clock.
  import type { Stats } from '$lib/types'

  import { ArrowUp, ArrowDown } from '@lucide/svelte'

  import Sparkline from '$lib/components/Sparkline.svelte'
  import { cost, durationPrecise, tokens } from './format'

  // `costHistory` / `tokenHistory` are optional per-interval burn series; when
  // given (the Watch kiosk), a sparkline renders under the matching stat. The
  // board/task panels omit them, so they render exactly as before.
  // `className` fully controls the layout of the group. It defaults to a centered
  // flex row (the Watch flanks); the board/task panel passes `contents` so the
  // totals flatten into the panel's own flex and distribute evenly with the gauges.
  let {
    stats,
    workedMs,
    taskId = null,
    class: className = 'flex flex-wrap items-center justify-center gap-x-8 gap-y-4',
    costHistory = null,
    tokenHistory = null
  }: {
    stats: Stats
    workedMs: number
    taskId?: string | null
    class?: string
    costHistory?: number[] | null
    tokenHistory?: number[] | null
  } = $props()
</script>

<div class={className}>
  <!-- Time -->
  <div class="flex flex-col items-center">
    <span class="text-xl font-semibold tabular-nums">{durationPrecise(workedMs)}</span>
    <span class="text-xs text-muted-foreground">{taskId ? 'Time on task' : 'Lifetime'}</span>
  </div>

  <!-- Cost -->
  <div class="flex flex-col items-center">
    <span class="text-xl font-semibold tabular-nums">{cost(stats.cost_usd)}</span>
    <span class="text-xs text-muted-foreground">{taskId ? 'Task cost' : 'Lifetime cost'}</span>
    {#if costHistory}
      <Sparkline values={costHistory} color="var(--success)" tip="Spend per interval over the last 24h" />
    {/if}
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
    {#if tokenHistory}
      <Sparkline values={tokenHistory} color="var(--primary)" tip="Tokens per interval over the last 24h" />
    {/if}
  </div>
</div>
