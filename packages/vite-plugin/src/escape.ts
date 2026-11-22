/**
 * Every string that comes from the contract is hostile wherever it is written into generated code or a comment: a title with
 * a newline must not end a `//` comment and start code, an enum value with a quote must not end its string. One routine per
 * target, used everywhere the generators write contract text:
 *
 *   - `jsString`      a JavaScript or TypeScript string literal (also a string-literal type)
 *   - `luauString`    a Luau string literal (also a string-singleton type)
 *   - `jsDoc`         the body of a `/** ... *\/` comment
 *   - `lineComment`   the text of a one-line comment (`//` or `--`): one line, whatever the text held
 *   - `luauDoc`       `---` doc-comment lines
 *
 * Names (methods, params, properties, components) never reach this: they are identifiers, checked when the contract is read.
 */

/** A line break of any kind JavaScript or Luau might read as one, or any other control character. */
const LINE_BREAKS = /\r\n|[\r\n\u2028\u2029\u0085\v\f]/
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u0008\u000e-\u001f\u007f]/g

/** A JavaScript string literal. `JSON.stringify` does the quoting; U+2028 and U+2029 are escaped too, for older readers. */
export function jsString(value: string): string {
  return JSON.stringify(value).replace(/\u2028/g, '\\u2028').replace(/\u2029/g, '\\u2029')
}

/**
 * A Luau string literal: `"`, `\`, line breaks and every control character escaped. Luau has no `\u0000`: the escape is
 * decimal `\ddd`, at most `\255`, so only the characters below U+0020 and DEL are written that way; anything else (U+2028 and
 * U+2029 too) is an ordinary character inside a Luau string and is kept.
 */
export function luauString(value: string): string {
  let out = '"'
  for (const char of value) {
    const code = char.codePointAt(0) as number
    if (char === '"') out += '\\"'
    else if (char === '\\') out += '\\\\'
    else if (char === '\n') out += '\\n'
    else if (char === '\r') out += '\\r'
    else if (char === '\t') out += '\\t'
    else if (code < 0x20 || code === 0x7f) {
      out += `\\${String(code).padStart(3, '0')}`
    } else out += char
  }
  return `${out}"`
}

/** Text as one line: every line break and control character becomes a space. */
export function oneLine(text: string): string {
  return text.split(LINE_BREAKS).join(' ').replace(CONTROL, ' ')
}

/** The text of a one-line comment (`// text`, `-- text`): it can neither end the comment nor start code on the next line. */
export function lineComment(text: string): string {
  return oneLine(text)
}

/** The lines of a doc comment body: split on every kind of line break, controls blanked. */
function lines(text: string): string[] {
  return text.split(LINE_BREAKS).map((line) => line.replace(CONTROL, ' ').trimEnd())
}

/** A `/** ... *\/` doc comment at `indent`, or nothing for no text. `*\/` inside the text cannot end it early. */
export function jsDoc(text: string | undefined, indent: string): string {
  if (text === undefined) return ''
  const safe = lines(text).map((line) => line.replace(/\*\//g, '* /'))
  if (safe.length === 1) return `${indent}/** ${safe[0]} */\n`
  return `${indent}/**\n${safe.map((line) => `${indent} * ${line}`.trimEnd()).join('\n')}\n${indent} */\n`
}

/** `--- ` doc-comment lines at `indent`, or nothing for no text. */
export function luauDoc(text: string | undefined, indent: string): string {
  if (text === undefined) return ''
  return lines(text).map((line) => `${indent}--- ${line}`.trimEnd()).join('\n') + '\n'
}
