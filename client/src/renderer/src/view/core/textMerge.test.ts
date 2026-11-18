import { describe, expect, test } from 'bun:test'
import { diff1, mapCaret, merge3, rebase } from './textMerge'

describe('diff1', () => {
  test('is the single edit between two strings', () => {
    expect(diff1('hello world', 'hello brave world')).toEqual({ at: 6, del: 0, ins: 'brave ' })
    expect(diff1('abcdef', 'abXef')).toEqual({ at: 2, del: 2, ins: 'X' })
    expect(diff1('same', 'same')).toEqual({ at: 4, del: 0, ins: '' })
  })
})

describe('merge3 (the host rule the client reproduces when it rebases)', () => {
  test('one side unchanged takes the other', () => {
    expect(merge3('a', 'a', 'b')).toEqual({ value: 'b', conflict: false })
    expect(merge3('a', 'b', 'a')).toEqual({ value: 'b', conflict: false })
    expect(merge3('a', 'b', 'b')).toEqual({ value: 'b', conflict: false })
  })
  test('non-overlapping hunks compose', () => {
    expect(merge3('milk', 'milk\n- oat milk', 'Milk')).toEqual({
      value: 'Milk\n- oat milk',
      conflict: false
    })
  })
  test('two insertions at the same point compose, theirs first', () => {
    expect(merge3('', '+agent', 'user')).toEqual({ value: '+agentuser', conflict: false })
  })
  test('overlapping edits resolve to mine and report a conflict', () => {
    expect(merge3('buy milk', 'buy tea', 'buy soy')).toEqual({ value: 'buy soy', conflict: true })
  })
})

describe('mapCaret', () => {
  test('a caret before the change stays; after it shifts by the length difference', () => {
    expect(mapCaret('hello world', 'hello brave world', 2)).toBe(2)
    expect(mapCaret('hello world', 'hello brave world', 9)).toBe(15)
  })
  test('a caret inside a replaced span lands at the end of the replacement', () => {
    expect(mapCaret('abcdef', 'abXYef', 3)).toBe(4)
  })
  test('a caret at the end of an appended value stays at the end of what it was', () => {
    expect(mapCaret('milk', 'milk\n- oat', 4)).toBe(4)
  })
})

describe('rebase', () => {
  test('keystrokes typed while a write was in flight survive a merged ack', () => {
    // sent "Milk", the host merged an agent line, the user then typed "!" at the start of the draft.
    expect(rebase('Milk', 'Milk\n- oat', '!Milk')).toBe('!Milk\n- oat')
  })
  test('an ack equal to what was sent leaves the draft alone', () => {
    expect(rebase('a', 'a', 'ab')).toBe('ab')
  })
})
