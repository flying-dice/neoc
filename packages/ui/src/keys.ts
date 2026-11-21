/**
 * Keyboard logic shared by the kit's composite controls (segmented control, menus, option lists, calendars,
 * focus traps). Pure functions over indexes, so they are tested without a DOM; the components own the events.
 */

export type Orientation = 'vertical' | 'horizontal' | 'both'

/**
 * The index an arrow key moves to in a list of `count` items (roving focus), or null when the key is not a
 * movement key for this orientation. `wrap` loops past either end; Home and End jump to the ends.
 */
export function moveIndex(
  count: number,
  current: number,
  key: string,
  orientation: Orientation = 'vertical',
  wrap = true
): number | null {
  if (count <= 0) return null
  const vertical = orientation !== 'horizontal'
  const horizontal = orientation !== 'vertical'
  let delta = 0
  if ((key === 'ArrowDown' && vertical) || (key === 'ArrowRight' && horizontal)) delta = 1
  else if ((key === 'ArrowUp' && vertical) || (key === 'ArrowLeft' && horizontal)) delta = -1
  else if (key === 'Home') return 0
  else if (key === 'End') return count - 1
  else return null
  const next = current + delta
  if (next < 0) return wrap ? count - 1 : 0
  if (next >= count) return wrap ? 0 : count - 1
  return next
}

/** The next enabled index from `from` in `direction`, skipping disabled items; `from` itself when none is enabled. */
export function nextEnabled(disabled: readonly boolean[], from: number, direction: 1 | -1): number {
  const n = disabled.length
  for (let step = 1; step <= n; step++) {
    const i = (((from + direction * step) % n) + n) % n
    if (!disabled[i]) return i
  }
  return from
}

/** Where focus goes on Tab inside a trap of `count` focusables: wraps at both ends. */
export function trapIndex(count: number, current: number, shift: boolean): number {
  if (count <= 0) return -1
  if (current < 0) return shift ? count - 1 : 0
  return shift ? (current - 1 + count) % count : (current + 1) % count
}

/** Type-to-select: the next label starting with `buffer` after `from`, case-insensitive, wrapping; -1 for none. */
export function typeAhead(labels: readonly string[], buffer: string, from: number): number {
  const q = buffer.toLowerCase()
  if (!q) return -1
  const n = labels.length
  for (let step = 1; step <= n; step++) {
    const i = (((from + step) % n) + n) % n
    if (labels[i]!.toLowerCase().startsWith(q)) return i
  }
  return -1
}

/** True for the keyboard path to a context menu: the Menu key, or Shift+F10. */
export const isContextMenuKey = (e: { key: string; shiftKey: boolean }): boolean =>
  e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)

/** True for the keys that activate a control: Enter or Space. */
export const isActivateKey = (key: string): boolean => key === 'Enter' || key === ' '

type KeyEvent = { key: string; ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean }

/** Whether a keydown may be typed text (a printable single character with no command modifier). */
export const isPrintable = (e: KeyEvent): boolean =>
  e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && e.key !== ' '

/**
 * The keys a sum or a sentence is typed with: a printable character (not Space), Backspace or Delete, with no command
 * modifier. Enter, Space, Tab, Escape and the arrows press or move; they are not typing, and Cmd+R is not either.
 * Alt is a modifier here too, so Alt+key (option-typed characters on a Mac) is not typing: this is the same rule the input
 * modality uses, and a window never forwards what it would not call typing.
 */
export const isTypingKey = (e: KeyEvent): boolean =>
  isPrintable(e) || ((e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.metaKey && !e.altKey)
