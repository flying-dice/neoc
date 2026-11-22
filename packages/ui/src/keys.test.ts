import { describe, expect, test } from 'bun:test'
import { gridIndex, isContextMenuKey, isTypingKey, moveIndex, nextEnabled, trapIndex } from './keys'

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

  test('typing keys: characters, Backspace and Delete, never with a command modifier, Space, Enter, Tab or Escape', () => {
    for (const key of ['a', '7', '*', '=', 'Backspace', 'Delete']) expect(isTypingKey({ key })).toBe(true)
    for (const key of [' ', 'Enter', 'Tab', 'Escape', 'ArrowUp', 'Shift', 'F5']) expect(isTypingKey({ key })).toBe(false)
    for (const mod of ['metaKey', 'ctrlKey', 'altKey'] as const)
      for (const key of ['r', 'Backspace']) expect(isTypingKey({ key, [mod]: true })).toBe(false)
  })
})

describe('gridIndex', () => {
  // 7 items in rows of 3:  0 1 2 / 3 4 5 / 6
  test('Left and Right step in reading order and stop at the ends', () => {
    expect(gridIndex(7, 3, 2, 'ArrowRight')).toBe(3)
    expect(gridIndex(7, 3, 3, 'ArrowLeft')).toBe(2)
    expect(gridIndex(7, 3, 6, 'ArrowRight')).toBe(6)
    expect(gridIndex(7, 3, 0, 'ArrowLeft')).toBe(0)
  })

  test('Up and Down keep the column and stop at the first and last rows', () => {
    expect(gridIndex(7, 3, 1, 'ArrowDown')).toBe(4)
    expect(gridIndex(7, 3, 4, 'ArrowUp')).toBe(1)
    expect(gridIndex(7, 3, 1, 'ArrowUp')).toBe(1)
    expect(gridIndex(7, 3, 6, 'ArrowDown')).toBe(6)
  })

  test('Down from above a short last row lands on its last item', () => {
    expect(gridIndex(7, 3, 5, 'ArrowDown')).toBe(6)
    expect(gridIndex(7, 3, 3, 'ArrowDown')).toBe(6)
  })

  test('Home and End go to the ends of the row, or of the grid with a modifier', () => {
    expect(gridIndex(7, 3, 4, 'Home')).toBe(3)
    expect(gridIndex(7, 3, 4, 'End')).toBe(5)
    expect(gridIndex(7, 3, 6, 'End')).toBe(6)
    expect(gridIndex(7, 3, 4, 'Home', true)).toBe(0)
    expect(gridIndex(7, 3, 4, 'End', true)).toBe(6)
  })

  test('one row, an empty grid, and keys that do not move', () => {
    expect(gridIndex(3, 8, 1, 'ArrowDown')).toBe(1)
    expect(gridIndex(0, 3, 0, 'ArrowRight')).toBeNull()
    expect(gridIndex(7, 3, 0, 'Enter')).toBeNull()
  })
})

describe('isContextMenuKey', () => {
  test('the Menu key and Shift+F10 ask for a context menu; F10 alone and other keys do not', () => {
    expect(isContextMenuKey({ key: 'ContextMenu' })).toBe(true)
    expect(isContextMenuKey({ key: 'F10', shiftKey: true })).toBe(true)
    expect(isContextMenuKey({ key: 'F10' })).toBe(false)
    expect(isContextMenuKey({ key: 'F10', shiftKey: true, ctrlKey: true })).toBe(false)
    expect(isContextMenuKey({ key: 'Enter' })).toBe(false)
  })
})
