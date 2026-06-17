<script lang="ts">
  // The three subscription/agent usage gauges: the 5-hour (or generic) limit, the
  // weekly limit, and the live context window. Pure presentational; the parent
  // owns fetching and passes the current `stats`.
  import type { Stats } from '$lib/types'

  import Gauge from './Gauge.svelte'
  import { pctLabel, resetsLabel, tokens, usageColor } from './format'

  // `className` fully controls the layout of the group. It defaults to a centered
  // flex row (the Watch flanks); the board/task panel passes `contents` so the
  // gauges flatten into the panel's own flex and distribute evenly with the totals.
  let {
    stats,
    gaugeSize = 'size-20',
    class: className = 'flex flex-wrap items-center justify-center gap-x-8 gap-y-4'
  }: { stats: Stats; gaugeSize?: string; class?: string } = $props()

  const usagePct = $derived(stats.usage_utilization ?? 0)
  const sevenDayPct = $derived(stats.usage_seven_day_utilization ?? 0)
  const contextPct = $derived(
    stats.context_window > 0
      ? Math.min(100, Math.max(0, (stats.context_tokens / stats.context_window) * 100))
      : 0
  )
  // When the headless stream reports no numeric utilization, fall back to its
  // categorical status (e.g. "allowed") rather than a misleading 0%.
  const usageStatusLabel = $derived(stats.usage_status?.replace(/_/g, ' ') ?? 'Unknown')
</script>

<div class={className}>
  {#if stats.usage_utilization != null}
    <Gauge
      pct={usagePct}
      size={gaugeSize}
      label={stats.usage_seven_day_utilization != null ? '5-hour limit' : 'Usage limit'}
      color={usageColor(usagePct)}
      tip={`Subscription 5-hour usage limit: ${pctLabel(usagePct)} used${resetsLabel(
        stats.usage_resets_at
      )}. This is the whole subscription (all terminals), not just Seraphim.`}
    />
  {:else}
    <!-- The headless agent stream reports a status, not a percentage. -->
    <div
      class="flex flex-col items-center gap-1.5"
      title={`Subscription rate-limit status from the agent stream${resetsLabel(
        stats.usage_resets_at
      )}. The headless stream reports a status, not a percentage.`}
    >
      <div class="flex items-center justify-center rounded-full border-[9px] border-border {gaugeSize}">
        <span class="px-1 text-center text-xs font-semibold capitalize leading-tight">
          {usageStatusLabel}
        </span>
      </div>
      <span class="text-xs text-muted-foreground">Usage limit</span>
    </div>
  {/if}

  {#if stats.usage_seven_day_utilization != null}
    <Gauge
      pct={sevenDayPct}
      size={gaugeSize}
      label="Weekly limit"
      color={usageColor(sevenDayPct)}
      tip={`Subscription 7-day usage limit: ${pctLabel(sevenDayPct)} used${resetsLabel(
        stats.usage_seven_day_resets_at
      )}. This is the whole subscription (all terminals), not just Seraphim.`}
    />
  {/if}

  <Gauge
    pct={contextPct}
    size={gaugeSize}
    label="Context"
    color="var(--primary)"
    tip={`Context window: ${tokens(stats.context_tokens)} / ${tokens(
      stats.context_window
    )} tokens (${pctLabel(contextPct)}) used before auto-compaction.`}
  />
</div>
