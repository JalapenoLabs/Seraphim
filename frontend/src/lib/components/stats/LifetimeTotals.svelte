<script lang="ts">
  // The three lifetime/total stats: worked time (which ticks up live), cost, and a
  // token breakdown (input over total over output). Pure presentational; the
  // parent owns fetching and the live `workedMs` clock.
  import type { Stats } from '$lib/types'

  import { ArrowUp, ArrowDown } from '@lucide/svelte'

  import { cost, durationPrecise, tokens } from './format'

  let {
    stats,
    workedMs,
    taskId = null,
    class: className = ''
  }: { stats: Stats; workedMs: number; taskId?: string | null; class?: string } = $props()
</script>

<div class="flex flex-wrap items-center justify-center gap-x-8 gap-y-4 {className}">
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
