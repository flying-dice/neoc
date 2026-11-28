/**
 * A command typed on one line, as a shell reads it: words between spaces, and a word with a space in it in quotes
 * (`"two words"` or `'two words'`; in double quotes a backslash keeps the next character as it is). This is how the form
 * shows the agent's command and an MCP server's arguments, which the backend keeps as lists of words.
 */

/** The words of `text`. An open quote runs to the end. */
export function wordsOf(text: string): string[] {
  const words: string[] = []
  let word = ''
  // a word is there once anything of it is read, so `""` is an empty word
  let started = false
  let quote: '"' | "'" | null = null
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!
    if (quote === "'") {
      if (c === "'") quote = null
      else word += c
    } else if (quote === '"') {
      if (c === '"') quote = null
      else if (c === '\\' && i + 1 < text.length) word += text[++i]
      else word += c
    } else if (/\s/.test(c)) {
      if (started) words.push(word)
      word = ''
      started = false
    } else if (c === '"' || c === "'") {
      quote = c
      started = true
    } else {
      word += c
      started = true
    }
  }
  if (started) words.push(word)
  return words
}

/** The words as one line that `wordsOf` reads back: a word that is empty or has a space or a quote in it is quoted. */
export function textOf(words: readonly string[]): string {
  return words.map((word) => (word !== '' && !/[\s"'\\]/.test(word) ? word : `"${word.replace(/["\\]/g, '\\$&')}"`)).join(' ')
}
