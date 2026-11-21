/**
 * Reordering by keys, never by index: the keyboard path (`Alt+Arrow`), the
 * drag path (insertion index from row rectangles) and the optimistic order a list shows while the host settles.
 */

export type Key = string | number

export interface Move<K extends Key = Key> {
  key: K
  /** The row the moved row now sits in front of; null = the end of the list. */
  beforeKey: K | null
  /** The new order. */
  keys: K[]
  from: number
  to: number
}

/** Apply "put `key` in front of `beforeKey`" (null = last). Returns null if nothing would change. */
export function moveBefore<K extends Key>(
  keys: readonly K[],
  key: K,
  beforeKey: K | null
): Move<K> | null {
  const from = keys.indexOf(key)
  if (from < 0 || beforeKey === key) return null
  const rest = keys.filter((k) => k !== key)
  const at = beforeKey === null ? rest.length : rest.indexOf(beforeKey)
  if (at < 0) return null
  const next = [...rest.slice(0, at), key, ...rest.slice(at)]
  if (next[from] === key) return null // dropped where it was
  return { key, beforeKey, keys: next, from, to: at }
}

/** Move one step with the keyboard (`Alt+ArrowUp` = -1, `Alt+ArrowDown` = +1). Null at either end. */
export function moveBy<K extends Key>(keys: readonly K[], key: K, delta: -1 | 1): Move<K> | null {
  const from = keys.indexOf(key)
  const to = from + delta
  if (from < 0 || to < 0 || to >= keys.length) return null
  // Moving down puts the row in front of the one two places ahead (or last); moving up, in front of the previous.
  const beforeKey = delta < 0 ? keys[to] : (keys[to + 1] ?? null)
  return moveBefore(keys, key, beforeKey)
}

export interface Rect {
  top: number
  bottom: number
}

/**
 * The index a dragged row would be inserted at, for pointer position `y` over the other rows' rectangles
 * (in order, the dragged row excluded): in front of the first row whose midpoint is below the pointer.
 */
export function insertionIndex(rects: readonly Rect[], y: number): number {
  for (let i = 0; i < rects.length; i++) if (y < (rects[i].top + rects[i].bottom) / 2) return i
  return rects.length
}

/** `keys` without `key`, then `key` at `index`: the order the list shows during a drag. */
export function withInserted<K extends Key>(keys: readonly K[], key: K, index: number): K[] {
  const rest = keys.filter((k) => k !== key)
  const at = Math.min(Math.max(0, index), rest.length)
  return [...rest.slice(0, at), key, ...rest.slice(at)]
}

/** What a screen reader hears after a move ("Note 3 moved to position 2 of 10"). */
export const moveAnnouncement = (title: string, to: number, total: number): string =>
  `${title} moved to position ${to + 1} of ${total}`
