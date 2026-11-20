/**
 * Avatar and Image helpers: initials from a name, a stable fallback colour for a name, and which image sources
 * the kit will load at all. An image URL is app-supplied (untrusted): only `https:` and same-origin paths load.
 */
import type { PaletteToken } from './contrast'

/** Two letters at most: the first letters of the first and last words ("Ada Lovelace" -> "AL", "Ada" -> "A"). */
export function initials(name: string): string {
  const words = name
    .normalize('NFKC')
    .split(/[\s._-]+/u)
    .filter((w) => /[\p{L}\p{N}]/u.test(w))
  if (words.length === 0) return ''
  const first = [...words[0]!.replace(/^[^\p{L}\p{N}]+/u, '')][0] ?? ''
  const last = words.length > 1 ? ([...words[words.length - 1]!.replace(/^[^\p{L}\p{N}]+/u, '')][0] ?? '') : ''
  return (first + last).toLocaleUpperCase()
}

/** The tokens an automatic avatar colour chooses from (saturated, readable under white or black ink). */
const AUTO: readonly PaletteToken[] = [
  'blue',
  'green',
  'orange',
  'purple',
  'pink',
  'indigo',
  'teal',
  'brown'
]

/** A stable palette token for a name: the same person always gets the same colour. */
export function colorForName(name: string): PaletteToken {
  let h = 5381
  for (const ch of name) h = ((h << 5) + h + (ch.codePointAt(0) ?? 0)) | 0
  return AUTO[Math.abs(h) % AUTO.length]!
}

/** An image source the kit may load: an https URL, or a same-origin absolute path. Everything else is refused. */
export function safeImageSrc(src: unknown): string | null {
  if (typeof src !== 'string') return null
  const s = src.trim()
  if (s === '' || /[\u0000-\u001f\u007f\s]/.test(s)) return null
  if (s.startsWith('/') && !s.startsWith('//') && !s.includes('\\')) return s
  try {
    return new URL(s).protocol === 'https:' ? s : null
  } catch {
    return null
  }
}

export type AvatarStatus = 'online' | 'busy' | 'away' | 'offline'
export const AVATAR_SIZES = { xs: 24, sm: 32, md: 40, lg: 56, xl: 80 } as const
export type AvatarSize = keyof typeof AVATAR_SIZES
