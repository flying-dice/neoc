/**
 * Keyboard handling for list-like widgets with one tab stop (roving tabindex): the list, the tab list and the
 * sidebar. Pure: a key event goes in, what to do comes out, so it is testable without a DOM.
 */

export interface KeyLike {
  key: string
  altKey?: boolean
  ctrlKey?: boolean
  metaKey?: boolean
  shiftKey?: boolean
}

export type ListAction =
  | { type: 'focus'; index: number }
  | { type: 'reorder'; delta: -1 | 1 }
  | { type: 'swipe'; side: 'leading' | 'trailing' | 'close' }
  | { type: 'typeahead'; char: string }

export interface ListKeyContext {
  /** The focused row's position among the rendered rows, and how many are rendered. */
  index: number
  count: number
  /** Rows per Page Up / Page Down. */
  page: number
  canReorder: boolean
  /** Which swipe sides the focused row has, and whether one is open. */
  swipe?: { leading: boolean; trailing: boolean; open: boolean }
  /** Right-to-left layouts swap the swipe arrows. */
  rtl?: boolean
}

const clamp = (n: number, count: number): number => Math.min(Math.max(0, n), Math.max(0, count - 1))

/** The action for a key press in a list, or null when the key is not the list's (let it through). */
export function listKeyAction(e: KeyLike, c: ListKeyContext): ListAction | null {
  const mod = e.ctrlKey || e.metaKey
  if (e.altKey && !mod) {
    if (c.canReorder && (e.key === 'ArrowUp' || e.key === 'ArrowDown'))
      return { type: 'reorder', delta: e.key === 'ArrowUp' ? -1 : 1 }
    if (c.swipe && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      // Left reveals the trailing actions, as a swipe left does (mirrored in RTL).
      const side = (e.key === 'ArrowLeft') !== !!c.rtl ? 'trailing' : 'leading'
      if (c.swipe.open) return { type: 'swipe', side: 'close' }
      return c.swipe[side] ? { type: 'swipe', side } : null
    }
    return null
  }
  if (mod) {
    if (e.key === 'Home') return { type: 'focus', index: 0 }
    if (e.key === 'End') return { type: 'focus', index: clamp(Infinity, c.count) }
    return null
  }
  if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
    if (c.swipe?.open) return { type: 'swipe', side: 'close' }
    const side = c.swipe?.trailing ? 'trailing' : c.swipe?.leading ? 'leading' : null
    return side ? { type: 'swipe', side } : null
  }
  switch (e.key) {
    case 'ArrowDown':
      return { type: 'focus', index: clamp(c.index + 1, c.count) }
    case 'ArrowUp':
      return { type: 'focus', index: clamp(c.index - 1, c.count) }
    case 'Home':
      return { type: 'focus', index: 0 }
    case 'End':
      return { type: 'focus', index: clamp(Infinity, c.count) }
    case 'PageDown':
      return { type: 'focus', index: clamp(c.index + Math.max(1, c.page), c.count) }
    case 'PageUp':
      return { type: 'focus', index: clamp(c.index - Math.max(1, c.page), c.count) }
    case 'Escape':
      return c.swipe?.open ? { type: 'swipe', side: 'close' } : null
  }
  if (e.key.length === 1 && e.key !== ' ' && !e.altKey) return { type: 'typeahead', char: e.key }
  return null
}

/**
 * Type-to-select: the index of the next label starting with `query` after `from` (wrapping), or -1. A repeated
 * single letter cycles through the matches, like a native listbox.
 */
export function typeaheadMatch(labels: readonly string[], query: string, from: number): number {
  const q = query.toLowerCase()
  if (!q || labels.length === 0) return -1
  const cycle = [...q].every((ch) => ch === q[0])
  const needle = cycle ? q[0] : q
  for (let step = 1; step <= labels.length; step++) {
    const i = (from + (cycle ? step : step - 1) + labels.length) % labels.length
    if (labels[i].trim().toLowerCase().startsWith(needle)) return i
  }
  return -1
}

/** Accumulates typed characters for a short window, then forgets them. */
export class TypeaheadBuffer {
  private text = ''
  private at = 0
  constructor(private readonly windowMs = 700) {}

  push(char: string, now: number): string {
    if (now - this.at > this.windowMs) this.text = ''
    this.at = now
    this.text += char
    return this.text
  }
}

/** Arrow keys for a horizontal tab list: next / previous wrap, Home / End jump. Returns the new index or null. */
export function tabKeyIndex(key: string, index: number, count: number, rtl = false): number | null {
  if (count === 0) return null
  const forward = rtl ? 'ArrowLeft' : 'ArrowRight'
  const back = rtl ? 'ArrowRight' : 'ArrowLeft'
  if (key === forward) return (index + 1) % count
  if (key === back) return (index - 1 + count) % count
  if (key === 'Home') return 0
  if (key === 'End') return count - 1
  return null
}
