<script lang="ts">
  // A minimal SVG sparkline: a filled line of a small numeric series, used for the
  // at-a-glance burn-rate trends on the Watch page. Renders nothing until there
  // are at least two points, so it never reserves empty space on a fresh kiosk.
  let {
    values,
    color = 'var(--primary)',
    width = 64,
    height = 18,
    tip = ''
  }: { values: number[]; color?: string; width?: number; height?: number; tip?: string } = $props()

  // Map the series to SVG points, normalizing the value range to the height. A
  // flat series (all equal) rests on the baseline rather than dividing by zero.
  const geometry = $derived.by(() => {
    if (values.length < 2) {
      return null
    }
    const max = Math.max(...values)
    const min = Math.min(...values)
    const span = max - min || 1
    const stepX = width / (values.length - 1)
    const points = values.map((value, index) => {
      const x = index * stepX
      const y = height - ((value - min) / span) * height
      return `${x.toFixed(1)},${y.toFixed(1)}`
    })
    const line = points.join(' ')
    const area = `0,${height} ${line} ${width},${height}`
    return { line, area }
  })
</script>

{#if geometry}
  <svg
    viewBox="0 0 {width} {height}"
    class="mt-1 h-[18px] w-16"
    preserveAspectRatio="none"
    role="img"
    aria-label={tip}
  >
    {#if tip}
      <title>{tip}</title>
    {/if}
    <polygon points={geometry.area} fill={color} opacity="0.16" />
    <polyline
      points={geometry.line}
      fill="none"
      stroke={color}
      stroke-width="1.5"
      stroke-linejoin="round"
      stroke-linecap="round"
      vector-effect="non-scaling-stroke"
    />
  </svg>
{/if}
