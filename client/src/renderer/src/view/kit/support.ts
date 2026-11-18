/**
 * Shared by the controls, presentation and display adapters: the closed token vocabulary of the common props
 * (`tint`, `background`, `foreground`, `corner`, spec 5.1) as CSS, number coercion for ranges, and the
 * kit-icon lookup. Apps never pass CSS: every value here is looked up in a closed table, an unknown token is
 * ignored, never passed through.
 */
import { isPaletteToken } from '@agentos/ui'
import type { ViewContext } from '../context'
import type { Scope } from '../core/view'
import type { Node } from '../protocol'

const SURFACES: Record<string, string> = {
  primary: 'var(--bg)',
  secondary: 'var(--bg-secondary)',
  tertiary: 'var(--bg-tertiary)',
  grouped: 'var(--bg-grouped)',
  elevated: 'var(--bg-elevated)',
  fill: 'var(--fill)',
  fill2: 'var(--fill-secondary)',
  fill3: 'var(--fill-tertiary)',
  fill4: 'var(--fill-quaternary)',
  tint: 'var(--tint-fill)',
  tintSoft: 'color-mix(in srgb, var(--tint) 15%, transparent)'
}

const LABELS: Record<string, string> = {
  primary: 'var(--label)',
  secondary: 'var(--label-secondary)',
  tertiary: 'var(--label-tertiary)',
  tint: 'var(--tint)',
  destructive: 'var(--ios-red)',
  success: 'color-mix(in oklab, var(--ios-green) 55%, var(--label))',
  warning: 'color-mix(in oklab, var(--ios-orange) 52%, var(--label))'
}

const RADII: Record<string, string> = {
  none: '0',
  xs: 'var(--radius-xs)',
  sm: 'var(--radius-sm)',
  md: 'var(--radius-md)',
  lg: 'var(--radius-lg)',
  xl: 'var(--radius-xl)',
  full: '999px'
}

/**
 * The appearance common props as a style string: `tint` re-points `--tint` for the subtree, `background` is a
 * surface token, `foreground` a label token, `corner` a radius token. Unknown tokens produce nothing.
 */
export function appearanceStyle(ctx: ViewContext, node: Node, scope: Scope): string {
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)
  const out: string[] = []
  const tint = p('tint')
  if (isPaletteToken(tint)) out.push(`--tint:var(--ios-${tint})`)
  const background = p('background')
  if (typeof background === 'string' && SURFACES[background]) out.push(`background:${SURFACES[background]}`)
  const foreground = p('foreground')
  if (typeof foreground === 'string' && LABELS[foreground]) out.push(`color:${LABELS[foreground]}`)
  const corner = p('corner')
  if (typeof corner === 'string' && RADII[corner]) out.push(`border-radius:${RADII[corner]}`)
  return out.join(';')
}

/** A finite number from a prop, or the fallback. */
export const num = (v: unknown, fallback: number): number =>
  typeof v === 'number' && Number.isFinite(v) ? v : fallback

/** A finite number from a prop, or undefined. */
export const optNum = (v: unknown): number | undefined =>
  typeof v === 'number' && Number.isFinite(v) ? v : undefined

/** A non-empty string from a prop, or undefined. */
export const optStr = (v: unknown): string | undefined =>
  typeof v === 'string' && v !== '' ? v : undefined
