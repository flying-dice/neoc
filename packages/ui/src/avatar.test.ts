import { describe, expect, test } from 'bun:test'
import { colorForName, initials, safeImageSrc } from './avatar'

describe('Avatar initials and colour', () => {
  test('first and last word initials, upper case', () => {
    expect(initials('Ada Lovelace')).toBe('AL')
    expect(initials('ada')).toBe('A')
    expect(initials('  grace   brewster  murray hopper ')).toBe('GH')
    expect(initials('code-reviewer')).toBe('CR')
    expect(initials('Édith Piaf')).toBe('ÉP')
  })

  test('nothing to show for empty or symbol-only names', () => {
    expect(initials('')).toBe('')
    expect(initials('   ')).toBe('')
    expect(initials('---')).toBe('')
  })

  test('the colour for a name is stable and a palette token', () => {
    expect(colorForName('Ada')).toBe(colorForName('Ada'))
    expect(colorForName('Ada')).toMatch(/^[a-z]+$/)
    const seen = new Set(['Ada', 'Grace', 'Linus', 'Margaret', 'Alan', 'Barbara', 'Ken', 'Dennis'].map(colorForName))
    expect(seen.size).toBeGreaterThan(2)
  })

  test('only https URLs and same-origin paths load as images', () => {
    expect(safeImageSrc('https://example.com/a.png')).toBe('https://example.com/a.png')
    expect(safeImageSrc('/assets/logo.png')).toBe('/assets/logo.png')
    for (const bad of [
      'http://example.com/a.png',
      'javascript:alert(1)',
      'data:image/svg+xml,<svg onload=alert(1)>',
      '//evil.example/a.png',
      'file:///etc/passwd',
      'a b',
      ''
    ])
      expect(safeImageSrc(bad)).toBeNull()
    expect(safeImageSrc(undefined)).toBeNull()
  })
})
