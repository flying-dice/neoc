/**
 * Markdown to a small, closed document tree. The text is parsed with the `marked` lexer (tokens, never HTML) and
 * mapped here to `Block` / `Inline` nodes that the `Markdown` atom renders as Svelte elements, so nothing in the
 * source can become markup: raw HTML in the source is kept as *text*, a link whose scheme is not http(s) or mailto
 * loses its `href`, and images are never fetched (their alt text is shown). Agent and tool text is untrusted.
 */
import { Lexer, type Token, type Tokens } from 'marked'

export type Inline =
  | { t: 'text'; text: string }
  | { t: 'strong' | 'em' | 'del'; children: Inline[] }
  | { t: 'code'; text: string }
  | { t: 'link'; href: string; children: Inline[] }
  | { t: 'br' }

export type Align = 'left' | 'center' | 'right' | null

export interface ListItem {
  /** `undefined` for a plain item, a boolean for a task item (`- [x]`). */
  checked?: boolean
  children: Block[]
}

export type Block =
  | { t: 'heading'; depth: 1 | 2 | 3 | 4 | 5 | 6; children: Inline[] }
  | { t: 'paragraph'; children: Inline[] }
  | { t: 'code'; lang: string; text: string }
  | { t: 'quote'; children: Block[] }
  | { t: 'list'; ordered: boolean; start: number; items: ListItem[] }
  | { t: 'table'; align: Align[]; header: Inline[][]; rows: Inline[][][] }
  | { t: 'hr' }

/** The optional GFM features (the `extensions` prop). All are on unless a list says otherwise. */
export interface MarkdownOptions {
  tables?: boolean
  tasklist?: boolean
  code?: boolean
}

/** `extensions: ['tables']` enables only those; no list (or an unknown value) enables all. */
export function optionsFromExtensions(extensions: unknown): Required<MarkdownOptions> {
  if (!Array.isArray(extensions)) return { tables: true, tasklist: true, code: true }
  return {
    tables: extensions.includes('tables'),
    tasklist: extensions.includes('tasklist'),
    code: extensions.includes('code')
  }
}

const SAFE_SCHEMES = new Set(['http:', 'https:', 'mailto:'])

/**
 * A link target the client may hand to `openUrl`, or null. Only http, https and mailto: `javascript:`, `data:`,
 * `file:` and relative links are dropped (the text stays, the link does not).
 */
export function safeHref(href: unknown): string | null {
  if (typeof href !== 'string') return null
  const trimmed = href.trim()
  if (trimmed === '' || /[\u0000-\u001f\u007f]/.test(trimmed)) return null
  try {
    const url = new URL(trimmed)
    return SAFE_SCHEMES.has(url.protocol) ? trimmed : null
  } catch {
    return null
  }
}

const text = (t: string): Inline => ({ t: 'text', text: t })

function inlines(tokens: Token[] | undefined, fallback: string): Inline[] {
  if (!tokens) return fallback ? [text(fallback)] : []
  const out: Inline[] = []
  for (const token of tokens) {
    const node = inline(token)
    if (node) out.push(...(Array.isArray(node) ? node : [node]))
  }
  return out
}

function inline(token: Token): Inline | Inline[] | null {
  switch (token.type) {
    case 'text':
    case 'escape':
      return text((token as Tokens.Text).text)
    case 'strong':
    case 'em':
    case 'del': {
      const t = token as Tokens.Strong
      return { t: token.type, children: inlines(t.tokens, t.text) }
    }
    case 'codespan':
      return { t: 'code', text: (token as Tokens.Codespan).text }
    case 'br':
      return { t: 'br' }
    case 'link': {
      const t = token as Tokens.Link
      const href = safeHref(t.href)
      const children = inlines(t.tokens, t.text)
      return href ? { t: 'link', href, children } : children
    }
    case 'image': {
      // Never fetched: the alt text stands in.
      const t = token as Tokens.Image
      return text(t.text || 'image')
    }
    case 'html':
      // Raw HTML is shown as the text it is.
      return text((token as Tokens.HTML).raw.replace(/\n$/, ''))
    default:
      return token.raw ? text(token.raw) : null
  }
}

