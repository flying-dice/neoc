import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import {
  PALETTE_TOKENS,
  contrastRatio,
  cssColor,
  formatColorValue,
  inkFor,
  luminance,
  normaliseHex,
  onColor,
  onColorRatio,
  parseColorValue,
  parseHex,
  tokenInk
} from './contrast'

describe('data colour contrast', () => {
  test('parses #rgb and #rrggbb, rejects the rest', () => {
    expect(parseHex('#fff')).toEqual([255, 255, 255])
    expect(parseHex('#0a84FF')).toEqual([10, 132, 255])
    for (const bad of ['red', '#12', '#12345', '#gggggg', 'rgb(0,0,0)', '']) expect(parseHex(bad)).toBeNull()
    expect(normaliseHex('#ABC')).toBe('#aabbcc')
    expect(normaliseHex('javascript:1')).toBeNull()
  })

  test('black and white have the extreme luminance and contrast', () => {
    expect(luminance([0, 0, 0])).toBe(0)
    expect(luminance([255, 255, 255])).toBeCloseTo(1, 5)
    expect(contrastRatio(0, 1)).toBeCloseTo(21, 5)
  })

  test('ink on a hex fill is white on dark, black on light', () => {
    expect(onColor('#000080')).toBe('#ffffff')
    expect(onColor('#ffd60a')).toBe('#000000')
    expect(onColor('nope')).toBeNull()
  })

  test('ink on every hex in a coarse RGB cube holds 4.5:1', () => {
    for (let r = 0; r < 256; r += 51)
      for (let g = 0; g < 256; g += 51)
        for (let b = 0; b < 256; b += 51) {
          const hex = `#${[r, g, b].map((c) => c.toString(16).padStart(2, '0')).join('')}`
          expect(onColorRatio(hex)).toBeGreaterThanOrEqual(4.5)
        }
  })

  test('only palette tokens and hex values are colours', () => {
    expect(cssColor('teal')).toBe('var(--ios-teal)')
    expect(cssColor('#ABCDEF')).toBe('#abcdef')
    expect(cssColor('url(javascript:1)')).toBeUndefined()
    expect(cssColor('red; background: url(x)')).toBeUndefined()
    expect(inkFor('#ffffff')).toBe('#000000')
    expect(inkFor('blue')).toBe(tokenInk('var(--ios-blue)'))
    expect(inkFor('bogus')).toBeUndefined()
  })
})

describe('colour values', () => {
  test('tokens, #rgb, #rrggbb and #rrggbbaa parse; the rest does not', () => {
    expect(parseColorValue('blue')).toEqual({ kind: 'token', token: 'blue' })
    expect(parseColorValue('#ABC')).toEqual({ kind: 'hex', hex: '#aabbcc', alpha: 1 })
    expect(parseColorValue('#ff000080')).toMatchObject({ kind: 'hex', hex: '#ff0000' })
    expect(parseColorValue('#ff000080')).toHaveProperty('alpha', 128 / 255)
    for (const bad of ['', 'rebeccapurple', '#12', 'javascript:1', 42, null]) expect(parseColorValue(bad)).toBeNull()
  })

  test('format is the inverse: opaque hex has no alpha digits', () => {
    expect(formatColorValue({ kind: 'token', token: 'teal' })).toBe('teal')
    expect(formatColorValue({ kind: 'hex', hex: '#112233', alpha: 1 })).toBe('#112233')
    expect(formatColorValue({ kind: 'hex', hex: '#112233', alpha: 0.5 })).toBe('#11223380')
    expect(formatColorValue({ kind: 'hex', hex: '#112233', alpha: 2 })).toBe('#112233')
    expect(formatColorValue({ kind: 'hex', hex: '#112233', alpha: -1 })).toBe('#11223300')
  })
})

