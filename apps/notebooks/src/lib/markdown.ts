/**
 * A note's Markdown, read into what the reading view draws. The text is never made into markup: it becomes blocks
 * (headings, paragraphs, lists, quotes, code, rules) of runs (text with a style, code, links, tags), and the view draws
 * each as an element of its own, so nothing a note says can be read as HTML.
 *
 * What is read: `# headings`, paragraphs (a line break is a line break), `-`, `*`, `+` and `1.` lists, nested by their
 * indent, with `- [ ]` tasks, `> quotes`, fenced code, `---` rules, and in a line `**bold**`, `*italic*`, `~~struck~~`,
 * `==highlighted==`, `` `code` ``, `[text](https://...)`, `[[Note]]`, `[[Note|shown text]]` and `#tag`.
 */

export interface Style {
  readonly bold: boolean
  readonly italic: boolean
  readonly strike: boolean
  readonly mark: boolean
}

export type Run =
  | { readonly kind: 'text'; readonly text: string; readonly style: Style }
  | { readonly kind: 'code'; readonly text: string }
  /** A link out of the app: `url` is http or https. */
  | { readonly kind: 'link'; readonly text: string; readonly url: string; readonly style: Style }
  /** A link to the note called `target`. */
  | { readonly kind: 'wiki'; readonly text: string; readonly target: string; readonly style: Style }
  /** `tag` is in lower case, without the `#`; `text` is as it was written, with it. */
  | { readonly kind: 'tag'; readonly text: string; readonly tag: string }

export interface Item {
  readonly runs: readonly Run[]
  /** Whether it is ticked, for a task (`- [ ]`); null for an item that is not one. */
  readonly task: boolean | null
  /** The line of the text it is on, from 0: where a task is ticked. */
  readonly line: number
  readonly children: readonly List[]
}

export interface List {
  readonly ordered: boolean
  readonly items: readonly Item[]
}

export type Block =
  | { readonly kind: 'heading'; readonly level: 1 | 2 | 3 | 4 | 5 | 6; readonly runs: readonly Run[] }
  /** Each line of the paragraph is a line of the reading view. */
  | { readonly kind: 'paragraph'; readonly lines: readonly (readonly Run[])[] }
  | { readonly kind: 'list'; readonly list: List }
  | { readonly kind: 'quote'; readonly lines: readonly (readonly Run[])[] }
  | { readonly kind: 'code'; readonly text: string }
  | { readonly kind: 'rule' }

const PLAIN: Style = { bold: false, italic: false, strike: false, mark: false }

/** The marks that turn a style on and off, the longest first: `**` is not two `*`. */
const MARKS: readonly { readonly mark: string; readonly style: keyof Style }[] = [
  { mark: '**', style: 'bold' },
  { mark: '~~', style: 'strike' },
  { mark: '==', style: 'mark' },
  { mark: '*', style: 'italic' },
  { mark: '_', style: 'italic' }
]

const isWord = (character: string | undefined): boolean => character !== undefined && /[\p{L}\p{N}]/u.test(character)
const isSpace = (character: string | undefined): boolean => character === undefined || /\s/.test(character)

