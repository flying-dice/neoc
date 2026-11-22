/**
 * What an app wrote in a notification is shown to the person as text, and an app can write anything: a newline that
 * pushes the title onto a second line, control characters, an override that reverses the app's name beside it, or
 * nothing visible at all. The host checks length and blankness only, so the shell cleans what it draws. Nothing here
 * makes markup: the result is a string for a text node.
 *
 * The classes are the platform's own Unicode properties, not a list kept by hand:
 *  - `Bidi_Control` characters are removed (the marks, embeddings, overrides and isolates). The text is shown with
 *    `dir="auto"` in its own isolated element, so its direction needs no help from them.
 *  - `Default_Ignorable_Code_Point` characters (zero-width space, word joiner, soft hyphen, the Hangul and Khmer
 *    fillers, Mongolian free variation selectors, the tag block, ...) draw nothing and are removed, except where they
 *    do their job: a zero-width joiner or non-joiner between two letters or pictographs (a family emoji, Persian
 *    and Indic words), a variation selector after the character it selects (the emoji form, a keycap), and a tag run
 *    that ends a flag of England, Scotland or Wales after its black flag.
 *  - Characters that are not default-ignorable but draw a blank anyway (the braille blank, the object replacement
 *    character) become spaces.
 *  - A title or body counts as having something to show only if it holds a letter, number, punctuation, symbol or
 *    mark: any other result is `''`, and the caller names the app instead.
 */

const BIDI_CONTROL = /\p{Bidi_Control}/u
const IGNORABLE = /\p{Default_Ignorable_Code_Point}/u
const CONTROL = /\p{Cc}/u
/** Draw nothing, though they are not default-ignorable: the braille blank and the object replacement character. */
const BLANK_GLYPH = /[\u2800\uFFFC]/u
const ZWNJ = '\u200C'
const ZWJ = '\u200D'
/** Before a joiner: a letter, a mark (an Indic virama), a pictograph, a skin tone or an emoji presentation selector. */
const JOINS_AFTER = /[\p{L}\p{M}\p{Extended_Pictographic}\p{Emoji_Modifier}\uFE0F]/u
const JOINS_BEFORE = /[\p{L}\p{M}\p{Extended_Pictographic}]/u
/** A variation selector selects a form of the character before it: an emoji, a keycap base, an ideograph. */
const SELECTS_AFTER = /[\p{Extended_Pictographic}\p{Emoji}\p{Ideographic}]/u
const VARIATION_SELECTOR = /[\uFE00-\uFE0F\u{E0100}-\u{E01EF}]/u
const TAG = /[\u{E0020}-\u{E007F}]/u
const PICTOGRAPH = /\p{Extended_Pictographic}/u
/** The tag letters and digits of a flag's subdivision code (`gbeng`), then the cancel tag. */
const SUBDIVISION_FLAG = /^[\u{E0030}-\u{E0039}\u{E0061}-\u{E007A}]{2,8}\u{E007F}$/u
const SOMETHING_TO_SHOW = /[\p{L}\p{N}\p{P}\p{S}\p{M}]/u

/**
 * The characters of `text` that may be drawn, line breaks as `linebreak` (a space for a title, `'\n'` for a body).
 */
function visibleCharacters(text: string, linebreak: string): string {
  const chars = Array.from(text.replace(/\r\n/g, '\n').replace(/[\r\u0085\u2028\u2029]/g, '\n'))
  const out: string[] = []
  const last = (): string => out[out.length - 1] ?? ''
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i] as string
    if (c === '\n') out.push(linebreak)
    else if (CONTROL.test(c) || BLANK_GLYPH.test(c)) out.push(' ')
    else if (BIDI_CONTROL.test(c)) continue
    else if (c === ZWJ || c === ZWNJ) {
      if (JOINS_AFTER.test(last()) && JOINS_BEFORE.test(chars[i + 1] ?? '')) out.push(c)
    } else if (VARIATION_SELECTOR.test(c)) {
      if (SELECTS_AFTER.test(last())) out.push(c)
    } else if (TAG.test(c)) {
      // a run of tags is kept whole, after its black flag, or not at all
      let end = i
      while (TAG.test(chars[end] ?? '')) end++
      const run = chars.slice(i, end).join('')
      if (PICTOGRAPH.test(last()) && SUBDIVISION_FLAG.test(run)) out.push(run)
      i = end - 1
    } else if (!IGNORABLE.test(c)) out.push(c)
  }
  return out.join('')
}

/**
 * A title for one line: line breaks and controls become spaces, runs of white space collapse to one space, and the
 * ends are trimmed. Returns `''` when nothing visible is left; the caller names the app instead.
 */
export function cleanTitle(title: string): string {
  const line = visibleCharacters(title, ' ').replace(/\s+/g, ' ').trim()
  return SOMETHING_TO_SHOW.test(line) ? line : ''
}

/**
 * A body for a few lines: line breaks stay (as `\n`), at most one blank line in a row, controls become spaces, the
 * white space of each line collapses, and the ends are trimmed. Returns `''` when nothing visible is left.
 */
export function cleanBody(body: string): string {
  const text = visibleCharacters(body, '\n')
    .split('\n')
    .map((line) => line.replace(/[^\S\n]+/g, ' ').trim())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
  return SOMETHING_TO_SHOW.test(text) ? text : ''
}

/** The most a screen reader is told of a body: the screen shows a few lines of it, so the speech gives the start of it. */
export const SPOKEN_BODY_MAX = 300

/** The start of a body, as one line, cut at a word where it can be, for a screen reader (it never splits a character). */
export function spokenBody(body: string): string {
  const chars = Array.from(body.replace(/\s+/g, ' ').trim())
  if (chars.length <= SPOKEN_BODY_MAX) return chars.join('')
  const cut = chars.slice(0, SPOKEN_BODY_MAX).join('')
  const space = cut.lastIndexOf(' ')
  return `${(space > SPOKEN_BODY_MAX / 2 ? cut.slice(0, space) : cut).trimEnd()}…`
}
