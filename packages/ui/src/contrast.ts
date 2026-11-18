/**
 * Data colours (a task label, an avatar, a swatch) are content, not styling: they arrive as a palette token or a
 * `#rrggbb`. Text on top of one must still read at 4.5:1, so the kit picks the foreground from the fill.
 *
 * A `#rrggbb` fill is measured here (WCAG relative luminance). A palette token resolves per colour scheme in CSS,
 * so it cannot be measured in script: `tokenInk` is the CSS `oklch(from <fill> ...)` form that flips black or
 * white at the lightness where the two contrast equally. `scripts` for the audit of every token in both schemes
 * live next to the tests (`contrast.test.ts`).
 */

export const PALETTE_TOKENS = [
  'blue',
  'red',
  'green',
  'orange',
  'yellow',
  'purple',
  'pink',
  'indigo',
  'teal',
  'mint',
  'cyan',
  'brown',
  'gray',
  'gray2',
  'gray3',
  'gray4',
  'gray5',
  'gray6'
] as const

export type PaletteToken = (typeof PALETTE_TOKENS)[number]

export const isPaletteToken = (v: unknown): v is PaletteToken =>
  typeof v === 'string' && (PALETTE_TOKENS as readonly string[]).includes(v)

const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i

/** `#rgb` or `#rrggbb` to `[r, g, b]` (0..255), or null. */
export function parseHex(value: string): [number, number, number] | null {
  const m = HEX.exec(value.trim())
  if (!m) return null
  let h = m[1]!
  if (h.length === 3) h = [...h].map((c) => c + c).join('')
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number]
}

/** WCAG 2.x relative luminance of an sRGB colour (0..1). */
export function luminance([r, g, b]: [number, number, number]): number {
  const lin = (c: number): number => {
    const s = c / 255
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/** WCAG contrast ratio of two luminances (1..21). */
export const contrastRatio = (a: number, b: number): number =>
  (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)

/** Black or white, whichever reads better on `hex`; null when `hex` is not a colour. */
export function onColor(hex: string): '#000000' | '#ffffff' | null {
  const rgb = parseHex(hex)
  if (!rgb) return null
  const lum = luminance(rgb)
  return contrastRatio(lum, 1) >= contrastRatio(lum, 0) ? '#ffffff' : '#000000'
}

/** The contrast the chosen foreground achieves on `hex` (for tests and audits). */
export function onColorRatio(hex: string): number {
  const rgb = parseHex(hex)
  if (!rgb) return 0
  const lum = luminance(rgb)
  return Math.max(contrastRatio(lum, 1), contrastRatio(lum, 0))
}

/** A `#rrggbb` normalised to lower case six digits, or null. */
export function normaliseHex(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const rgb = parseHex(value)
  return rgb ? `#${rgb.map((c) => c.toString(16).padStart(2, '0')).join('')}` : null
}

/** The CSS colour of a data colour: a palette token's variable, or a normalised hex. Anything else is undefined. */
export function cssColor(value: unknown): string | undefined {
  if (isPaletteToken(value)) return `var(--ios-${value})`
  return normaliseHex(value) ?? undefined
}

/**
 * Text colour for a fill that is a palette token (CSS only, because the token's value depends on the scheme):
 * white below OKLCH lightness 0.59, black above. Equal contrast sits near 0.57 (every token passes 4.5:1 for 0.57 to 0.61); 0.59 keeps white off the mid-light
 * tokens, where black wins.
 */
export const tokenInk = (fill: string): string =>
  `oklch(from ${fill} clamp(0, (0.59 - l) * 1000, 1) 0 0)`

/** Ink for any data colour: exact for a hex, the relative-colour form for a token variable. */
export function inkFor(value: unknown): string | undefined {
  const css = cssColor(value)
  if (!css) return undefined
  return css.startsWith('#') ? (onColor(css) ?? undefined) : tokenInk(css)
}

/** A colour value as the ColorPicker handles it: a palette token, or `#rrggbb` with an optional alpha (`#rrggbbaa`). */
export type ColorValue =
  | { kind: 'token'; token: PaletteToken }
  | { kind: 'hex'; hex: string; alpha: number }

const HEX8 = /^#([0-9a-f]{6})([0-9a-f]{2})$/i

/** Parse a wire colour: a palette token, `#rgb`, `#rrggbb` or `#rrggbbaa`. Null for anything else. */
export function parseColorValue(value: unknown): ColorValue | null {
  if (isPaletteToken(value)) return { kind: 'token', token: value }
  if (typeof value !== 'string') return null
  const m8 = HEX8.exec(value.trim())
  if (m8) return { kind: 'hex', hex: `#${m8[1]!.toLowerCase()}`, alpha: parseInt(m8[2]!, 16) / 255 }
  const hex = normaliseHex(value)
  return hex ? { kind: 'hex', hex, alpha: 1 } : null
}

/** The wire string: the token, `#rrggbb` when opaque, `#rrggbbaa` otherwise. */
export function formatColorValue(c: ColorValue): string {
  if (c.kind === 'token') return c.token
  const a = Math.round(Math.min(1, Math.max(0, c.alpha)) * 255)
  return a === 255 ? c.hex : `${c.hex}${a.toString(16).padStart(2, '0')}`
}
