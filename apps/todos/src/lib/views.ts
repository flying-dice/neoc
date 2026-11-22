/**
 * What the window can show: one of the person's own lists, or one of the five smart lists, which gather todos from
 * every list (Today, Scheduled, All, Flagged, Completed). Which todos a view shows is decided here, from the board.
 */
import type { Color, Todo } from 'app:rpc'

/** The smart lists, in the order the sidebar shows them. `icon` is a glyph of the icon set, `color` a palette role. */
export const SMART = [
  { id: 'today', name: 'Today', icon: 'sun', color: 'blue' },
  { id: 'scheduled', name: 'Scheduled', icon: 'clock', color: 'red' },
  { id: 'all', name: 'All', icon: 'rectangle.stack', color: 'gray' },
  { id: 'flagged', name: 'Flagged', icon: 'flag', color: 'orange' },
  { id: 'completed', name: 'Completed', icon: 'checkmark', color: 'green' }
] as const

export type SmartList = (typeof SMART)[number]
export type SmartId = SmartList['id']
export type View = { readonly kind: 'smart'; readonly id: SmartId } | { readonly kind: 'list'; readonly id: number }

/** The colours a list can have, in the order new lists take them. */
export const COLORS: readonly Color[] = ['blue', 'red', 'orange', 'yellow', 'green', 'purple', 'brown']

export const smart = (id: SmartId): View => ({ kind: 'smart', id })
export const listView = (id: number): View => ({ kind: 'list', id })
export const sameView = (a: View, b: View): boolean => a.kind === b.kind && a.id === b.id

/**
 * The todos a view shows, in the order it shows them. A list and the smart lists show what is still to do; only
 * Completed shows what is done. Today includes what is overdue, and Scheduled is in order of the day.
 */
export function shown(todos: readonly Todo[], view: View, today: string): Todo[] {
  const open = todos.filter((t) => !t.done)
  if (view.kind === 'list') return open.filter((t) => t.list === view.id)
  switch (view.id) {
    case 'today':
      return open.filter((t) => t.due !== '' && t.due <= today)
    case 'scheduled':
      return open.filter((t) => t.due !== '').sort((a, b) => a.due.localeCompare(b.due) || a.id - b.id)
    case 'all':
      return open
    case 'flagged':
      return open.filter((t) => t.flagged)
    case 'completed':
      return todos.filter((t) => t.done)
  }
}
