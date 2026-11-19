import { describe, expect, test } from 'bun:test'
import { place, placeAtPoint, submenuFlips } from './floating'
import {
  isActivateKey,
  isContextMenuKey,
  isPrintable,
  isTypingKey,
  moveIndex,
  nextEnabled,
  trapIndex,
  typeAhead
} from './keys'

describe('roving focus', () => {
  test('arrows move by one and wrap; Home and End jump', () => {
    expect(moveIndex(4, 0, 'ArrowDown')).toBe(1)
    expect(moveIndex(4, 3, 'ArrowDown')).toBe(0)
    expect(moveIndex(4, 0, 'ArrowUp')).toBe(3)
    expect(moveIndex(4, 2, 'Home')).toBe(0)
    expect(moveIndex(4, 1, 'End')).toBe(3)
    expect(moveIndex(4, 3, 'ArrowDown', 'vertical', false)).toBe(3)
  })

  test('an orientation ignores the other axis, both accepts either', () => {
    expect(moveIndex(3, 0, 'ArrowRight', 'vertical')).toBeNull()
    expect(moveIndex(3, 0, 'ArrowDown', 'horizontal')).toBeNull()
    expect(moveIndex(3, 0, 'ArrowRight', 'horizontal')).toBe(1)
    expect(moveIndex(3, 0, 'ArrowDown', 'both')).toBe(1)
    expect(moveIndex(3, 0, 'ArrowRight', 'both')).toBe(1)
    expect(moveIndex(3, 0, 'a')).toBeNull()
    expect(moveIndex(0, 0, 'ArrowDown')).toBeNull()
  })

  test('disabled items are skipped', () => {
    expect(nextEnabled([false, true, true, false], 0, 1)).toBe(3)
    expect(nextEnabled([false, true, true, false], 3, 1)).toBe(0)
    expect(nextEnabled([false, true, true, false], 0, -1)).toBe(3)
    expect(nextEnabled([true, true], 0, 1)).toBe(0)
  })

  test('type-to-select finds the next label with the prefix, wrapping', () => {
    const labels = ['Apple', 'Banana', 'Blueberry', 'Cherry']
    expect(typeAhead(labels, 'b', 0)).toBe(1)
    expect(typeAhead(labels, 'b', 1)).toBe(2)
    expect(typeAhead(labels, 'b', 2)).toBe(1)
    expect(typeAhead(labels, 'bl', 0)).toBe(2)
    expect(typeAhead(labels, 'z', 0)).toBe(-1)
    expect(typeAhead(labels, '', 0)).toBe(-1)
  })
})

describe('focus trap and key predicates', () => {
  test('Tab wraps at both ends', () => {
    expect(trapIndex(3, 2, false)).toBe(0)
    expect(trapIndex(3, 0, true)).toBe(2)
    expect(trapIndex(3, 1, false)).toBe(2)
    expect(trapIndex(3, -1, false)).toBe(0)
    expect(trapIndex(3, -1, true)).toBe(2)
    expect(trapIndex(0, 0, false)).toBe(-1)
  })

  test('the keyboard path to a context menu is the Menu key or Shift+F10', () => {
    expect(isContextMenuKey({ key: 'ContextMenu', shiftKey: false })).toBe(true)
    expect(isContextMenuKey({ key: 'F10', shiftKey: true })).toBe(true)
    expect(isContextMenuKey({ key: 'F10', shiftKey: false })).toBe(false)
    expect(isContextMenuKey({ key: 'a', shiftKey: true })).toBe(false)
  })

  test('activate keys and printable keys', () => {
    expect(isActivateKey('Enter')).toBe(true)
    expect(isActivateKey(' ')).toBe(true)
    expect(isActivateKey('a')).toBe(false)
    const k = { ctrlKey: false, metaKey: false, altKey: false }
    expect(isPrintable({ key: 'a', ...k })).toBe(true)
    expect(isPrintable({ key: 'a', ...k, metaKey: true })).toBe(false)
    expect(isPrintable({ key: ' ', ...k })).toBe(false)
    expect(isPrintable({ key: 'Enter', ...k })).toBe(false)
  })

  test('typing keys: characters, Backspace and Delete, never with a command modifier, Space, Enter, Tab or Escape', () => {
    for (const key of ['a', '7', '*', '=', 'Backspace', 'Delete']) expect(isTypingKey({ key })).toBe(true)
    for (const key of [' ', 'Enter', 'Tab', 'Escape', 'ArrowUp', 'Shift', 'F5']) expect(isTypingKey({ key })).toBe(false)
    for (const mod of ['metaKey', 'ctrlKey', 'altKey'] as const)
      for (const key of ['r', 'Backspace']) expect(isTypingKey({ key, [mod]: true })).toBe(false)
  })
})

describe('floating placement', () => {
  const container = { width: 400, height: 300 }
  const size = { width: 200, height: 120 }

  test('sits below the anchor, centred, with an arrow pointing at it', () => {
    const p = place({ x: 150, y: 20, width: 100, height: 30 }, size, container, 'bottom')
    expect(p.edge).toBe('bottom')
    expect(p.y).toBe(20 + 30 + 8)
    expect(p.x).toBe(200 - 100)
    expect(p.arrow).toBe(100)
  })

  test('flips to the other side when there is no room, and clamps into the container', () => {
    const below = place({ x: 150, y: 250, width: 100, height: 30 }, size, container, 'bottom')
    expect(below.edge).toBe('top')
    expect(below.y).toBe(250 - 120 - 8)
    const edge = place({ x: 0, y: 20, width: 40, height: 30 }, size, container, 'bottom')
    expect(edge.x).toBe(8)
    expect(edge.arrow).toBeGreaterThanOrEqual(16)
    const right = place({ x: 380, y: 20, width: 20, height: 30 }, size, container, 'bottom')
    expect(right.x + size.width).toBeLessThanOrEqual(container.width - 8)
  })

  test('leading means the left in left-to-right and the right in right-to-left', () => {
    const wide = { width: 800, height: 300 }
    const anchor = { x: 400, y: 100, width: 40, height: 40 }
    expect(place(anchor, size, wide, 'leading').x).toBe(400 - 200 - 8)
    expect(place(anchor, size, wide, 'leading', 8, 8, true).x).toBe(440 + 8)
    expect(place(anchor, size, wide, 'trailing').x).toBe(440 + 8)
  })

  test('on a surface larger than the container the max size says how far it can grow', () => {
    const tiny = place({ x: 10, y: 10, width: 20, height: 20 }, { width: 500, height: 400 }, { width: 320, height: 240 })
    expect(tiny.maxWidth).toBe(304)
    expect(tiny.maxHeight).toBe(224)
  })

  test('a context menu opens at the point, flipping at the far edges', () => {
    expect(placeAtPoint({ x: 50, y: 60 }, { width: 100, height: 100 }, container)).toEqual({ x: 50, y: 60 })
    expect(placeAtPoint({ x: 380, y: 290 }, { width: 100, height: 100 }, container)).toEqual({ x: 280, y: 190 })
    expect(placeAtPoint({ x: 0, y: 0 }, { width: 100, height: 100 }, container)).toEqual({ x: 8, y: 8 })
  })

  test('a submenu flips left near the right edge', () => {
    expect(submenuFlips({ x: 200, y: 0, width: 100, height: 40 }, 180, 400)).toBe(true)
    expect(submenuFlips({ x: 20, y: 0, width: 100, height: 40 }, 180, 400)).toBe(false)
  })
})
