/**
 * The `actions` slot of an Alert or ConfirmationDialog: Buttons with a `role` become the kit's `AlertAction`s.
 * `cancel` is the bold one that runs on Escape, `destructive` is red, `primary` is the preferred action (initial
 * focus, Enter from a prompt field). A tap runs the button node's own handler through the view core; for an alert
 * with prompt `fields` the entered values travel in the tap payload as `{ fields }`.
 */
import type { AlertAction, AlertField } from '@agentos/ui'
import { childPath } from '../core/tree'
import type { Scope, ViewCore } from '../core/view'
import type { Json, Node } from '../protocol'

const text = (v: unknown): string => (v === undefined || v === null ? '' : String(v))

export function alertActions(
  view: ViewCore,
  node: Node,
  path: string,
  scope: Scope,
  withFields: boolean
): AlertAction[] {
  const out: AlertAction[] = []
  for (const child of node.slots?.actions ?? []) {
    if (child.type !== 'Button') continue
    const p = (name: string): unknown => view.evaluate(child.props?.[name], scope)
    const role = p('role')
    out.push({
      key: child.key,
      label: text(p('label') || p('title')),
      style: role === 'cancel' ? 'cancel' : role === 'destructive' ? 'destructive' : 'default',
      preferred: role === 'primary',
      disabled: p('disabled') === true,
      onclick: (values) =>
        view.fire(
          child,
          childPath(path, child.key, 'actions'),
          'tap',
          scope,
          withFields && Object.keys(values).length ? ({ fields: values } as Json) : undefined
        )
    })
  }
  return out
}

/** Prompt fields of an Alert (`fields`), validated: a key is required, anything else is ignored. */
export function alertFields(raw: unknown): AlertField[] {
  if (!Array.isArray(raw)) return []
  const out: AlertField[] = []
  for (const f of raw) {
    if (typeof f !== 'object' || f === null) continue
    const { key, placeholder, secure } = f as Record<string, unknown>
    if (typeof key !== 'string' || key === '' || out.some((o) => o.key === key)) continue
    out.push({
      key,
      placeholder: typeof placeholder === 'string' ? placeholder : undefined,
      secure: secure === true
    })
  }
  return out
}
