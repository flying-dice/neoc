/**
 * When a note was edited, as a person reads it. The backend keeps a moment (`2026-06-09T08:41:00.000Z`); which day
 * that is, and how long ago, is the person's own (their clock, their time zone), so it is worked out here, in the window.
 */

const DAY = 24 * 60 * 60 * 1000

/** The local midnight a moment's day starts at. */
const startOf = (date: Date): number => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()

/** How many days before today a moment's day is: 0 for today, 1 for yesterday. Never negative: a clock ahead is today. */
export function daysAgo(edited: string, now: Date): number {
  const then = new Date(edited)
  if (Number.isNaN(then.getTime())) return 0
  return Math.max(0, Math.round((startOf(now) - startOf(then)) / DAY))
}

/** The heading a note is listed under: Today, Yesterday, Previous 7 Days, Previous 30 Days, a month of this year, or a year. */
export function groupOf(edited: string, now: Date): string {
  const days = daysAgo(edited, now)
  if (days === 0) return 'Today'
  if (days === 1) return 'Yesterday'
  if (days <= 7) return 'Previous 7 Days'
  if (days <= 30) return 'Previous 30 Days'
  const then = new Date(edited)
  // one fixed form: the same text for every person, and in every story
  return then.getFullYear() === now.getFullYear() ? then.toLocaleDateString('en-GB', { month: 'long' }) : String(then.getFullYear())
}

const time = (date: Date): string => date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })

/** How an edit reads next to a note in a list: the time today, Yesterday, the weekday this week, or the date (`09/06/2026`). */
export function stamp(edited: string, now: Date): string {
  const then = new Date(edited)
  if (Number.isNaN(then.getTime())) return ''
  const days = daysAgo(edited, now)
  if (days === 0) return time(then)
  if (days === 1) return 'Yesterday'
  if (days < 7) return then.toLocaleDateString('en-GB', { weekday: 'long' })
  return then.toLocaleDateString('en-GB', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

/** How an edit reads over the note itself: `9 June 2026 at 09:41`. */
export function moment(edited: string): string {
  const then = new Date(edited)
  if (Number.isNaN(then.getTime())) return ''
  return `${then.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })} at ${time(then)}`
}
