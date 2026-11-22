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
function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => !el.closest('[inert]') && el.getClientRects().length > 0
  )
}

interface TrapOptions {
  /** Called on Escape (focus inside the dialog). */
  onescape?: () => void
}

/**
 * A Svelte attachment: trap Tab inside `node`, focus into it on mount (the `[data-autofocus]` element, else the first
 * focusable, else the dialog itself), restore on destroy.
 */
export function trapFocus(options: TrapOptions = {}): (node: HTMLElement) => () => void {
  const { onescape } = options
  return (node) => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const target = node.querySelector<HTMLElement>('[data-autofocus]') ?? focusables(node)[0] ?? node
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
      if (previous?.isConnected) previous.focus({ preventScroll: true })
    }
  }
}
