import { describe, expect, test } from 'bun:test'
import { isApplePlatform, isSearchShortcut, searchShortcut } from './shortcuts'

type Chord = Parameters<typeof isSearchShortcut>[0]
const chord = (extra: Partial<Chord> = {}): Chord => ({
  key: 'k',
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...extra
})

describe('isSearchShortcut', () => {
  test('Cmd+K on a Mac, Ctrl+K elsewhere', () => {
    expect(isSearchShortcut(chord({ metaKey: true }), true)).toBe(true)
    expect(isSearchShortcut(chord({ ctrlKey: true }), false)).toBe(true)
  })

  test('the other platform’s modifier is not it: Ctrl+K on a Mac stays the text field’s', () => {
    expect(isSearchShortcut(chord({ ctrlKey: true }), true)).toBe(false)
    expect(isSearchShortcut(chord({ metaKey: true }), false)).toBe(false)
    expect(isSearchShortcut(chord({ metaKey: true, ctrlKey: true }), true)).toBe(false)
  })

  test('Caps Lock does not matter; Shift, Alt, a held key and a plain K do', () => {
    expect(isSearchShortcut(chord({ key: 'K', metaKey: true }), true)).toBe(true)
    expect(isSearchShortcut(chord({ metaKey: true, shiftKey: true }), true)).toBe(false)
    expect(isSearchShortcut(chord({ metaKey: true, altKey: true }), true)).toBe(false)
    expect(isSearchShortcut(chord({ metaKey: true, repeat: true }), true)).toBe(false)
    expect(isSearchShortcut(chord(), true)).toBe(false)
    expect(isSearchShortcut(chord({ key: 'j', metaKey: true }), true)).toBe(false)
  })

  test('what assistive technology is told', () => {
    expect(searchShortcut(true)).toBe('Meta+K')
    expect(searchShortcut(false)).toBe('Control+K')
  })
})

describe('isApplePlatform', () => {
  test.each([
    [{ platform: 'MacIntel' }, true],
    [{ userAgentData: { platform: 'macOS' } }, true],
    [{ platform: 'iPad' }, true],
    [{ platform: 'Win32' }, false],
    [{ platform: 'Linux x86_64' }, false],
    [{ userAgentData: { platform: 'Windows' }, platform: 'Win32' }, false],
    [{}, false]
  ])('%p → %p', (nav, expected) => {
    expect(isApplePlatform(nav)).toBe(expected)
  })
})
