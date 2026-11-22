import { describe, expect, test } from 'bun:test'
import { cleanBody, cleanTitle, spokenBody, SPOKEN_BODY_MAX } from './notificationText'

// Characters that cannot be written into a source file as they are, by code point.
const FAMILY = '\u{1F468}\u200D\u{1F469}\u200D\u{1F467}\u200D\u{1F466}'
const RAINBOW_FLAG = '\u{1F3F3}\uFE0F\u200D\u{1F308}'
const HEART_ON_FIRE = '❤\uFE0F\u200D\u{1F525}'
const ENGLAND = '\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}'
const WAVING_HAND_DARK = '\u{1F44B}\u{1F3FF}'
const KEYCAP = '1\uFE0F⃣'
// می\u200Cخواهم ("I want"): a zero-width non-joiner between the letters of one word
const PERSIAN = 'می\u200Cخواهم'
// क्ष written with an explicit joiner after the virama
const HINDI = 'क्\u200Dष'

describe('cleanTitle', () => {
  test('plain text is left alone', () => {
    expect(cleanTitle('Backup finished')).toBe('Backup finished')
  })

  test('line breaks, tabs and other controls become single spaces, so the title is one line', () => {
    expect(cleanTitle('Backup\nfinished')).toBe('Backup finished')
    expect(cleanTitle('a\r\nb\u2028c\u2029d\u0085e')).toBe('a b c d e')
    expect(cleanTitle('a\t\tb\u0000c\u001bd\u007fe\u009ff')).toBe('a b c d e f')
    expect(cleanTitle('  too   many \n\n spaces  ')).toBe('too many spaces')
  })

  test('bidirectional controls are removed, so a title cannot reorder the text around it', () => {
    expect(cleanTitle('\u202Egnp.exe\u202C')).toBe('gnp.exe')
    expect(
      cleanTitle('a\u2066b\u2067c\u2068d\u2069e\u200Ef\u200Fg\u061Ch\u202Ai\u202Bj\u202Dk')
    ).toBe('abcdefghijk')
  })

  test('characters that draw nothing are removed', () => {
    expect(cleanTitle('Back\u200Bup\u2060\uFEFF fin\u00ADished')).toBe('Backup finished')
    expect(
      cleanTitle('a\u034Fb\u115Fc\u3164d\uFFA0e\u17B4f\u17B5g\u180Bh\u180Ci\u180Dj\u2062k\u2064l')
    ).toBe('abcdefghijkl')
    // a joiner that joins nothing (at an end, doubled, beside a space) is as invisible as a zero-width space
    expect(cleanTitle('\u200Dab\u200D')).toBe('ab')
    expect(cleanTitle('a \u200D b')).toBe('a b')
    expect(cleanTitle('a\u200D\u200D b')).toBe('a b')
    expect(cleanTitle('a\u200C b')).toBe('a b')
  })

  test('a family emoji, a rainbow flag and a heart on fire keep their joiners', () => {
    expect(cleanTitle(`${FAMILY} Backup done`)).toBe(`${FAMILY} Backup done`)
    expect(cleanTitle(RAINBOW_FLAG)).toBe(RAINBOW_FLAG)
    expect(cleanTitle(HEART_ON_FIRE)).toBe(HEART_ON_FIRE)
  })

  test('the flag of England keeps its tag sequence, a skin tone and a keycap their forms', () => {
    expect(cleanTitle(`${ENGLAND} match`)).toBe(`${ENGLAND} match`)
    expect(cleanTitle(WAVING_HAND_DARK)).toBe(WAVING_HAND_DARK)
    expect(cleanTitle(KEYCAP)).toBe(KEYCAP)
  })

  test('tags that are not a flag are removed: a hidden message after letters, or a run that never ends', () => {
    expect(cleanTitle('hi\u{E0068}\u{E0065}\u{E006C}\u{E0070}\u{E007F}')).toBe('hi')
    expect(cleanTitle('\u{1F3F4}\u{E0067}\u{E0062}')).toBe('\u{1F3F4}')
    expect(cleanTitle('\u{1F3F4}\u{E0001}\u{E0020}')).toBe('\u{1F3F4}')
  })

  test('words of other scripts keep what spells them: Persian with its non-joiner, Hindi with its joiner', () => {
    expect(cleanTitle(PERSIAN)).toBe(PERSIAN)
    expect(cleanTitle(HINDI)).toBe(HINDI)
    expect(cleanTitle('اكتمل النسخ 完了 ✅ 🙂')).toBe('اكتمل النسخ 完了 ✅ 🙂')
  })

  test('a variation selector that selects nothing is removed', () => {
    expect(cleanTitle('a\uFE0Fb')).toBe('ab')
    expect(cleanTitle(' \uFE0F x')).toBe('x')
    expect(cleanTitle('❤\uFE0F\uFE0F')).toBe('❤\uFE0F')
  })

  test('a title that shows nothing is empty, for the caller to name the app instead', () => {
    const blanks = [
      '',
      '   ',
      '\n\t \u202E',
      '\u200B\u200B\u200B',
      '\u2800\u2800\u2800', // braille blank
      '\uFE0F', // a lone emoji presentation selector
      '\uFE0F\uFE0F\uFE0E',
      '\u17B4\u17B5', // Khmer inherent vowels
      '\u180B\u180C\u180D\u180E', // Mongolian free variation selectors and vowel separator
      '\uFFFC', // object replacement character
      '\u200D',
      '\u200C\u200D\u200C',
      '\u2062\u2063\u3164\uFFA0',
      '\u{E0067}\u{E0062}\u{E007F}', // a tag run with no flag before it
      '\u2800 \u200B \uFFFC \uFE0F'
    ]
    for (const blank of blanks) expect(cleanTitle(blank)).toBe('')
  })

  test('markup is just characters: it is drawn as text by the caller, not changed here', () => {
    expect(cleanTitle('<img src=x onerror=alert(1)>')).toBe('<img src=x onerror=alert(1)>')
  })
})

describe('cleanBody', () => {
  test('line breaks stay, in any of their spellings', () => {
    expect(cleanBody('one\ntwo\r\nthree\rfour\u2028five')).toBe('one\ntwo\nthree\nfour\nfive')
  })

  test('at most one blank line in a row, white space per line collapsed, ends trimmed', () => {
    expect(cleanBody('\n\n a   b \n\n\n\n c \n\n')).toBe('a b\n\nc')
  })

  test('controls become spaces; bidi and invisible characters go; joiners that join stay', () => {
    expect(cleanBody('a\u0007b\tc\u202Ed\u200Be')).toBe('a b cde')
    expect(cleanBody(`${FAMILY}\n${PERSIAN}`)).toBe(`${FAMILY}\n${PERSIAN}`)
  })

  test('a body that shows nothing is empty', () => {
    expect(cleanBody('\u200B\n \u202E\n')).toBe('')
    expect(cleanBody('\u2800\n\u2800')).toBe('')
  })
})

describe('spokenBody', () => {
  test('a short body is read as it is, on one line', () => {
    expect(spokenBody('12 files\ncopied')).toBe('12 files copied')
  })

  test('a long body is cut at a word and says so', () => {
    const spoken = spokenBody('word '.repeat(2000))
    expect(Array.from(spoken).length).toBeLessThanOrEqual(SPOKEN_BODY_MAX + 1)
    expect(spoken.endsWith('word…')).toBe(true)
  })

  test('it never splits a character', () => {
    const spoken = spokenBody('\u{1F600}'.repeat(SPOKEN_BODY_MAX + 5))
    expect(spoken).toBe('\u{1F600}'.repeat(SPOKEN_BODY_MAX) + '…')
  })
})
