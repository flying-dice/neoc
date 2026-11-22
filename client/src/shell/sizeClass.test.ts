import { describe, expect, test } from 'bun:test'
import { COMPACT_BELOW, sizeClassOf } from './sizeClass'

describe('sizeClassOf', () => {
  test('a length below the breakpoint is compact; the breakpoint itself is regular', () => {
    expect(COMPACT_BELOW).toBe(560)
    expect(sizeClassOf(390)).toBe('compact')
    expect(sizeClassOf(559)).toBe('compact')
    expect(sizeClassOf(560)).toBe('regular')
    expect(sizeClassOf(1280)).toBe('regular')
  })
})
