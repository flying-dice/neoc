import { describe, expect, test } from 'bun:test'
import type { ViewContext } from './context'
import type { Node } from './protocol'
import { commonOf, layoutStyle, needsHost, oneOf, str, unit } from './props'

/** The slice of the view context the prop helpers read: expression evaluation. */
const ctx = (values: Record<string, unknown> = {}): ViewContext =>
  ({
    app: 'notes',
    view: {
      evaluate: (e: unknown) =>
        typeof e === 'object' && e !== null && '$ref' in e
          ? values[(e as { $ref: string }).$ref]
          : e
    }
  }) as unknown as ViewContext

const node = (props: Node['props'], extra: Partial<Node> = {}): Node => ({
  type: 'X',
  key: 'x',
  props,
  ...extra
})

describe('coercion', () => {
  test('units are 4 pt, integers 0 to 8, clamped', () => {
    expect([0, 2, 8, 9, -3, 1.6, 'x', null].map(unit)).toEqual([0, 8, 32, 32, 0, 8, 0, 0])
  })
  test('oneOf falls back for unknown enum values (additive kit versions)', () => {
    expect(oneOf('h', ['v', 'h'] as const, 'v')).toBe('h')
    expect(oneOf('diagonal', ['v', 'h'] as const, 'v')).toBe('v')
    expect(str(null, 'x')).toBe('x')
    expect(str(0)).toBe('0')
  })
})

describe('the closed layout vocabulary', () => {
  test('padding as a unit or per edge, grow, align', () => {
    expect(layoutStyle(ctx(), node({ padding: 4 }), {})).toBe('padding:16px')
    expect(layoutStyle(ctx(), node({ padding: { x: 2, top: 4 } }), {})).toBe(
      'padding-top:16px;padding-left:8px;padding-right:8px'
    )
    expect(layoutStyle(ctx(), node({ grow: 1, align: 'center' }), {})).toBe(
      'flex-grow:1;align-self:center'
    )
  })
  test('frame clamps to numbers and "fill"; nothing arbitrary reaches CSS', () => {
    expect(layoutStyle(ctx(), node({ frame: 'fill' }), {})).toBe('width:100%;height:100%')
    expect(layoutStyle(ctx(), node({ frame: { minW: 100, maxW: 'calc(1px)' } }), {})).toBe(
      'min-width:100px'
    )
    expect(layoutStyle(ctx(), node({ align: 'url(x)', grow: 'lots' }), {})).toBe('')
  })
  test('props are evaluated, so a ref can drive layout', () => {
    expect(
      layoutStyle(ctx({ 'client.pad': 1 }), node({ padding: { $ref: 'client.pad' } }), {})
    ).toBe('padding:4px')
  })
})

describe('common props', () => {
  test('test id, accessible name and description pass through; an explicit testid wins', () => {
    const c = commonOf(
      ctx(),
      node({ label: 'Save', hint: 'Saves the note', disabled: true, shortcut: 'cmd+s' }),
      'main/_0/save',
      {}
    )
    expect(c).toMatchObject({
      testid: 'notes-save',
      label: 'Save',
      hint: 'Saves the note',
      disabled: true,
      shortcut: 'cmd+s'
    })
    expect(commonOf(ctx(), node({ testid: 'custom' }), 'main/save', {}).testid).toBe('custom')
  })
  test('row templates derive ids from the row key', () => {
    expect(commonOf(ctx(), node({}), 'main/list/@row/item', { rowKey: 7 }).testid).toBe(
      'notes-list-row-7-item'
    )
  })
})

describe('offline: which handlers need the host', () => {
  test('emit and `true` do; client actions do not; a seq with an emit does', () => {
    expect(needsHost(true)).toBe(true)
    expect(needsHost(undefined)).toBe(false)
    expect(needsHost([{ do: 'set', ref: 'client.x', value: 1 }])).toBe(false)
    expect(needsHost([{ do: 'set', ref: 'client.x', value: 1 }, { do: 'emit' }])).toBe(true)
    expect(
      needsHost([
        {
          do: 'seq',
          actions: [
            { do: 'toggle', ref: 'client.x' },
            { do: 'emit', event: 'go' }
          ]
        }
      ])
    ).toBe(true)
    expect(needsHost([{ do: 'seq', actions: [{ do: 'toggle', ref: 'client.x' }] }])).toBe(false)
  })
})
