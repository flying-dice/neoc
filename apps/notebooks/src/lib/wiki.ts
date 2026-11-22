/**
 * What a note's text says about it, as the backend reads it (server/text.luau): the notes it links to (`[[Title]]`), its
 * tags (`#tag`) and the line that stands for it in a list. The window uses the title rules to say what is wrong with a
 * title before it is sent; the stand-in backend of the stories and tests uses the rest.
 */

/** The inside of a link: no bracket and no line break. */
const LINK = /\[\[([^[\]\n]+)\]\]/g
const TITLE_MAX = 120
const EXCERPT_MAX = 160

/** How a title is compared: two titles that differ only in capitals and outer spaces name the same note. */
export const keyOf = (title: string): string => title.trim().toLowerCase()

const cut = (line: string, max: number): string => {
  const characters = [...line]
  return characters.length <= max ? line : `${characters.slice(0, max).join('')}…`
}

/** Why a note cannot be called `title`, or null when it can. */
export function titleProblem(title: string): string | null {
  const named = title.trim()
  if (named === '') return 'A note needs a title.'
  if ([...named].length > TITLE_MAX) return `A title is at most ${TITLE_MAX} characters.`
  if (/[[\]|#^\n]/.test(named)) return 'A title cannot have [ ] | # or ^ in it.'
  return null
}

/** The lines that are writing: none of a fenced block, and no inline code. */
function prose(body: string): string[] {
  const kept: string[] = []
  let fenced = false
  for (const line of body.split('\n')) {
    if (/^\s*```/.test(line)) fenced = !fenced
    else if (!fenced) kept.push(line.replace(/`[^`]*`/g, ''))
  }
  return kept
}

/** The title a link's inside names: what is before the `|` and the `#`. */
const targetOf = (inside: string): string => keyOf(/^[^|#]*/.exec(inside)?.[0] ?? '')

/** A line without the marks of its styles and its links: `**Sow** in [[March|spring]]` reads `Sow in spring`. */
const plain = (line: string): string =>
  line
    .replace(LINK, (_, inside: string) => (inside.includes('|') ? inside.slice(inside.indexOf('|') + 1) : inside).trim())
    .replace(/\*\*|~~|==|`/g, '')

/** The keys of the titles a text links to, each once, in the order they are first linked. */
export function linksOf(body: string): string[] {
  const found = new Set<string>()
  for (const line of prose(body)) for (const [, inside = ''] of line.matchAll(LINK)) if (targetOf(inside) !== '') found.add(targetOf(inside))
  return [...found]
}

/** The tags of a text, in lower case without the `#`, each once, in alphabetical order. `#1` is a number, not a tag. */
export function tagsOf(body: string): string[] {
  const found = new Set<string>()
  for (const line of prose(body)) {
    for (const [, tag = ''] of ` ${line.replace(LINK, '')}`.matchAll(/\s#([A-Za-z0-9_/-]+)/g)) {
      if (/[^0-9]/.test(tag)) found.add(tag.toLowerCase())
    }
  }
  return [...found].sort()
}

/**
 * The line that stands for a text in a list: its first line with something on it that is not a heading (a note's first
 * heading is often its title again), or its first heading when it has nothing else. It is read without the marks of a
 * list, a quote, a style or a link.
 */
export function excerptOf(body: string): string {
  let heading = ''
  for (const line of body.split('\n')) {
    const said = plain(line.replace(/^[\s#>\-*+]+/, '')).trim()
    if (said === '' || /^\s*```/.test(line)) continue
    if (!/^#{1,6}\s/.test(line)) return cut(said, EXCERPT_MAX)
    if (heading === '') heading = said
  }
  return cut(heading, EXCERPT_MAX)
}

/** The first line of a text that links to the title with this key, without its marks; empty when none does. */
export function mentionOf(body: string, key: string): string {
  for (const line of prose(body)) for (const [, inside = ''] of line.matchAll(LINK)) if (targetOf(inside) === key) return cut(plain(line).trim(), EXCERPT_MAX)
  return ''
}

/** The first line of a text that has `words` in it, whatever their capitals, without its marks; empty when none does. */
export function lineWith(body: string, words: string): string {
  const line = body.split('\n').find((l) => l.toLowerCase().includes(words.toLowerCase()))
  return line === undefined ? '' : cut(plain(line).trim(), EXCERPT_MAX)
}

/** A text with its links to the title with this key rewritten to `title`: the shown text and the heading of each link stay. */
export function retarget(body: string, key: string, title: string): string {
  return body.replace(LINK, (whole, inside: string) => (targetOf(inside) === key ? `[[${title}${/[|#].*$/.exec(inside)?.[0] ?? ''}]]` : whole))
}

/**
 * A text with the task on one of its lines ticked, or unticked when it was: `- [ ] Milk` and `- [x] Milk`. `line` counts
 * from 0. A line that is not a task leaves the text as it is.
 */
export function toggleTask(body: string, line: number): string {
  const lines = body.split('\n')
  const at = lines[line]
  const task = at === undefined ? null : /^(\s*(?:[-*+]|\d+[.)])\s+\[)([ xX])(\].*)$/.exec(at)
  if (!task) return body
  lines[line] = `${task[1]}${task[2] === ' ' ? 'x' : ' '}${task[3]}`
  return lines.join('\n')
}
