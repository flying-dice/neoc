import { describe, expect, test } from 'bun:test'
import { cleanBody, cleanTitle } from './notificationText'

describe('cleanTitle', () => {
  test('plain text is left alone', () => {
    expect(cleanTitle('Backup finished')).toBe('Backup finished')
  })

  test('line breaks, tabs and other controls become single spaces, so the title is one line', () => {
    expect(cleanTitle('Backup\nfinished')).toBe('Backup finished')
    expect(cleanTitle('a\r\nb c d\u0085e')).toBe('a b c d e')
    expect(cleanTitle('a\t\tb\u0000c\u001bd\u007fe\u009ff')).toBe('a b c d e f')
    expect(cleanTitle('  too   many \n\n spaces  ')).toBe('too many spaces')
  })

  test('bidirectional controls are removed, so a title cannot reorder the text around it', () => {
    expect(cleanTitle('‮gnp.exe‬')).toBe('gnp.exe')
    expect(cleanTitle('a⁦b⁧c⁨d⁩e‎f‏g؜h‪i‫j‭k')).toBe('abcdefghijk')
  })

  test('characters that draw nothing are removed', () => {
    expect(cleanTitle('Back​up‌‍⁠﻿ fin­ished')).toBe('Backup finished')
    expect(cleanTitle('x\u{E0041}\u{E0042}y')).toBe('xy')
  })

  test('a title with nothing visible is empty, for the caller to name the app instead', () => {
    expect(cleanTitle('​​​')).toBe('')
    expect(cleanTitle('\n\t ‮⁣ㅤ')).toBe('')
    expect(cleanTitle('')).toBe('')
  })

  test('markup is just characters: it is drawn as text by the caller, not changed here', () => {
    expect(cleanTitle('<img src=x onerror=alert(1)>')).toBe('<img src=x onerror=alert(1)>')
  })

  test('letters of other scripts and emoji survive', () => {
    expect(cleanTitle('اكتمل النسخ 完了 ✅ 🙂')).toBe('اكتمل النسخ 完了 ✅ 🙂')
  })
})

describe('cleanBody', () => {
  test('line breaks stay, in any of their spellings', () => {
    expect(cleanBody('one\ntwo\r\nthree\rfour five')).toBe('one\ntwo\nthree\nfour\nfive')
  })

  test('at most one blank line in a row, white space per line collapsed, ends trimmed', () => {
    expect(cleanBody('\n\n a   b \n\n\n\n c \n\n')).toBe('a b\n\nc')
  })

  test('controls become spaces; bidi and zero-width characters go', () => {
    expect(cleanBody('a\u0007b\tc‮d​e')).toBe('a b cde')
  })

  test('a body with nothing visible is empty', () => {
    expect(cleanBody('​\n ‮\n')).toBe('')
  })
})
