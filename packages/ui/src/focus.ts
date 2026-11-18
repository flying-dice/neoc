/**
 * Focus management for overlays, as Svelte attachments: `{@attach trapFocus()}` on a dialog moves focus into it,
 * keeps Tab inside it, and puts focus back where it came from when the dialog goes away.
 */
import { trapIndex } from './keys'

const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])'
].join(',')

/** The tabbable elements inside `root`, in DOM order (hidden ones excluded). */
export function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => !el.closest('[inert]') && el.getClientRects().length > 0
  )
}

export interface TrapOptions {
  /** Where focus starts: a selector inside the dialog (default `[data-autofocus]`, then the first focusable). */
  initial?: string
  /** Put focus back on the previously focused element when the dialog is removed (default true). */
  restore?: boolean
  /** With no `initial` match: focus the first focusable (default) or the dialog itself. */
  fallback?: 'first' | 'container'
  /** Called on Escape (focus inside the dialog). */
  onescape?: () => void
}

/** A Svelte attachment: trap Tab inside `node`, focus into it on mount, restore on destroy. */
export function trapFocus(options: TrapOptions = {}): (node: HTMLElement) => () => void {
  const { initial = '[data-autofocus]', restore = true, fallback = 'first', onescape } = options
  return (node) => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const target =
      node.querySelector<HTMLElement>(initial) ??
      (fallback === 'first' ? focusables(node)[0] : undefined) ??
      node
    if (target === node && !node.hasAttribute('tabindex')) node.setAttribute('tabindex', '-1')
    // After the transition frame: the element must be laid out to take focus.
    const frame = requestAnimationFrame(() => target.focus({ preventScroll: true }))

    const onkeydown = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && onescape) {
        e.stopPropagation()
        onescape()
        return
      }
      if (e.key !== 'Tab') return
      const items = focusables(node)
      if (items.length === 0) return void e.preventDefault()
      const at = items.indexOf(document.activeElement as HTMLElement)
      const next = trapIndex(items.length, at, e.shiftKey)
      // Only intercept at the ends (or when focus is outside): the browser handles the middle.
      if (at === -1 || (e.shiftKey && at === 0) || (!e.shiftKey && at === items.length - 1)) {
        e.preventDefault()
        items[next]?.focus()
      }
    }
    node.addEventListener('keydown', onkeydown)
    return () => {
      cancelAnimationFrame(frame)
      node.removeEventListener('keydown', onkeydown)
      if (restore && previous?.isConnected) previous.focus({ preventScroll: true })
    }
  }
}
