import { describe, expect, test } from 'bun:test'
import { messageOf } from './errors'

describe('messageOf', () => {
  test('an Error says its message, anything else is shown as text', () => {
    expect(messageOf(new Error('boom'))).toBe('boom')
    expect(messageOf(new TypeError('bad'))).toBe('bad')
    expect(messageOf('plain')).toBe('plain')
    expect(messageOf(42)).toBe('42')
    expect(messageOf(null)).toBe('null')
  })
})
