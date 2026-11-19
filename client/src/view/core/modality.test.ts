import { afterEach, describe, expect, test } from 'bun:test'
import { inputModality, noteInput, pressesFocusedControl } from './modality'

type Mods = Partial<Record<'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey', boolean>>
const keys = (key: string, mods: Mods = {}): Parameters<typeof pressesFocusedControl>[0] => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods
})
const button = { matches: (sel: string) => sel.includes('button') } as unknown as Element
const section = { matches: () => false } as unknown as Element

afterEach(() => noteInput({ type: 'keydown', key: 'Tab' }))

describe('input modality', () => {
  test('a pointerdown makes it a pointer; Tab makes it the keyboard; typing makes it typing', () => {
    noteInput({ type: 'pointerdown' })
    expect(inputModality()).toBe('pointer')
    noteInput({ type: 'keydown', key: '+' }) // typing after a click does not make the click a Tab
    noteInput({ type: 'keydown', key: '3' })
    expect(inputModality()).toBe('typing')
    noteInput({ type: 'keydown', key: 'Tab' })
    expect(inputModality()).toBe('keyboard')
    noteInput({ type: 'keydown', key: '8' }) // tabbed to a key, then typed a sum: Enter finishes it
    expect(inputModality()).toBe('typing')
    noteInput({ type: 'keydown', key: 'Tab' })
    expect(inputModality()).toBe('keyboard')
    noteInput({ type: 'keydown', key: 'Backspace' })
    expect(inputModality()).toBe('typing')
  })
  test('Enter, Space, the arrows, modifiers and shortcuts with a command key do not count as typing', () => {
    noteInput({ type: 'keydown', key: 'Tab' })
    for (const key of ['Enter', ' ', 'ArrowDown', 'Shift', 'Meta', 'Control'])
      noteInput({ type: 'keydown', key })
    noteInput({ type: 'keydown', key: 'c', metaKey: true })
    noteInput({ type: 'keydown', key: 'a', ctrlKey: true })
    expect(inputModality()).toBe('keyboard')
  })
})

describe('Enter and Space on a focused control', () => {
  test('press a control that was reached with the keyboard', () => {
    expect(pressesFocusedControl(keys('Enter'), button, 'keyboard')).toBe(true)
    expect(pressesFocusedControl(keys(' '), button, 'keyboard')).toBe(true)
  })
  test('are left to the window shortcut after a click or touch, whatever the browser says about :focus-visible', () => {
    expect(pressesFocusedControl(keys('Enter'), button, 'pointer')).toBe(false)
    expect(pressesFocusedControl(keys(' '), button, 'pointer')).toBe(false)
  })
  test('are left to the window shortcut once the person has typed on the key they tabbed to', () => {
    expect(pressesFocusedControl(keys('Enter'), button, 'typing')).toBe(false)
  })
  test('only Enter and Space, only with no modifier, only on a pressable control', () => {
    expect(pressesFocusedControl(keys('7'), button, 'keyboard')).toBe(false)
    expect(pressesFocusedControl(keys('Enter', { metaKey: true }), button, 'keyboard')).toBe(false)
    expect(pressesFocusedControl(keys('Enter', { ctrlKey: true }), button, 'keyboard')).toBe(false)
    expect(pressesFocusedControl(keys('Enter', { altKey: true }), button, 'keyboard')).toBe(false)
    expect(pressesFocusedControl(keys('Enter', { shiftKey: true }), button, 'keyboard')).toBe(false)
    expect(pressesFocusedControl(keys('Enter'), section, 'keyboard')).toBe(false)
    expect(pressesFocusedControl(keys('Enter'), null, 'keyboard')).toBe(false)
  })
})
