/**
 * Menu content from wire nodes: the children of a `Menu`, or the `contextMenu` slot of any node, become the
 * kit's `MenuItem[]`. `Button` is an item, `Toggle` a checkmark item, `Divider` a separator, `Picker` the choices
 * of a group (the chosen one checked), a nested `Menu` a submenu, `Section` a heading over its items. `Group` and
 * `Show` are transparent. Anything else is not a menu item and is left out. Taps run through the view core, so a
 * menu item behaves exactly like the same node elsewhere (client actions first, `emit` towards the app).
 */
import { icons, type IconName, type MenuItem } from '@agentos/ui'
import type { Json, Node } from '../protocol'
import { parseShortcut } from './shortcuts'
import { childPath } from './tree'
import {
  choose,
  keyOf,
  optionsFromRows,
  parseOptions,
  selectedKeys,
  type PickerOption
} from './options'
import type { Scope, ViewCore } from './view'

const text = (v: unknown): string => (v === undefined || v === null ? '' : String(v))

/** A kit icon name from a prop, or undefined for anything the icon set does not have. */
export const iconOf = (v: unknown): IconName | undefined =>
  typeof v === 'string' && v in icons ? (v as IconName) : undefined

/** `cmd+n` shown the way menus show it: `⌘N` on Apple platforms, `Ctrl+N` elsewhere. */
export function formatShortcut(spec: string, apple: boolean): string | undefined {
  const c = parseShortcut(spec)
  if (!c) return undefined
  const key =
    c.key === 'escape'
      ? 'Esc'
      : c.key === 'enter'
        ? '↩'
        : c.key === 'backspace'
          ? '⌫'
          : c.key === ' '
            ? 'Space'
            : c.key.startsWith('arrow')
              ? { arrowup: '↑', arrowdown: '↓', arrowleft: '←', arrowright: '→' }[c.key]!
              : c.key.length === 1
                ? c.key.toUpperCase()
                : c.key[0]!.toUpperCase() + c.key.slice(1)
  if (apple)
    return `${c.ctrl ? '⌃' : ''}${c.alt ? '⌥' : ''}${c.shift ? '⇧' : ''}${c.cmd ? '⌘' : ''}${key}`
  return [c.cmd || c.ctrl ? 'Ctrl' : '', c.alt ? 'Alt' : '', c.shift ? 'Shift' : '', key]
    .filter(Boolean)
    .join('+')
}

export interface MenuContext {
  view: ViewCore
  scope: Scope
  apple: boolean
}

/** Options of a Picker: a literal list, or the rows of the source its `options` prop names. */
export function pickerOptions(view: ViewCore, node: Node, scope: Scope): PickerOption[] {
  const raw = view.evaluate(node.props?.options, scope)
  if (typeof raw === 'string') {
    const data = view.store.sources[raw]
    if (data?.kind !== 'rows') return []
    return optionsFromRows(
      data.keys.map((k) => data.rows[String(k)]).filter((r) => r !== undefined)
    )
  }
  return parseOptions(raw)
}

export function menuItems(
  ctx: MenuContext,
  nodes: readonly Node[] | undefined,
  parentPath: string,
  slotName?: string
): MenuItem[] {
  const { view, scope } = ctx
  const out: MenuItem[] = []
  for (const node of nodes ?? []) {
    const path = childPath(parentPath, node.key, slotName)
    const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)
    const label = text(p('label') || p('title'))
    const disabled = p('disabled') === true
    const icon = iconOf(p('icon'))
    switch (node.type) {
      case 'Button': {
        const spec = text(p('shortcut'))
        out.push({
          key: node.key,
          label,
          icon,
          disabled,
          destructive: p('role') === 'destructive',
          shortcut: spec ? formatShortcut(spec, ctx.apple) : undefined,
          onclick: () => view.fire(node, path, 'tap', scope)
        })
        break
      }
      case 'Toggle': {
        const binding = view.bindingOf(node.props?.on, scope)
        const on = Boolean(binding ? binding.value : p('on'))
        out.push({
          key: node.key,
          label,
          disabled: disabled || binding === null,
          checked: on,
          onclick: () => {
            binding?.set(!on)
            view.fire(node, path, 'change', scope, { value: !on })
          }
        })
        break
      }
      case 'Divider':
        out.push({ separator: true })
        break
      case 'Picker': {
        const multiple = p('multiple') === true
        const binding = view.bindingOf(node.props?.selection, scope)
        const current = binding ? binding.value : p('selection')
        const chosen = new Set(selectedKeys(current, multiple))
        if (label) out.push({ heading: label })
        for (const o of pickerOptions(view, node, scope))
          out.push({
            key: `${node.key}-${o.key}`,
            label: o.label,
            disabled: disabled || o.disabled,
            checked: chosen.has(o.key),
            onclick: () => {
              const next = choose(current, o.value, multiple)
              binding?.set(next)
              view.fire(node, path, 'change', scope, { value: next as Json })
            }
          })
        break
      }
      case 'Menu':
        out.push({
          key: node.key,
          label,
          disabled,
          submenu: menuItems(ctx, node.children, path)
        })
        break
      case 'Section': {
        const heading = text(p('header') || p('label') || p('title'))
        if (heading) out.push({ heading })
        out.push(...menuItems(ctx, node.children, path))
        break
      }
      case 'Group':
        out.push(...menuItems(ctx, node.children, path))
        break
      case 'Show':
        if (p('when') === true) out.push(...menuItems(ctx, node.children, path))
        else out.push(...menuItems(ctx, node.slots?.otherwise, path, 'otherwise'))
        break
    }
  }
  return out
}

/** True when the picker's key is currently chosen (used by tests and the menu picker label). */
export const isChosen = (current: unknown, value: Json, multiple: boolean): boolean =>
  selectedKeys(current, multiple).includes(keyOf(value))
