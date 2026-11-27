import { describe, expect, test } from 'bun:test'
import { TINTS, initialsOf, squareOf, tintOf } from './avatar'

describe('initials', () => {
  test('are the first letters of the first two words, in capitals', () => {
    expect(initialsOf('Scripted agent')).toBe('SA')
    expect(initialsOf('writer')).toBe('W')
    expect(initialsOf('  Ada   Lovelace  King ')).toBe('AL')
  })

  test('take a whole character, not half of one', () => {
    expect(initialsOf('🦊 fox')).toBe('🦊F')
    expect(initialsOf('élan vital')).toBe('ÉV')
  })

  test('of a blank name are none', () => {
    expect(initialsOf('   ')).toBe('')
  })
})

describe('the tint', () => {
  test('is the same every time for the same agent', () => {
    expect(tintOf('3f2c9a1e-agent')).toBe(tintOf('3f2c9a1e-agent'))
  })

  test('is one of the palette colours, and agents get different ones', () => {
    const ids = Array.from({ length: 48 }, (_, i) => `agent-${i}`)
    for (const id of ids) expect(TINTS).toContain(tintOf(id))
    expect(new Set(ids.map(tintOf)).size).toBeGreaterThan(6)
  })
})

describe('the square kept of a picture', () => {
  test('is the middle of a wide one', () => {
    expect(squareOf(400, 300)).toEqual({ x: 50, y: 0, side: 300 })
  })

  test('is the middle of a tall one', () => {
    expect(squareOf(300, 401)).toEqual({ x: 0, y: 50, side: 300 })
  })

  test('is the whole of a square one', () => {
    expect(squareOf(256, 256)).toEqual({ x: 0, y: 0, side: 256 })
  })
})
