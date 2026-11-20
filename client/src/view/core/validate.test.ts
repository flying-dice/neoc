import { describe, expect, test } from 'bun:test'
import { parseRules, validateText } from './validate'

describe('text field validation (advice for the person typing)', () => {
  test('required, length, pattern', () => {
    expect(validateText('', { required: true })).toBe('Required')
    expect(validateText('   ', { required: true })).toBe('Required')
    expect(validateText('', {})).toBeNull()
    expect(validateText('abcdef', { maxLength: 3 })).toBe('At most 3 characters')
    expect(validateText('x1', { pattern: '^[a-z]+$' })).toBe('Not in the expected format')
    expect(validateText('xy', { pattern: '^[a-z]+$' })).toBeNull()
  })
  test('numbers: format, min and max', () => {
    expect(validateText('abc', {}, true)).toBe('Enter a number')
    expect(validateText('3', { min: 5 }, true)).toBe('At least 5')
    expect(validateText('9', { max: 5 }, true)).toBe('At most 5')
    expect(validateText('5', { min: 5, max: 5 }, true)).toBeNull()
    expect(validateText('3', { min: 5 }, false)).toBeNull() // min and max only constrain numeric fields
  })
  test('an invalid pattern from the tree never blocks typing', () => {
    expect(validateText('anything', { pattern: '(' })).toBeNull()
  })
  test('rules are read defensively from the tree', () => {
    expect(parseRules({ required: true, maxLength: 5, pattern: 7, min: 'x' })).toEqual({
      required: true,
      pattern: undefined,
      min: undefined,
      max: undefined,
      maxLength: 5
    })
    expect(parseRules(null)).toEqual({})
    expect(parseRules([1])).toEqual({})
  })
})
