import { describe, expect, test } from 'bun:test'
import { evaluate, formatDisplay } from './evaluate'

describe('evaluate', () => {
  test.each([
    ['1 + 2 * 3', 7],
    ['(1 + 2) * 3', 9],
    ['2 ^ 3 ^ 2', 512],
    ['-4 + 10', 6],
    ['-(2 + 3)', -5],
    ['10 % 4', 2],
    ['7 ÷ 2', 3.5],
    ['3 × −2', -6],
    ['0.1 + 0.2', 0.3]
  ])('%s = %d', (expr, expected) => {
    expect(evaluate(expr)).toBe(expected)
  })

  test.each(['', '1 +', '(1 + 2', '1 / 0', '2 $ 3', '1..2'])('rejects %p', (expr) => {
    expect(() => evaluate(expr)).toThrow()
  })
})

describe('formatDisplay', () => {
  test('groups thousands', () => expect(formatDisplay(1234567)).toBe('1,234,567'))
  test('keeps decimals', () => expect(formatDisplay(3.5)).toBe('3.5'))
  test('uses exponent for huge values', () => expect(formatDisplay(1e12)).toBe('1e12'))
  test('keeps 9 significant digits in exponent form', () =>
    expect(formatDisplay(987654321 * 999)).toBe('9.86666667e11'))
  test('uses exponent for tiny values', () => expect(formatDisplay(0.00000012)).toBe('1.2e-7'))
})
