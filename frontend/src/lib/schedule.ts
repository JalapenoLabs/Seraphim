// Shared helpers for the availability schedule: weekday labels, time-of-day
// formatting, and a client-side mirror of the backend's "are we in a window
// right now" check so the board can show why the agent is idle.

import type { AvailabilityWindow, Settings } from './types'

// Index 0 = Monday, matching the Rust `weekday` field.
export const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']

// "HH:MM" (24h) from minutes since midnight, for <input type="time"> values.
export function minutesToTime(minutes: number): string {
  const hours = Math.floor(minutes / 60)
  const remainder = minutes % 60
  return `${String(hours).padStart(2, '0')}:${String(remainder).padStart(2, '0')}`
}

// Minutes since midnight from an "HH:MM" value.
export function timeToMinutes(time: string): number {
  const [hours, minutes] = time.split(':').map(Number)
  return hours * 60 + minutes
}

// The operator's local wall clock in a given IANA zone: the ISO date, the
// weekday (0 = Monday), and minutes since midnight. Computed from Intl parts so
// it tracks daylight saving the same way the backend's chrono-tz does.
function localClock(timeZone: string, now: Date) {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false
  }).formatToParts(now)

  const lookup = (type: string) => parts.find((part) => part.type === type)?.value ?? '00'
  const year = Number(lookup('year'))
  const month = Number(lookup('month'))
  const day = Number(lookup('day'))
  // Some engines emit "24" for midnight under hour12:false; normalize it.
  const hour = Number(lookup('hour')) % 24
  const minute = Number(lookup('minute'))

  const isoDate = `${lookup('year')}-${lookup('month')}-${lookup('day')}`
  // getUTCDay is 0 = Sunday; shift so 0 = Monday to match the backend.
  const weekday = (new Date(Date.UTC(year, month - 1, day)).getUTCDay() + 6) % 7

  return { isoDate, weekday, minuteOfDay: hour * 60 + minute }
}

function inWindow(window: AvailabilityWindow, weekday: number, minuteOfDay: number): boolean {
  return (
    window.weekday === weekday &&
    minuteOfDay >= window.start_minute &&
    minuteOfDay < window.end_minute
  )
}

// Whether the agent would pick up work right now under these settings. Mirrors
// `availability::is_available` in the API: disabled means always; a skipped date
// blocks the day; empty windows mean any time of day.
export function isWithinSchedule(settings: Settings, now: Date): boolean {
  if (!settings.availability_enabled) {
    return true
  }

  let clock: ReturnType<typeof localClock>
  try {
    clock = localClock(settings.availability_timezone, now)
  } catch (error) {
    // An invalid zone fails open here too, matching the backend.
    console.debug('isWithinSchedule got an invalid time zone, treating as available', error)
    return true
  }

  if (settings.availability_skip_dates.includes(clock.isoDate)) {
    return false
  }
  if (!settings.availability_windows.length) {
    return true
  }
  return settings.availability_windows.some((window) =>
    inWindow(window, clock.weekday, clock.minuteOfDay)
  )
}