function blocks(tokens: Token[], o: Required<MarkdownOptions>): Block[] {
  const out: Block[] = []
  for (const token of tokens) {
    switch (token.type) {
      case 'space':
      case 'def':
        break
      case 'heading': {
        const t = token as Tokens.Heading
        out.push({
          t: 'heading',
          depth: Math.min(6, Math.max(1, t.depth)) as 1 | 2 | 3 | 4 | 5 | 6,
          children: inlines(t.tokens, t.text)
        })
        break
      }
      case 'paragraph': {
        const t = token as Tokens.Paragraph
        out.push({ t: 'paragraph', children: inlines(t.tokens, t.text) })
        break
      }
      case 'text': {
        // A tight list item's text, or a loose text block.
        const t = token as Tokens.Text
        out.push({ t: 'paragraph', children: inlines(t.tokens, t.text) })
        break
      }
      case 'code': {
        const t = token as Tokens.Code
        out.push(
          o.code
            ? { t: 'code', lang: (t.lang ?? '').split(/\s/)[0] ?? '', text: t.text }
            : { t: 'paragraph', children: [text(t.raw.replace(/\n$/, ''))] }
        )
        break
      }
      case 'blockquote':
        out.push({ t: 'quote', children: blocks((token as Tokens.Blockquote).tokens, o) })
        break
      case 'list': {
        const t = token as Tokens.List
        out.push({
          t: 'list',
          ordered: t.ordered,
          start: typeof t.start === 'number' ? t.start : 1,
          items: t.items.map((item) => itemOf(item, o))
        })
        break
      }
      case 'table': {
        const t = token as Tokens.Table
        if (!o.tables) {
          out.push({ t: 'paragraph', children: [text(t.raw.replace(/\n$/, ''))] })
          break
        }
        out.push({
          t: 'table',
          align: t.align,
          header: t.header.map((c) => inlines(c.tokens, c.text)),
          rows: t.rows.map((row) => row.map((c) => inlines(c.tokens, c.text)))
        })
        break
      }
      case 'hr':
        out.push({ t: 'hr' })
        break
      case 'html':
        // A block of raw HTML is shown as text, never injected.
        out.push({ t: 'paragraph', children: [text((token as Tokens.HTML).raw.replace(/\n+$/, ''))] })
        break
      default:
        if (token.raw.trim()) out.push({ t: 'paragraph', children: [text(token.raw.trim())] })
    }
  }
  return out
}

function itemOf(item: Tokens.ListItem, o: Required<MarkdownOptions>): ListItem {
  // The lexer puts a leading `checkbox` token into the item for a task; it is the item's state, not content.
  const content = item.tokens.filter((t) => t.type !== 'checkbox')
  const children = blocks(content, o)
  if (item.task && o.tasklist) return { checked: item.checked === true, children }
  if (item.task) {
    const marker = item.checked ? '[x] ' : '[ ] '
    const first = children[0]
    if (first && first.t === 'paragraph') first.children.unshift(text(marker))
    else children.unshift({ t: 'paragraph', children: [text(marker)] })
  }
  return { children }
}

/** Parse Markdown source into blocks. Never throws: the lexer accepts any string. */
export function parseMarkdown(source: string, options: MarkdownOptions = {}): Block[] {
  const o = { tables: true, tasklist: true, code: true, ...options }
  return blocks(Lexer.lex(source, { gfm: true, breaks: false }), o)
}

/** The plain text of inline nodes (for an accessible name or a test). */
export function plainText(nodes: Inline[]): string {
  return nodes
    .map((n) =>
      n.t === 'text' || n.t === 'code'
        ? n.text
        : n.t === 'br'
          ? '\n'
          : plainText(n.children)
    )
    .join('')
}
