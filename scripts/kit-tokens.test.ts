import { describe, expect, test } from 'bun:test'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { buildTokensCss } from './kit-tokens.ts'

const components = join(import.meta.dirname, '../packages/kit/src/components')
const sources = readdirSync(components)
  .filter((f) => f.endsWith('.ts') && !f.endsWith('.stories.ts') && !f.endsWith('.test.ts'))
  .map((f) => ({ file: f, text: readFileSync(join(components, f), 'utf8') }))

const declared = (css: string): Set<string> => new Set([...css.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]!))

describe('the token sheet', () => {
  const tokens = buildTokensCss()
  const shell = readFileSync(join(import.meta.dirname, '../packages/ui/src/styles/tokens.css'), 'utf8')

  test('carries every custom property of the shell, so a frame and the shell cannot drift', () => {
    const own = declared(tokens)
    const missing = [...declared(shell)].filter((name) => !own.has(name))
    expect(missing).toEqual([])
  })

  test('keys the dark theme on data-theme, as the frame document does', () => {
    expect(tokens).toContain("[data-theme='dark']")
    expect(tokens).toContain('light-dark(')
  })

  test('follows the system setting when nothing sets data-theme (a standalone app), and data-theme wins when it is set', () => {
    expect(tokens).toMatch(/:root\s*\{\s*color-scheme:\s*light dark;/)
    expect(tokens).toMatch(/\[data-theme='light'\]\s*\{\s*color-scheme:\s*light;/)
    expect(tokens).toMatch(/\[data-theme='dark'\]\s*\{\s*color-scheme:\s*dark;/)
  })

  test('carries the base reset', () => {
    expect(tokens).toContain(':focus-visible')
    expect(tokens).toContain('prefers-reduced-motion')
  })

  test('every var(--x) a component reads, without a fallback, is a token or a property a component sets', () => {
    const known = declared(tokens)
    for (const { text } of sources) for (const name of declared(text)) known.add(name)
    const unknown: string[] = []
    for (const { file, text } of sources) {
      for (const m of text.matchAll(/var\((--[\w-]+)\s*([,)])/g)) {
        const name = m[1]!
        const hasFallback = m[2] === ','
        if (!hasFallback && !known.has(name)) unknown.push(`${file}: ${name}`)
      }
    }
    expect(unknown).toEqual([])
  })
})

describe('the components', () => {
  test('use no innerHTML, eval or external URL (the frame allows same-host scripts only)', () => {
    for (const { file, text } of sources) {
      const code = text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '')
      expect({ file, bad: /\binnerHTML\b|\beval\s*\(|new Function\b|https?:\/\//.test(code) }).toEqual({ file, bad: false })
    }
  })

  test('every component defines a ui- tag and says what it is for', () => {
    for (const { file, text } of sources) {
      const m = text.match(/customElements\.define\('(ui-[\w-]+)'/)
      expect({ file, tag: m?.[1] ? true : false }).toEqual({ file, tag: true })
      expect({ file, tagged: text.includes(`@tag ${m?.[1]}`) }).toEqual({ file, tagged: true })
    }
  })
})
