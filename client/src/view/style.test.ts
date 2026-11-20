import { describe, expect, test } from 'bun:test'
import { createHarness } from './host/harness'
import { joinStyle, surfaceStyle } from './style'
import type { ViewContext } from './context'
import type { Node } from './protocol'

/** Only `view.evaluate` is read: literals and refs through the real core. */
async function ctxFor(): Promise<ViewContext> {
  const h = createHarness()
  const view = await h.openNotes()
  return { view, app: 'notes' } as unknown as ViewContext
}

const node = (props: Node['props']): Node => ({ type: 'Stack', key: 's', props })

describe('surfaceStyle: the surface props map to tokens, never to raw values', () => {
  test('background, foreground, corner and tint', async () => {
    const ctx = await ctxFor()
    const style = surfaceStyle(
      ctx,
      node({ background: 'tintSoft', foreground: 'secondary', corner: 'lg', tint: 'orange' }),
      {}
    )
    expect(style).toContain('background:color-mix(in srgb, var(--tint) 15%, transparent)')
    expect(style).toContain('color:var(--label-secondary)')
    expect(style).toContain('border-radius:var(--radius-lg);overflow:hidden')
    expect(style).toContain('--tint:var(--ios-orange)')
  })

  test('unknown tokens and raw colours are ignored', async () => {
    const ctx = await ctxFor()
    expect(
      surfaceStyle(ctx, node({ background: '#ff0000', foreground: 'red', tint: 'calc(1px)' }), {})
    ).toBe('')
    expect(surfaceStyle(ctx, node({ corner: 'url(x)' }), {})).toBe('')
    expect(surfaceStyle(ctx, node({}), {})).toBe('')
  })

  test('refs are read like any prop', async () => {
    const ctx = await ctxFor()
    ctx.view.write('state.body', 'x')
    expect(
      surfaceStyle(ctx, node({ corner: { $fn: 'if', args: [true, 'full', 'none'] } }), {})
    ).toContain('9999px')
  })

  test('joinStyle skips empty styles', () => {
    expect(joinStyle('a:1', '', undefined, 'b:2')).toBe('a:1;b:2')
    expect(joinStyle('', undefined)).toBeUndefined()
  })
})
