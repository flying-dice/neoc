/** One drag controller per window (view instance): drop targets and the drag in progress are shared by its nodes. */
import { DragController } from '../core/dnd'
import type { ViewCore } from '../core/view'

const controllers = new WeakMap<ViewCore, DragController>()

export function dragControllerOf(view: ViewCore): DragController {
  let c = controllers.get(view)
  if (!c) controllers.set(view, (c = new DragController()))
  return c
}
