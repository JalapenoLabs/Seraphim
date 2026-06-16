// Human-readable summary of a Claude `rate_limit_event` payload, shared by the
// task activity log and the watch feed so both describe the subscription usage
// limit the same way and never dump raw JSON.

// Human labels for the rate-limit window and status codes Claude emits.
const RATE_LIMIT_WINDOWS: Record<string, string> = {
  five_hour: '5-hour limit',
  weekly: 'weekly limit'
}
const RATE_LIMIT_STATUSES: Record<string, string> = {
  allowed: 'allowed',
  allowed_warning: 'approaching limit',
  rejected: 'limit reached'
}

function humanize(value: string): string {
  return value.replace(/_/g, ' ')
}

// A reset moment as "3:40 PM (in 2h 14m)", or just the clock time once it's past.
function formatReset(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000)
  const time = date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
  const diffMinutes = Math.round((date.getTime() - Date.now()) / 60000)
  if (diffMinutes <= 0) {
    return time
  }
  const relative =
    diffMinutes < 60 ? `${diffMinutes}m` : `${Math.floor(diffMinutes / 60)}h ${diffMinutes % 60}m`
  return `${time} (in ${relative})`
}

// Turns a rate_limit_event payload into one clean line instead of raw JSON.
export function describeRateLimit(payload: Record<string, unknown>): string {
  const info = payload?.rate_limit_info as Record<string, unknown> | undefined
  if (!info) {
    return 'Rate limit update'
  }
  const window = RATE_LIMIT_WINDOWS[String(info.rateLimitType)] ?? humanize(String(info.rateLimitType ?? 'usage'))
  const status = RATE_LIMIT_STATUSES[String(info.status)] ?? humanize(String(info.status ?? ''))
  let line = `Rate limit · ${window}: ${status}`
  if (typeof info.resetsAt === 'number') {
    line += `, resets ${formatReset(info.resetsAt)}`
  }
  if (info.isUsingOverage) {
    const overage = RATE_LIMIT_STATUSES[String(info.overageStatus)] ?? humanize(String(info.overageStatus ?? ''))
    line += ` · overage ${overage}`
    if (typeof info.overageResetsAt === 'number') {
      line += `, resets ${formatReset(info.overageResetsAt)}`
    }
  }
  return line
}
