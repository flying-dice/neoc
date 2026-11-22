/**
 * How system search decides that a name answers what was typed, and how well. Pure functions: no DOM, no state.
 *
 * Case and accents never matter ("cafe" finds "Café", "CALC" finds "Calculator"). A match is one of three kinds, best
 * first: the text starts with what was typed; every word typed starts a word of the text ("home scr" finds "Show Home
 * Screen", "screen home" too); every word typed is somewhere in the text ("ulat" finds "Calculator").
 */

/** How well a text answers a query: its start, the start of its words, or somewhere inside. Lower is better. */
export const MATCH = { prefix: 0, wordStart: 1, substring: 2 } as const
export type MatchRank = (typeof MATCH)[keyof typeof MATCH]

/** Text as it is compared: lower case, accents and other combining marks removed, spaces at the ends dropped. */
export function fold(text: string): string {
  return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().trim()
}

/** What separates the words of a name: spaces and the punctuation names are written with. */
const BETWEEN_WORDS = /[\s\-_./,:;()&+]+/

/** How well `text` answers `query`, or null when it does not (an empty query answers nothing). */
export function matchRank(query: string, text: string): MatchRank | null {
  const q = fold(query)
  const t = fold(text)
  if (!q || !t) return null
  if (t.startsWith(q)) return MATCH.prefix
  const tokens = q.split(/\s+/)
  const words = t.split(BETWEEN_WORDS).filter(Boolean)
  if (tokens.every((token) => words.some((word) => word.startsWith(token)))) return MATCH.wordStart
  if (tokens.every((token) => t.includes(token))) return MATCH.substring
  return null
}
