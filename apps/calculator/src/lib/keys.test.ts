import { describe, expect, test } from 'bun:test'
import { KEYS, keyTyped } from './keys'

describe('the keypad', () => {
  test('has nineteen keys, each with its own test id', () => {
    expect(KEYS).toHaveLength(19)
    expect(new Set(KEYS.map((k) => k.id)).size).toBe(19)
  })

  test('every key whose label is a glyph has a name to be read', () => {
    for (const k of KEYS) {
      if (!/^[0-9]$/.test(k.label) && k.key !== 'clear') expect(k.name, k.id).toBeString()
    }
  })
})

describe('the keyboard', () => {
  test.each([
    ['7', '7'],
    ['+', '+'],
    ['x', '*'],
    ['*', '*'],
    ['/', '/'],
    ['=', '='],
    ['Enter', '='],
    ['%', 'percent'],
    ['Backspace', 'backspace'],
    ['Escape', 'clear'],
    ['.', '.'],
    [',', '.']
  ] as const)('%s is the %s key', (typed, key) => {
    expect(keyTyped(typed)).toBe(key)
  })

  test.each(['a', 'Tab', 'Shift', 'F5', '10', 'constructor'])('%s is not a key', (typed) => {
    expect(keyTyped(typed)).toBeUndefined()
  })
})
