import { describe, expect, test } from 'bun:test'
import { jsDoc, jsString, lineComment, luauDoc, luauString, oneLine } from './escape.ts'

const HOSTILE = [
  'a\nb',
  'a\rb',
  'a\r\nb',
  'a\u2028b',
  'a\u2029b',
  'a\u0085b',
  'a"b',
  "a'b",
  'a\\b',
  'a*/b',
  'a]]b',
  'a--]]b',
  'a\u0000b',
  'a\u001fb',
  'a\u007fb',
  'a\tb',
  'a`b${1}',
  '"; process.exit(1); //',
  '\n--[[ x ]]\nlocal x = os.exit()'
]

describe('a JavaScript string literal', () => {
  test('round-trips every hostile string and is one line', () => {
    for (const text of HOSTILE) {
      const literal = jsString(text)
      expect(literal, JSON.stringify(text)).not.toMatch(/[\n\r\u2028\u2029]/)
      expect(new Function(`return ${literal}`)(), JSON.stringify(text)).toBe(text)
    }
  })
})

describe('a Luau string literal', () => {
  /** Read a literal back with the rules of Luau's lexer for the escapes the routine writes. */
  const unquote = (literal: string): string => {
    expect(literal.startsWith('"') && literal.endsWith('"')).toBe(true)
    let out = ''
    const body = literal.slice(1, -1)
    for (let i = 0; i < body.length; i++) {
      const c = body[i] as string
      if (c !== '\\') {
        expect(c, 'an unescaped quote or line break would end the literal').not.toMatch(/["\n\r]/)
        out += c
        continue
      }
      const next = body[++i] as string
      if (next === 'n') out += '\n'
      else if (next === 'r') out += '\r'
      else if (next === 't') out += '\t'
      else if (next === '"' || next === '\\') out += next
      else {
        const digits = body.slice(i, i + 3)
        expect(digits).toMatch(/^\d{3}$/)
        out += String.fromCodePoint(Number(digits))
        i += 2
      }
    }
    return out
  }
  test('round-trips every hostile string and is one line', () => {
    for (const text of HOSTILE) {
      const literal = luauString(text)
      expect(literal, JSON.stringify(text)).not.toMatch(/[\n\r]/)
      expect(unquote(literal), JSON.stringify(text)).toBe(text)
    }
  })
  test('has no `\\u` escape (Luau reads only `\\u{...}`)', () => {
    expect(luauString('\u0000\u001f')).toBe('"\\000\\031"')
  })
})

describe('comments', () => {
  test('a one-line comment holds one line whatever the text held', () => {
    for (const text of HOSTILE) expect(lineComment(text), JSON.stringify(text)).not.toMatch(/[\n\r\u2028\u2029\u0085]/)
    expect(oneLine('a\r\nb\u2028c')).toBe('a b c')
  })
  test('a doc comment cannot be ended early and every line stays inside it', () => {
    for (const text of HOSTILE) {
      const doc = jsDoc(text, '  ')
      // the only `*/` is the last thing in the comment
      expect(doc.indexOf('*/'), JSON.stringify(text)).toBe(doc.lastIndexOf('*/'))
      expect(doc.trimEnd().endsWith('*/')).toBe(true)
      for (const line of doc.trimEnd().split('\n')) expect(line.trimStart()).toMatch(/^(\/\*\*|\*|\*\/|\/\*\* .* \*\/)/)
    }
    expect(jsDoc(undefined, '')).toBe('')
    expect(jsDoc('one line', '')).toBe('/** one line */\n')
  })
  test('Luau doc lines all start with the comment marker', () => {
    for (const text of HOSTILE) {
      for (const line of luauDoc(text, '  ').trimEnd().split('\n')) expect(line, JSON.stringify(text)).toMatch(/^ {2}---( |$)/)
    }
    expect(luauDoc(undefined, '')).toBe('')
  })
})
