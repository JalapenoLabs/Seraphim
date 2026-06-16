// Shared formatting + geometry for the agent statistics widgets (the collapsible
// board panel and the kiosk Watch page). Kept framework-free so every stats
// surface formats numbers, durations, and the usage gauge identically.

// SVG donut geometry (radius 42 in a 0..100 viewBox).
export const GAUGE_RADIUS = 42
export const GAUGE_CIRCUMFERENCE = 2 * Math.PI * GAUGE_RADIUS

export function pctLabel(value: number): string {
  return `${value.toFixed(1)}%`
}

export function tokens(value: number): string {
  return value.toLocaleString()
}

export function cost(value: number): string {
  return `$${value.toFixed(2)}`
}

// "2d 3h", "3h 12m", "12m 4s", "4s" - the two most significant units.
export function duration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const days = Math.floor(total / 86400)
  const hours = Math.floor((total % 86400) / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  if (days > 0) {
    return `${days}d ${hours}h`
  }
  if (hours > 0) {
    return `${hours}h ${minutes}m`
  }
  if (minutes > 0) {
    return `${minutes}m ${seconds}s`
  }
  return `${seconds}s`
}

// Like `duration`, but always carries down to whole seconds so the headline Time
// stat visibly ticks up second-by-second while the agent works, instead of
// freezing at "3h 12m" for a minute at a time (issue #173).
export function durationPrecise(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const days = Math.floor(total / 86400)
  const hours = Math.floor((total % 86400) / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  const parts: string[] = []
  if (days > 0) {
    parts.push(`${days}d`)
  }
  if (days > 0 || hours > 0) {
    parts.push(`${hours}h`)
  }
  if (days > 0 || hours > 0 || minutes > 0) {
    parts.push(`${minutes}m`)
  }
  parts.push(`${seconds}s`)
  return parts.join(' ')
}

export function resetsLabel(unix: number | null): string {
  if (!unix) {
    return ''
  }
  const date = new Date(unix * 1000)
  return `, resets ${date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`
}

// A traffic-light hue for the usage gauge; context stays a calm primary.
export function usageColor(value: number): string {
  if (value >= 90) {
    return 'var(--destructive)'
  }
  if (value >= 75) {
    return 'var(--warning)'
  }
  return 'var(--success)'
}
