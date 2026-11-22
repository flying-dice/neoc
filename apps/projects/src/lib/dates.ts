/**
 * When something last changed, as a person reads it. The backend keeps a moment (`2026-06-09T08:41:00.000Z`); which day
 * and hour that is belongs to the person (their time zone), so it is worked out here, in the window.
 */

/** How a moment reads: `9 June 2026 at 09:41`. Empty for text that is not a moment. */
export function moment(at: string): string {
  const then = new Date(at)
  if (Number.isNaN(then.getTime())) return ''
  // one fixed form: the same text for every person, and in every story
  const day = then.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })
  return `${day} at ${then.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}`
}
