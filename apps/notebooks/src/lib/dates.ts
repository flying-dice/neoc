/**
 * Days. Today's note is named after the day (`2026-06-09`), and which day is "today" is the person's own (their clock,
 * their time zone), so it is worked out here, in the window.
 */

const pad = (n: number): string => String(n).padStart(2, '0')

/** The day `date` falls on where the person is: `2026-06-09`, the title of that day's note. */
export const dayOf = (date: Date): string => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`

/**
 * When a note last changed, as it reads in a list: the time (`09:41`) for today, Yesterday, or the date (`9 Jun 2026`).
 * `updated` is the backend's moment (`2026-06-09T09:41:00.000Z`); empty for text that is not one.
 */
export function changedLabel(updated: string, now: Date): string {
  const at = new Date(updated)
  if (Number.isNaN(at.getTime())) return ''
  if (dayOf(at) === dayOf(now)) return `${pad(at.getHours())}:${pad(at.getMinutes())}`
  const yesterday = new Date(now)
  yesterday.setDate(now.getDate() - 1)
  if (dayOf(at) === dayOf(yesterday)) return 'Yesterday'
  return at.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
}
