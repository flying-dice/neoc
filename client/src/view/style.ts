/**
 * The surface props every node accepts: `background`, `foreground`, `tint`, `corner`: closed token
 * sets mapped to the kit's CSS variables. No raw values pass through: an unknown token is ignored.
 */
import type { Scope } from './core/view'
import type { ViewContext } from './context'
import type { Node } from './protocol'

const BACKGROUND: Record<string, string> = {
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
const FOREGROUND: Record<string, string> = {
  primary: 'var(--label)',
  secondary: 'var(--label-secondary)',
  tertiary: 'var(--label-tertiary)',
  tint: 'var(--tint)',
  destructive: 'var(--ios-red)',
  success: 'var(--ios-green)',
  warning: 'var(--ios-orange)'
}
const CORNER: Record<string, string> = {
  none: '0',
  xs: 'var(--radius-xs)',
  sm: 'var(--radius-sm)',
  md: 'var(--radius-md)',
  lg: 'var(--radius-lg)',
  xl: 'var(--radius-xl)',
  full: '9999px'
}
const PALETTE = new Set([
  'blue',
  'red',
  'green',
  'orange',
  'yellow',
  'purple',
  'pink',
  'indigo',
  'teal',
  'mint',
  'cyan',
  'brown',
  'gray',
  'gray2',
  'gray3',
  'gray4',
  'gray5',
  'gray6'
])

/** `background`, `foreground`, `tint` and `corner` of a node as an inline style (empty when none is set). */
export function surfaceStyle(ctx: ViewContext, node: Node, scope: Scope): string {
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)
  const out: string[] = []
  const bg = BACKGROUND[String(p('background'))]
  if (bg) out.push(`background:${bg}`)
  const fg = FOREGROUND[String(p('foreground'))]
  if (fg) out.push(`color:${fg}`)
  const tint = String(p('tint'))
  if (PALETTE.has(tint)) out.push(`--tint:var(--ios-${tint})`)
  const corner = CORNER[String(p('corner'))]
  if (corner) out.push(`border-radius:${corner};overflow:hidden`)
  return out.join(';')
}

/** Join inline styles, skipping empty ones. */
export const joinStyle = (...styles: (string | undefined)[]): string | undefined =>
  styles.filter(Boolean).join(';') || undefined
