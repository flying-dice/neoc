/**
 * Prop helpers shared by the adapters: coercion of evaluated values, the common props every node accepts
 * and the closed layout vocabulary (4 pt spacing units, frame clamps). Adapters call these inside
 * `$derived`, so every store read they make is tracked per prop.
 */
import type { Scope } from './core/view'
import { testid } from './core/testid'
import type { Action, Handler, Node } from './protocol'
import type { ViewContext } from './context'

export const str = (v: unknown, fallback = ''): string =>
  v === null || v === undefined ? fallback : String(v)
export const bool = (v: unknown): boolean => v === true
export const oneOf = <T extends string>(v: unknown, allowed: readonly T[], fallback: T): T =>
  (allowed as readonly unknown[]).includes(v) ? (v as T) : fallback

/** Spacing is an integer 0..8 in units of 4 pt; anything else is clamped, never passed through. */
export const unit = (v: unknown): number => Math.min(8, Math.max(0, Math.round(Number(v) || 0))) * 4

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)
const px = (v: unknown): string | undefined =>
  typeof v === 'number' && Number.isFinite(v) && v >= 0 ? `${Math.round(v)}px` : undefined

/** Layout props common to all nodes: `padding`, `frame`, `grow`, `align`. Returned as a style string. */
export function layoutStyle(ctx: ViewContext, node: Node, scope: Scope): string {
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)
  const out: string[] = []
  const padding = p('padding')
  if (isRecord(padding)) {
    const x = padding.x
    const y = padding.y
    const side = (name: string, fallback: unknown): void => {
      const v = padding[name] ?? fallback
      if (v !== undefined) out.push(`padding-${name}:${unit(v)}px`)
    }
    side('top', y)
    side('bottom', y)
    side('left', x)
    side('right', x)
  } else if (padding !== undefined) out.push(`padding:${unit(padding)}px`)

  const frame = p('frame')
  if (frame === 'fill') out.push('width:100%;height:100%')
  else if (isRecord(frame)) {
    const set = (css: string, key: string): void => {
      const v = px(frame[key])
      if (v) out.push(`${css}:${v}`)
    }
    set('min-width', 'minW')
    set('max-width', 'maxW')
    set('min-height', 'minH')
    set('max-height', 'maxH')
    set('width', 'idealW')
    set('height', 'idealH')
  }
  const grow = Number(p('grow'))
  if (Number.isFinite(grow) && grow > 0) out.push(`flex-grow:${Math.min(grow, 10)}`)
  const align = p('align')
  const alignSelf: Record<string, string> = {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    stretch: 'stretch'
  }
  if (typeof align === 'string' && alignSelf[align]) out.push(`align-self:${alignSelf[align]}`)
  return out.join(';')
}

export interface Common {
  /** The derived (or overridden) test id. */
  testid: string
  /** Accessible name. */
  label: string | undefined
  /** Accessible description. */
  hint: string | undefined
  /** Tooltip. */
  help: string | undefined
  disabled: boolean
  style: string
  shortcut: string | undefined
}

/** The common props of a node that adapters pass through to the kit component. */
export function commonOf(ctx: ViewContext, node: Node, path: string, scope: Scope): Common {
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)
  const optional = (v: unknown): string | undefined =>
    v === null || v === undefined || v === '' ? undefined : String(v)
  return {
    testid: testid(ctx.app, path, scope.rowKey, node.props?.testid),
    label: optional(p('label')),
    hint: optional(p('hint')),
    help: optional(p('help')),
    disabled: bool(p('disabled')),
    style: layoutStyle(ctx, node, scope),
    shortcut: optional(p('shortcut'))
  }
}

/** True when running the handler needs the host (an `emit`): offline, such controls are disabled. */
export function needsHost(handler: Handler | undefined): boolean {
  const needs = (a: Action): boolean => a.do === 'emit' || (a.do === 'seq' && a.actions.some(needs))
  if (handler === undefined) return false
  return handler === true || handler.some(needs)
}
