import { describe, expect, test } from 'bun:test'
import { ariaShortcut, parseShortcut, ShortcutRegistry, type KeyEventLike } from './shortcuts'

const key = (k: string, mods: Partial<KeyEventLike> = {}): KeyEventLike => ({
  key: k,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods
})

describe('shortcut parsing', () => {
  test('modifiers and aliases', () => {
    expect(parseShortcut('cmd+n')).toEqual({
      key: 'n',
      cmd: true,
      ctrl: false,
      alt: false,
      shift: false
    })
    expect(parseShortcut('Shift+Option+Return')).toEqual({
      key: 'enter',
      cmd: false,
      ctrl: false,
      alt: true,
      shift: true
    })
    expect(parseShortcut('esc')?.key).toBe('escape')
    expect(parseShortcut('')).toBeNull()
    expect(parseShortcut('hyper+x')).toBeNull()
  })
  test('"+" is a key as well as the separator', () => {
    expect(parseShortcut('+')?.key).toBe('+')
    expect(parseShortcut('cmd++')).toEqual({
      key: '+',
      cmd: true,
      ctrl: false,
      alt: false,
      shift: false
    })
    expect(parseShortcut('shift+')).toEqual({
      key: '+',
      cmd: false,
      ctrl: false,
      alt: false,
      shift: true
    })
    expect(parseShortcut('a+')).toBeNull() // "a" is not a modifier
    expect(ariaShortcut('+')).toBe('+')
  })
  test('a bare "+" fires on the Plus key', () => {
    const r = new ShortcutRegistry()
    let hits = 0
    r.register('+', () => hits++)
    expect(r.dispatch(key('+', { shiftKey: true }), { apple: true, editable: false })).toBe(true)
    expect(hits).toBe(1)
  })
})

describe('the window shortcut registry', () => {
  test('cmd is Meta on Apple platforms and Ctrl elsewhere', () => {
    const r = new ShortcutRegistry()
    let hits = 0
    r.register('cmd+n', () => hits++)
    expect(r.dispatch(key('n', { metaKey: true }), { apple: true, editable: false })).toBe(true)
    expect(r.dispatch(key('n', { ctrlKey: true }), { apple: true, editable: false })).toBe(false)
    expect(r.dispatch(key('n', { ctrlKey: true }), { apple: false, editable: false })).toBe(true)
    expect(hits).toBe(2)
  })
  test('a bare key never steals typing from an editable control; a command key still does', () => {
    const r = new ShortcutRegistry()
    const ran: string[] = []
    r.register('7', () => ran.push('seven'))
    r.register('cmd+s', () => ran.push('save'))
    expect(r.dispatch(key('7'), { apple: true, editable: true })).toBe(false)
    expect(r.dispatch(key('s', { metaKey: true }), { apple: true, editable: true })).toBe(true)
    expect(r.dispatch(key('7'), { apple: true, editable: false })).toBe(true)
    expect(ran).toEqual(['save', 'seven'])
  })
  test('unregistering, disabled shortcuts and the command list', () => {
    const r = new ShortcutRegistry()
    let on = false
    const off = r.register(
      'escape',
      () => undefined,
      () => on
    )
    expect(r.list()).toEqual(['escape'])
    expect(r.dispatch(key('Escape'), { apple: true, editable: false })).toBe(false)
    on = true
    expect(r.dispatch(key('Escape'), { apple: true, editable: false })).toBe(true)
    off()
    expect(r.dispatch(key('Escape'), { apple: true, editable: false })).toBe(false)
    expect(r.list()).toEqual([])
  })
  test('modifiers must match exactly', () => {
    const r = new ShortcutRegistry()
    r.register('cmd+n', () => undefined)
    expect(
      r.dispatch(key('n', { metaKey: true, shiftKey: true }), { apple: true, editable: false })
    ).toBe(true) // printable keys ignore shift
    expect(
      r.dispatch(key('n', { metaKey: true, altKey: true }), { apple: true, editable: false })
    ).toBe(false)
  })
})
