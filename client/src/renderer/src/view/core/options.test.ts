import { describe, expect, test } from 'bun:test'
import {
  choose,
  keyOf,
  optionsFromRows,
  parseOptions,
  selectedKeys,
  selectionLabel
} from './options'

describe('picker options', () => {
  test('a literal list keeps value, label, icon and disabled', () => {
    const options = parseOptions([
      { value: 'a', label: 'Alpha', icon: 'star' },
      { value: 2, label: 'Two', disabled: true },
      { value: true }
    ])
    expect(options).toEqual([
      { key: 'a', value: 'a', label: 'Alpha', icon: 'star', disabled: false },
      { key: '2', value: 2, label: 'Two', icon: undefined, disabled: true },
      { key: 'true', value: true, label: 'true', icon: undefined, disabled: false }
    ])
  })

  test('malformed entries and repeated values are dropped, the first wins', () => {
    expect(parseOptions(['x', null, { label: 'no value' }, { value: 'a', label: 'A' }, { value: 'a', label: 'again' }])).toHaveLength(1)
    expect(parseOptions('nope')).toEqual([])
    expect(parseOptions(undefined)).toEqual([])
  })

  test('options from source rows read the value and label fields', () => {
    const options = optionsFromRows([
      { id: 1, value: 'x', label: 'X' },
      { id: 2, value: 'y', label: 'Y', disabled: true }
    ])
    expect(options.map((o) => [o.key, o.label, o.disabled])).toEqual([
      ['x', 'X', false],
      ['y', 'Y', true]
    ])
  })

  test('keys: strings as they are, everything else as JSON', () => {
    expect(keyOf('a')).toBe('a')
    expect(keyOf(3)).toBe('3')
    expect(keyOf(null)).toBe('null')
    expect(keyOf({ a: 1 })).toBe('{"a":1}')
  })
})

describe('picker selection', () => {
  test('single selection is one key, multiple is a list', () => {
    expect(selectedKeys('a', false)).toEqual(['a'])
    expect(selectedKeys(null, false)).toEqual([])
    expect(selectedKeys(['a', 2], true)).toEqual(['a', '2'])
    expect(selectedKeys('a', true)).toEqual([])
  })

  test('choosing replaces a single selection and toggles in a multiple one', () => {
    expect(choose('a', 'b', false)).toBe('b')
    expect(choose(['a'], 'b', true)).toEqual(['a', 'b'])
    expect(choose(['a', 'b'], 'a', true)).toEqual(['b'])
    expect(choose(undefined, 1, true)).toEqual([1])
  })

  test('the closed menu label', () => {
    const o = parseOptions([
      { value: 'a', label: 'Alpha' },
      { value: 'b', label: 'Beta' },
      { value: 'c', label: 'Gamma' }
    ])
    expect(selectionLabel(o, [])).toBe('Select')
    expect(selectionLabel(o, ['b'])).toBe('Beta')
    expect(selectionLabel(o, ['a', 'b'])).toBe('Alpha, Beta')
    expect(selectionLabel(o, ['a', 'b', 'c'])).toBe('3 selected')
    expect(selectionLabel(o, ['zzz'])).toBe('Select')
  })
})
