/**
 * What an app wrote in a notification is shown to the person as text, and an app can write anything: a newline that
 * pushes the title onto a second line, control characters, an override that reverses the app's name next to it, or
 * nothing visible at all (zero-width characters). The host checks length and blankness only, so the shell cleans what
 * it draws. Nothing here makes markup: the result is a string for a text node.
 */

/** Bidirectional controls: marks, embeddings, overrides and isolates (U+061C, U+200E/F, U+202A-E, U+2066-9). */
const BIDI = /[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/g

/**
 * Characters that draw nothing: zero-width space, non-joiner and joiner, word joiner and the invisible math operators,
 * the byte order mark, a soft hyphen, the combining grapheme joiner, the Mongolian vowel separator, the Hangul fillers
 * and the invisible tag characters (U+E0000-E007F). A joiner inside an emoji sequence is lost with them: the emoji
 * shows as its parts, which is the safer way to be wrong.
 */
// eslint-disable-next-line no-misleading-character-class -- U+034F is a combining character, listed to be removed
const INVISIBLE = /[\u00AD\u034F\u115F\u1160\u180E\u200B-\u200D\u2060-\u2064\u3164\uFEFF\uFFA0]/g
const TAGS = /[\u{E0000}-\u{E007F}]/gu

/** Line breaks of every kind (CR, LF, NEL, line and paragraph separator), which a title must not contain. */
const LINE_BREAK = /\r\n|[\r\n\u0085\u2028\u2029]/g
/** C0 and C1 controls other than a line break (which `LINE_BREAK` has already turned into one), and DEL. */
// eslint-disable-next-line no-control-regex -- the controls are what it matches
const CONTROL = /[\u0000-\u0009\u000B\u000C\u000E-\u001F\u007F-\u009F]/g

const strip = (text: string): string =>
  text.replace(BIDI, '').replace(INVISIBLE, '').replace(TAGS, '')

/**
 * A title for one line: every line break and control character becomes a space, runs of white space collapse to one
 * space, and the ends are trimmed. Returns `''` when nothing visible is left; the caller names the app instead.
 */
export function cleanTitle(title: string): string {
  return strip(title).replace(LINE_BREAK, ' ').replace(CONTROL, ' ').replace(/\s+/g, ' ').trim()
}

/**
 * A body for a few lines: line breaks stay (as `\n`), at most one blank line in a row, controls become spaces, the
 * white space of each line collapses, and the ends are trimmed. Returns `''` when nothing visible is left.
 */
export function cleanBody(body: string): string {
  return strip(body)
    .replace(LINE_BREAK, '\n')
    .replace(CONTROL, ' ')
    .split('\n')
    .map((line) => line.replace(/[^\S\n]+/g, ' ').trim())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}
