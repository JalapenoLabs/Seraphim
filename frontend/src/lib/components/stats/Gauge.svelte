<script lang="ts">
  // One donut gauge: a percentage ring with the value centered and a caption
  // below. Shared by every statistics surface so the gauges look identical
  // everywhere. `size` lets the kiosk Watch page show larger gauges than the
  // compact board panel.
  import { GAUGE_RADIUS, GAUGE_CIRCUMFERENCE, pctLabel } from './format'

  let {
    pct,
    label,
    color,
    tip,
    size = 'size-20'
  }: { pct: number; label: string; color: string; tip: string; size?: string } = $props()

  const clamped = $derived(Math.min(100, Math.max(0, pct)))
</script>

<div class="flex flex-col items-center gap-1.5" title={tip}>
  <div class="relative {size}">
    <svg viewBox="0 0 100 100" class="size-full">
      <circle cx="50" cy="50" r={GAUGE_RADIUS} fill="none" stroke="var(--border)" stroke-width="9" />
      <circle
        cx="50"
        cy="50"
        r={GAUGE_RADIUS}
        fill="none"
        stroke={color}
        stroke-width="9"
        stroke-linecap="round"
        transform="rotate(-90 50 50)"
        stroke-dasharray={GAUGE_CIRCUMFERENCE}
        stroke-dashoffset={GAUGE_CIRCUMFERENCE * (1 - clamped / 100)}
      />
    </svg>
    <span class="absolute inset-0 flex items-center justify-center text-sm font-semibold tabular-nums">
      {pctLabel(pct)}
    </span>
  </div>
  <span class="text-xs text-muted-foreground">{label}</span>
</div>
