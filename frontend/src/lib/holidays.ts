// Suggested US federal holidays for the skip-date picker. These are common
// "don't ship today" dates an operator may want to blacklist with one click.
// Dates are returned as ISO "YYYY-MM-DD" strings so they drop straight into a
// Settings.availability_skip_dates list.

export type SuggestedHoliday = {
  name: string
  date: string
}

// Formats a calendar date as "YYYY-MM-DD" without going through a Date, so a
// local time-zone offset can never roll it onto the wrong day. `month` is 1-12.
function isoDate(year: number, month: number, day: number): string {
  const paddedMonth = String(month).padStart(2, '0')
  const paddedDay = String(day).padStart(2, '0')
  return `${year}-${paddedMonth}-${paddedDay}`
}

// The day-of-week (0 = Sunday) of a calendar date. UTC keeps it independent of
// the machine's local zone, which only matters for the weekday math below.
function dayOfWeek(year: number, month: number, day: number): number {
  return new Date(Date.UTC(year, month - 1, day)).getUTCDay()
}

// The nth occurrence (1-based) of `weekday` (0 = Sunday) in a given month.
function nthWeekday(year: number, month: number, weekday: number, occurrence: number): string {
  const firstWeekday = dayOfWeek(year, month, 1)
  const offset = (weekday - firstWeekday + 7) % 7
  const day = 1 + offset + (occurrence - 1) * 7
  return isoDate(year, month, day)
}

// The last occurrence of `weekday` (0 = Sunday) in a given month.
function lastWeekday(year: number, month: number, weekday: number): string {
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate()
  const lastWeekdayOfMonth = dayOfWeek(year, month, daysInMonth)
  const offset = (lastWeekdayOfMonth - weekday + 7) % 7
  return isoDate(year, month, daysInMonth - offset)
}

const MONDAY = 1
const THURSDAY = 4

// US federal holidays observed in `year`, in calendar order.
export function usFederalHolidays(year: number): SuggestedHoliday[] {
  return [
    { name: "New Year's Day", date: isoDate(year, 1, 1) },
    { name: 'Martin Luther King Jr. Day', date: nthWeekday(year, 1, MONDAY, 3) },
    { name: 'Presidents Day', date: nthWeekday(year, 2, MONDAY, 3) },
    { name: 'Memorial Day', date: lastWeekday(year, 5, MONDAY) },
    { name: 'Juneteenth', date: isoDate(year, 6, 19) },
    { name: 'Independence Day', date: isoDate(year, 7, 4) },
    { name: 'Labor Day', date: nthWeekday(year, 9, MONDAY, 1) },
    { name: 'Columbus Day', date: nthWeekday(year, 10, MONDAY, 2) },
    { name: 'Veterans Day', date: isoDate(year, 11, 11) },
    { name: 'Thanksgiving', date: nthWeekday(year, 11, THURSDAY, 4) },
    { name: 'Christmas Day', date: isoDate(year, 12, 25) }
  ]
}
