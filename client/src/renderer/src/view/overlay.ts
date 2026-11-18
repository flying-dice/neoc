/**
 * Presentation nodes (Sheet, Popover, Alert, ConfirmationDialog, Menu, context menus) are portal nodes: they render
 * nothing in flow and mount in the window's overlay layer, which is the window root (`<app>-view`, the nearest
 * positioned ancestor of every kit overlay). These helpers are Svelte attachments and lookups the adapters share:
 *
 *  - `portalToWindow`  moves the adapter's wrapper to the window root and reports where it came from (the default
 *                      anchor of a popover is the node it was written inside);
 *  - `modalWhile`      makes the window's content `inert` while a modal presentation is open (focus and assistive
 *                      technology stay in the overlay; toasts and the live region stay reachable);
 *  - `restoreFocusTo`  after a presentation closes, focus returns to the node that opened it, found again by its
 *                      derived test id, so it survives a re-render that replaced the element;
 *  - `anchorElement`   the element a popover points at: a node by key, else the nearest box above the origin.
 */
import { testid } from './core/testid'
import { findByKey } from './core/tree'
import type { ViewContext } from './context'

const rootOf = (el: Element, app: string): HTMLElement | null =>
  el.closest<HTMLElement>(`[data-testid="${CSS.escape(`${app}-view`)}"]`)

/** Attachment: move the node into the window root; `onorigin` gets the element it was rendered inside. */
export function portalToWindow(
  app: string,
  onorigin?: (origin: HTMLElement | null) => void
): (node: HTMLElement) => () => void {
  return (node) => {
    const origin = node.parentElement
    onorigin?.(origin)
    const root = origin ? rootOf(origin, app) : null
    if (root && node.parentElement !== root) root.appendChild(node)
    return () => node.remove()
  }
}

/** How many modal presentations are open per window content element (nested sheets and alerts stack). */
const holds = new WeakMap<Element, number>()

/** The window content: `<app>-tree`, a sibling of the overlay layer. */
function contentOf(node: Element, app: string): HTMLElement | null {
  const root = rootOf(node, app)
  return root?.querySelector<HTMLElement>(`[data-testid="${CSS.escape(`${app}-tree`)}"]`) ?? null
}

/** Attachment: while the node is mounted, the window content is `inert`. Mount it only while presented. */
export function modalWhile(app: string): (node: HTMLElement) => () => void {
  return (node) => {
    const content = contentOf(node, app)
    if (!content) return () => undefined
    holds.set(content, (holds.get(content) ?? 0) + 1)
    content.inert = true
    return () => {
      const n = (holds.get(content) ?? 1) - 1
      holds.set(content, n)
      if (n <= 0) content.inert = false
    }
  }
}

/** The derived test id of the focused element inside the window, to find the same node again later. */
export function focusedTestid(app: string): string | null {
  const active = document.activeElement
  if (!(active instanceof HTMLElement)) return null
  const id = active.closest<HTMLElement>('[data-testid]')?.dataset.testid
  return id && id.startsWith(`${app}-`) ? id : null
}

/** Put focus back on the node with this test id, unless focus already landed on something real. */
export function restoreFocusTo(app: string, id: string | null, within: Element | null): void {
  if (!id || !within) return
  const root = rootOf(within, app)
  const active = document.activeElement
  if (active && active !== document.body && root?.contains(active) && !active.closest('[inert]')) return
  root?.querySelector<HTMLElement>(`[data-testid="${CSS.escape(id)}"]`)?.focus({ preventScroll: true })
}

/** The first ancestor (including `from`) that has a layout box: `display: contents` wrappers have none. */
export function boxAncestor(from: HTMLElement | null): HTMLElement | null {
  let el = from
  while (el && el.getClientRects().length === 0) el = el.parentElement
  return el
}

/** The element a popover points at: the node with key `anchor`, else the box around where it was written. */
export function anchorElement(
  ctx: ViewContext,
  anchorKey: string | undefined,
  origin: HTMLElement | null
): HTMLElement | null {
  if (anchorKey && ctx.view.store.tree) {
    const hit = findByKey(ctx.view.store.tree, anchorKey)
    const root = origin ? rootOf(origin, ctx.app) : null
    const id = hit ? testid(ctx.app, hit.path) : null
    const el = id ? root?.querySelector<HTMLElement>(`[data-testid="${CSS.escape(id)}"]`) : null
    if (el) return boxAncestor(el)
  }
  return boxAncestor(origin)
}