/** The runs of one line of text. */
export function inline(text: string): Run[] {
  const runs: Run[] = []
  let style = PLAIN
  // which mark turned each style on: `*a_b*` is not closed by the `_`
  const opened = new Map<keyof Style, string>()
  let pending = ''
  const flush = (): void => {
    if (pending !== '') runs.push({ kind: 'text', text: pending, style })
    pending = ''
  }

  let at = 0
  scan: while (at < text.length) {
    const rest = text.slice(at)
    const character = text[at] ?? ''

    if (character === '\\' && at + 1 < text.length && /[\\`*_~=[\]#]/.test(text[at + 1] ?? '')) {
      pending += text[at + 1]
      at += 2
      continue
    }
    if (character === '`') {
      const end = text.indexOf('`', at + 1)
      if (end > at + 1) {
        flush()
        runs.push({ kind: 'code', text: text.slice(at + 1, end) })
        at = end + 1
        continue
      }
    }
    if (character === '[') {
      const wiki = /^\[\[([^[\]\n]+)\]\]/.exec(rest)
      const target = wiki ? (/^[^|#]*/.exec(wiki[1] ?? '')?.[0] ?? '').trim() : ''
      if (wiki && target !== '') {
        const inside = wiki[1] ?? ''
        const shown = inside.includes('|') ? inside.slice(inside.indexOf('|') + 1).trim() : inside.trim()
        flush()
        runs.push({ kind: 'wiki', text: shown === '' ? target : shown, target, style })
        at += wiki[0].length
        continue
      }
      const link = /^\[([^\]\n]+)\]\((https?:\/\/[^)\s]+)\)/.exec(rest)
      if (link) {
        flush()
        runs.push({ kind: 'link', text: link[1] ?? '', url: link[2] ?? '', style })
        at += link[0].length
        continue
      }
    }
    if (character === '#' && isSpace(text[at - 1])) {
      const tag = /^#([A-Za-z0-9_/-]+)/.exec(rest)
      if (tag && /[^0-9]/.test(tag[1] ?? '')) {
        flush()
        runs.push({ kind: 'tag', text: tag[0], tag: (tag[1] ?? '').toLowerCase() })
        at += tag[0].length
        continue
      }
    }
    for (const { mark, style: which } of MARKS) {
      if (!rest.startsWith(mark)) continue
      const before = text[at - 1]
      const after = text[at + mark.length]
      // a `_` inside a word is a letter of it: snake_case stays as written
      const inWord = mark === '_' && isWord(before) && isWord(after)
      const closes = opened.get(which) === mark && !isSpace(before) && !inWord
      const opens = !opened.has(which) && !isSpace(after) && !inWord && closerAfter(text, at + mark.length, mark)
      if (!closes && !opens) continue
      flush()
      if (closes) opened.delete(which)
      else opened.set(which, mark)
      style = { ...style, [which]: opens }
      at += mark.length
      continue scan
    }
    pending += character
    at++
  }
  flush()
  return runs
}

/** Whether `mark` comes again after `from`, with something other than a space before it: what is opened will be closed. */
function closerAfter(text: string, from: number, mark: string): boolean {
  for (let at = text.indexOf(mark, from + 1); at !== -1; at = text.indexOf(mark, at + 1)) {
    if (!isSpace(text[at - 1])) return true
  }
  return false
}

const HEADING = /^(#{1,6})\s+(.*)$/
const RULE = /^\s*(?:\*{3,}|-{3,}|_{3,})\s*$/
const FENCE = /^\s*```/
const QUOTE = /^\s*>\s?(.*)$/
const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/
const TASK = /^\[([ xX])\]\s+(.*)$/

/** How far a line is set in: a tab counts as four spaces. */
const indentOf = (space: string): number => [...space].reduce((width, c) => width + (c === '\t' ? 4 : 1), 0)

interface OpenItem {
  runs: Run[]
  task: boolean | null
  line: number
  children: OpenList[]
}
interface OpenList {
  ordered: boolean
  items: OpenItem[]
}

/**
 * The list that starts at line `from`: its items, nested by their indent, up to the first line that is not an item of
 * it. Returns the list and the line after it.
 */
function list(lines: readonly string[], from: number): { list: List; next: number } {
  // the lists that are open, the outermost first, each with the indent of its items
  const open: { indent: number; list: OpenList }[] = []
  let root: OpenList | undefined
  let at = from
  for (; at < lines.length; at++) {
    const item = ITEM.exec(lines[at] ?? '')
    if (!item) break
    const indent = indentOf(item[1] ?? '')
    const task = TASK.exec(item[3] ?? '')
    const made: OpenItem = {
      runs: inline(task ? (task[2] ?? '') : (item[3] ?? '')),
      task: task ? task[1] !== ' ' : null,
      line: at,
      children: []
    }
    while (open.length > 1 && (open.at(-1)?.indent ?? 0) > indent) open.pop()
    const top = open.at(-1)
    const parent = top?.list.items.at(-1)
    if (top && parent && indent > top.indent) {
      const nested: OpenList = { ordered: /\d/.test(item[2] ?? ''), items: [made] }
      parent.children.push(nested)
      open.push({ indent, list: nested })
    } else if (top) {
      top.list.items.push(made)
    } else {
      root = { ordered: /\d/.test(item[2] ?? ''), items: [made] }
      open.push({ indent, list: root })
    }
  }
  return { list: root ?? { ordered: false, items: [] }, next: at }
}

/** Whether a line starts a block of its own, and so ends the paragraph before it. */
const startsBlock = (line: string): boolean => HEADING.test(line) || RULE.test(line) || FENCE.test(line) || QUOTE.test(line) || ITEM.test(line)

/** The blocks of a note's text, in order. */
export function parse(body: string): Block[] {
  const lines = body.split('\n')
  const blocks: Block[] = []
  let at = 0
  while (at < lines.length) {
    const line = lines[at] ?? ''
    const heading = HEADING.exec(line)
    if (line.trim() === '') {
      at++
    } else if (FENCE.test(line)) {
      // up to the closing fence, or the end of the text when there is none
      const close = lines.findIndex((l, i) => i > at && FENCE.test(l))
      const end = close === -1 ? lines.length : close
      blocks.push({ kind: 'code', text: lines.slice(at + 1, end).join('\n') })
      at = end + 1
    } else if (heading) {
      // the pattern takes one to six `#`
      blocks.push({ kind: 'heading', level: (heading[1] ?? '#').length as 1 | 2 | 3 | 4 | 5 | 6, runs: inline((heading[2] ?? '').trim()) })
      at++
    } else if (RULE.test(line)) {
      blocks.push({ kind: 'rule' })
      at++
    } else if (QUOTE.test(line)) {
      const quoted: Run[][] = []
      for (let q = QUOTE.exec(lines[at] ?? ''); q; q = at < lines.length ? QUOTE.exec(lines[at] ?? '') : null) {
        quoted.push(inline(q[1] ?? ''))
        at++
      }
      blocks.push({ kind: 'quote', lines: quoted })
    } else if (ITEM.test(line)) {
      const read = list(lines, at)
      blocks.push({ kind: 'list', list: read.list })
      at = read.next
    } else {
      const said: Run[][] = []
      while (at < lines.length && (lines[at] ?? '').trim() !== '' && (said.length === 0 || !startsBlock(lines[at] ?? ''))) {
        said.push(inline((lines[at] ?? '').trim()))
        at++
      }
      blocks.push({ kind: 'paragraph', lines: said })
    }
  }
  return blocks
}
