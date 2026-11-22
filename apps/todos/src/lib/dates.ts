/**
 * Days. A todo is due on a day, not at a moment: the backend keeps it as `2026-06-09`, and which day is "today" is the
 * person's own (their clock, their time zone), so it is worked out here, in the window.
 */

const pad = (n: number): string => String(n).padStart(2, '0')

/** The day `date` falls on where the person is, as the backend keeps it: `2026-06-09`. */
export const dayOf = (date: Date): string => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`

/** The local midnight a day starts at; an invalid date for text that is not a day. */
function startOf(day: string): Date {
  const [, y, m, d] = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day) ?? []
  return y && m && d ? new Date(Number(y), Number(m) - 1, Number(d)) : new Date(NaN)
}

/** The day `days` after `day` (before it, when negative). */
export function shift(day: string, days: number): string {
  const date = startOf(day)
  date.setDate(date.getDate() + days)
  return dayOf(date)
}

/** How a due day reads next to a todo: Today, Tomorrow, Yesterday, or the date (`9 Jun 2026`). Empty for no day. */
export function dueLabel(due: string, today: string): string {
  if (due === '') return ''
  if (due === today) return 'Today'
  if (due === shift(today, 1)) return 'Tomorrow'
  if (due === shift(today, -1)) return 'Yesterday'
  const date = startOf(due)
  if (Number.isNaN(date.getTime())) return due
  // one fixed form: the same text for every person, and in every story
  return date.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
}

/** Whether a due day has passed. Days in this form sort as text. */
export const isOverdue = (due: string, today: string): boolean => due !== '' && due < today
