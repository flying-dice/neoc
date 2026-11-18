/**
 * The shared half of the presentation nodes (Sheet, Popover, Alert, ConfirmationDialog): whether the node is
 * presented (`presented` bound to a ref, or an `item` binding that presents while it holds a value), how it is
 * dismissed (the binding goes false or null, then the `dismiss` event), and where focus returns afterwards.
 * Focus is remembered by the opener's derived test id, so it survives a re-render that replaced the element.
 */
import type { ViewContext } from '../context'
import type { Scope } from '../core/view'
import type { Node } from '../protocol'
import { focusedTestid, restoreFocusTo } from '../overlay'

export interface PresentedOptions {
  ctx: ViewContext
  node: () => Node
  path: () => string
  scope: () => Scope
  /** The element the presentation lives in (focus returns inside its window). */
  host: () => HTMLElement | null
}

export function presented(o: PresentedOptions) {
  const { view, app } = o.ctx
  const presentedBinding = $derived(view.bindingOf(o.node().props?.presented, o.scope()))
  const itemBinding = $derived(view.bindingOf(o.node().props?.item, o.scope()))
  const open = $derived(
    itemBinding
      ? itemBinding.value !== null && itemBinding.value !== undefined
      : Boolean(presentedBinding ? presentedBinding.value : view.evaluate(o.node().props?.presented, o.scope()))
  )

  let opener: string | null = null
  let wasOpen = false
  $effect.pre(() => {
    if (open && !wasOpen) opener = focusedTestid(app)
    wasOpen = open
  })
  $effect(() => {
    if (open) return undefined
    const id = opener
    opener = null
    if (!id) return undefined
    const frame = requestAnimationFrame(() => restoreFocusTo(app, id, o.host()))
    return () => cancelAnimationFrame(frame)
  })

  return {
    get open() {
      return open
    },
    /** Close it from inside (Escape, scrim, a button, an action): bound state first, then the `dismiss` event. */
    dismiss(): void {
      if (!open) return
      if (itemBinding) itemBinding.set(null)
      else presentedBinding?.set(false)
      view.fire(o.node(), o.path(), 'dismiss', o.scope())
    }
  }
}
