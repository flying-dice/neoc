/**
 * Keyboard logic shared by the composite controls (segmented control, menus, focus traps), here and in the app kit
 * (`kit:keys`). Pure functions, so they are tested without a DOM; the components own the events.
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

type KeyEvent = { key: string; ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean }

/** Whether a keydown may be typed text (a printable single character with no command modifier). */
const isPrintable = (e: KeyEvent): boolean =>
  e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && e.key !== ' '

/**
 * The keys a sum or a sentence is typed with: a printable character (not Space), Backspace or Delete, with no command
 * modifier. Enter, Space, Tab, Escape and the arrows press or move; they are not typing, and Cmd+R is not either.
 * Alt is a modifier here too, so Alt+key (option-typed characters on a Mac) is not typing: this is the same rule the input
 * modality uses, and a window never forwards what it would not call typing.
 */
export const isTypingKey = (e: KeyEvent): boolean =>
  isPrintable(e) || ((e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.metaKey && !e.altKey)

/**
 * The index an arrow key moves to in a grid of `count` items laid out in rows of `columns` (roving focus), or null when
 * the key does not move. Left and Right step through the items in reading order (off the end of a row onto the next);
 * Up and Down keep the column and stop at the first and last rows (Down from above a short last row lands on its last
 * item); Home and End go to the ends of the row, and with Ctrl or Cmd to the first and last item.
 */
export function gridIndex(
  count: number,
  columns: number,
  current: number,
  key: string,
  toEnds = false
): number | null {
  if (count <= 0) return null
  const cols = Math.max(1, Math.min(columns, count))
  const at = Math.min(Math.max(current, 0), count - 1)
  const rowStart = at - (at % cols)
  switch (key) {
    case 'ArrowRight':
      return Math.min(at + 1, count - 1)
    case 'ArrowLeft':
      return Math.max(at - 1, 0)
    case 'ArrowDown':
      // a row below exists: the same column there, or its last item when that row is short
      return rowStart + cols < count ? Math.min(at + cols, count - 1) : at
    case 'ArrowUp':
      return at - cols >= 0 ? at - cols : at
    case 'Home':
      return toEnds ? 0 : rowStart
    case 'End':
      return toEnds ? count - 1 : Math.min(rowStart + cols - 1, count - 1)
    default:
      return null
  }
}

/** The keyboard's way to ask for a context menu: the Menu key, or Shift+F10. */
export const isContextMenuKey = (e: KeyEvent & { shiftKey?: boolean }): boolean =>
  e.key === 'ContextMenu' || (e.key === 'F10' && !!e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey)
