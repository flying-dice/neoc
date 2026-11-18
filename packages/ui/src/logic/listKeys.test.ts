import { describe, expect, test } from 'bun:test'
import { listKeyAction, tabKeyIndex, TypeaheadBuffer, typeaheadMatch } from './listKeys'

const ctx = { index: 3, count: 10, page: 5, canReorder: false }

describe('listKeyAction', () => {
  test('arrows move one row and clamp at the ends', () => {
    expect(listKeyAction({ key: 'ArrowDown' }, ctx)).toEqual({ type: 'focus', index: 4 })
    expect(listKeyAction({ key: 'ArrowUp' }, ctx)).toEqual({ type: 'focus', index: 2 })
    expect(listKeyAction({ key: 'ArrowUp' }, { ...ctx, index: 0 })).toEqual({ type: 'focus', index: 0 })
    expect(listKeyAction({ key: 'ArrowDown' }, { ...ctx, index: 9 })).toEqual({ type: 'focus', index: 9 })
  })

  test('Home, End, Page Up and Page Down', () => {
    expect(listKeyAction({ key: 'Home' }, ctx)).toEqual({ type: 'focus', index: 0 })
    expect(listKeyAction({ key: 'End' }, ctx)).toEqual({ type: 'focus', index: 9 })
    expect(listKeyAction({ key: 'PageDown' }, ctx)).toEqual({ type: 'focus', index: 8 })
    expect(listKeyAction({ key: 'PageDown' }, { ...ctx, index: 8 })).toEqual({ type: 'focus', index: 9 })
    expect(listKeyAction({ key: 'PageUp' }, ctx)).toEqual({ type: 'focus', index: 0 })
    expect(listKeyAction({ key: 'End', ctrlKey: true }, ctx)).toEqual({ type: 'focus', index: 9 })
  })

  test('Alt+Arrow reorders only when the list can reorder', () => {
    expect(listKeyAction({ key: 'ArrowUp', altKey: true }, ctx)).toBeNull()
    const c = { ...ctx, canReorder: true }
    expect(listKeyAction({ key: 'ArrowUp', altKey: true }, c)).toEqual({ type: 'reorder', delta: -1 })
    expect(listKeyAction({ key: 'ArrowDown', altKey: true }, c)).toEqual({ type: 'reorder', delta: 1 })
  })

  test('swipe actions open from the keyboard on the right side, and close again', () => {
    const swipe = { leading: true, trailing: true, open: false }
    const c = { ...ctx, swipe }
    expect(listKeyAction({ key: 'ArrowLeft', altKey: true }, c)).toEqual({ type: 'swipe', side: 'trailing' })
    expect(listKeyAction({ key: 'ArrowRight', altKey: true }, c)).toEqual({ type: 'swipe', side: 'leading' })
    expect(listKeyAction({ key: 'ArrowLeft', altKey: true }, { ...c, rtl: true })).toEqual({
      type: 'swipe',
      side: 'leading'
    })
    expect(listKeyAction({ key: 'ContextMenu' }, c)).toEqual({ type: 'swipe', side: 'trailing' })
    expect(listKeyAction({ key: 'F10', shiftKey: true }, { ...c, swipe: { ...swipe, trailing: false } })).toEqual({
      type: 'swipe',
      side: 'leading'
    })
    const open = { ...c, swipe: { ...swipe, open: true } }
    expect(listKeyAction({ key: 'Escape' }, open)).toEqual({ type: 'swipe', side: 'close' })
    expect(listKeyAction({ key: 'ArrowLeft', altKey: true }, open)).toEqual({ type: 'swipe', side: 'close' })
  })

  test('a row without that side ignores the swipe key', () => {
    const c = { ...ctx, swipe: { leading: false, trailing: false, open: false } }
    expect(listKeyAction({ key: 'ArrowLeft', altKey: true }, c)).toBeNull()
    expect(listKeyAction({ key: 'ContextMenu' }, c)).toBeNull()
    expect(listKeyAction({ key: 'Escape' }, ctx)).toBeNull()
  })

  test('printable keys are type-to-select; space and Enter are left to the row', () => {
    expect(listKeyAction({ key: 'n' }, ctx)).toEqual({ type: 'typeahead', char: 'n' })
    expect(listKeyAction({ key: ' ' }, ctx)).toBeNull()
    expect(listKeyAction({ key: 'Enter' }, ctx)).toBeNull()
    expect(listKeyAction({ key: 'c', metaKey: true }, ctx)).toBeNull()
  })
})

describe('typeahead', () => {
  const labels = ['Apple', 'Avocado', 'Banana', 'Cherry']

  test('finds the first match after the current row, wrapping', () => {
    expect(typeaheadMatch(labels, 'b', 0)).toBe(2)
    expect(typeaheadMatch(labels, 'ap', 2)).toBe(0)
    expect(typeaheadMatch(labels, 'zz', 0)).toBe(-1)
  })

  test('a repeated letter cycles through the matches', () => {
    expect(typeaheadMatch(labels, 'a', 0)).toBe(1)
    expect(typeaheadMatch(labels, 'aa', 1)).toBe(0)
  })

  test('the buffer forgets after its window', () => {
    const b = new TypeaheadBuffer(500)
    expect(b.push('a', 0)).toBe('a')
    expect(b.push('p', 200)).toBe('ap')
    expect(b.push('x', 1000)).toBe('x')
  })
})

describe('tabKeyIndex', () => {
  test('arrows wrap, Home and End jump', () => {
    expect(tabKeyIndex('ArrowRight', 2, 3)).toBe(0)
    expect(tabKeyIndex('ArrowLeft', 0, 3)).toBe(2)
    expect(tabKeyIndex('Home', 2, 3)).toBe(0)
    expect(tabKeyIndex('End', 0, 3)).toBe(2)
    expect(tabKeyIndex('ArrowRight', 0, 3, true)).toBe(2)
    expect(tabKeyIndex('x', 0, 3)).toBeNull()
    expect(tabKeyIndex('ArrowRight', 0, 0)).toBeNull()
  })
})