// The token form picks ink in CSS, per scheme. Audit it: replay the rule on every token's value in both schemes.
function oklabL([r, g, b]: [number, number, number]): number {
  const lin = (c: number): number => {
    const s = c / 255
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  const [lr, lg, lb] = [lin(r), lin(g), lin(b)]
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
  return 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
}

function tokenValues(): Record<string, { light: string; dark: string }> {
  const css = readFileSync(new URL('./styles/tokens.css', import.meta.url), 'utf8')
  const out: Record<string, { light: string; dark: string }> = {}
  for (const name of PALETTE_TOKENS) {
    const re = new RegExp(`--ios-${name}:\\s*(?:light-dark\\((#[0-9a-f]{6}),\\s*(#[0-9a-f]{6})\\)|(#[0-9a-f]{6}));`)
    const m = re.exec(css)
    if (!m) throw new Error(`token --ios-${name} not found in tokens.css`)
    out[name] = { light: m[1] ?? m[3]!, dark: m[2] ?? m[3]! }
  }
  return out
}

describe('palette token ink (CSS rule replayed on tokens.css)', () => {
  const THRESHOLD = 0.59
  for (const [name, values] of Object.entries(tokenValues())) {
    for (const scheme of ['light', 'dark'] as const) {
      test(`${name} in ${scheme} reads at 4.5:1`, () => {
        const rgb = parseHex(values[scheme])!
        const ink = oklabL(rgb) < THRESHOLD ? 1 : 0
        expect(contrastRatio(luminance(rgb), ink)).toBeGreaterThanOrEqual(4.5)
      })
    }
  }
  test('the CSS rule uses the audited threshold', () => {
    expect(tokenInk('var(--ios-blue)')).toContain(`(${THRESHOLD} - l)`)
  })
})

// ── Chrome that must be seen: the focus ring, the resize grip and the "···" dots (WCAG 1.4.11, 3:1), the limit label (1.4.3, 4.5:1)

/** A token declared in tokens.css as `#hex` or `light-dark(#hex, #hex)`, in each theme. */
function colourToken(name: string): { light: string; dark: string } {
  const css = readFileSync(new URL('./styles/tokens.css', import.meta.url), 'utf8')
  const m = new RegExp(`${name}:\\s*(?:light-dark\\((#[0-9a-f]{6}),\\s*(#[0-9a-f]{6})\\)|(#[0-9a-f]{6}));`).exec(css)
  if (!m) throw new Error(`${name} is not a plain hex or light-dark(hex, hex) token in tokens.css`)
  return { light: m[1] ?? m[3]!, dark: m[2] ?? m[3]! }
}
const ratio = (a: string, b: string): number => contrastRatio(luminance(parseHex(a)!), luminance(parseHex(b)!))

describe('the focus ring', () => {
  const ring = colourToken('--focus-ring')
  const surfaces = ['--bg', '--bg-elevated', '--bg-grouped-elevated'].map((name) => [name, colourToken(name)] as const)
  for (const scheme of ['light', 'dark'] as const) {
    for (const [name, surface] of surfaces) {
      test(`holds 3:1 on ${name} in ${scheme}`, () => {
        expect(ratio(ring[scheme], surface[scheme])).toBeGreaterThanOrEqual(3)
      })
    }
    test(`holds 3:1 on a black body (the Calculator's) in ${scheme}`, () => {
      expect(ratio(ring[scheme], '#000000')).toBeGreaterThanOrEqual(3)
    })
  }

  test('is the one ring: the base style uses it, solid, and does not dim or follow the accent', () => {
    const base = readFileSync(new URL('./styles/base.css', import.meta.url), 'utf8')
    expect(base).toContain('outline: 3px solid var(--focus-ring);')
    expect(base).not.toMatch(/:focus-visible\s*{[^}]*--tint/)
  })
})

describe('window chrome', () => {
  const grip = colourToken('--window-grip')
  for (const body of ['--bg-grouped-elevated', '--bg-elevated', 'a black body'] as const) {
    for (const scheme of ['light', 'dark'] as const) {
      test(`the resize grip and the "···" dots hold 3:1 on ${body} in ${scheme}`, () => {
        const surface = body === 'a black body' ? '#000000' : colourToken(body)[scheme]
        expect(ratio(grip[scheme], surface)).toBeGreaterThanOrEqual(3)
      })
    }
  }

  test('the limit label (13 px) holds 4.5:1 whatever the accent: it does not use the accent', () => {
    const label = colourToken('--label')
    const bg = colourToken('--bg-elevated')
    for (const scheme of ['light', 'dark'] as const) expect(ratio(label[scheme], bg[scheme])).toBeGreaterThanOrEqual(4.5)
    const window = readFileSync(new URL('./organisms/Window.svelte', import.meta.url), 'utf8')
    const rule = /\.limit-note\s*{[^}]*}/.exec(window)![0]
    expect(rule).toContain('background: var(--label)')
    expect(rule).toContain('color: var(--bg-elevated)')
    expect(rule).not.toMatch(/var\(--(tint|on-tint)/)
  })

  test('the "···" dots take the grip colour and the button is at least 44 x 24', () => {
    const dots = readFileSync(new URL('./atoms/MultitaskingDots.svelte', import.meta.url), 'utf8')
    expect(dots).toContain('background: var(--window-grip)')
    expect(dots).toMatch(/width: 44px;\s*height: 24px;/)
  })
})
