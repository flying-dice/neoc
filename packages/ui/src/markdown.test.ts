import { describe, expect, test } from 'bun:test'
import { optionsFromExtensions, parseMarkdown, plainText, safeHref, type Block } from './markdown'

const first = (src: string, options?: Parameters<typeof parseMarkdown>[1]): Block =>
  parseMarkdown(src, options)[0]!

describe('Markdown parsing', () => {
  test('headings, paragraphs and inline emphasis become typed nodes', () => {
    const blocks = parseMarkdown('# Title\n\nSome **bold**, _em_ and `code`.')
    expect(blocks[0]).toEqual({ t: 'heading', depth: 1, children: [{ t: 'text', text: 'Title' }] })
    const p = blocks[1]
    expect(p?.t).toBe('paragraph')
    if (p?.t === 'paragraph') {
      expect(p.children.map((c) => c.t)).toEqual(['text', 'strong', 'text', 'em', 'text', 'code', 'text'])
    }
  })

  test('lists keep order, start number and task state', () => {
    const list = first('3. three\n4. four')
    expect(list).toMatchObject({ t: 'list', ordered: true, start: 3 })
    const tasks = first('- [x] done\n- [ ] todo\n- plain')
    expect(tasks.t).toBe('list')
    if (tasks.t === 'list') expect(tasks.items.map((i) => i.checked)).toEqual([true, false, undefined])
  })

  test('with tasklist off a task item keeps its marker as text', () => {
    const tasks = first('- [x] done', { tasklist: false })
    if (tasks.t !== 'list') throw new Error('expected a list')
    const item = tasks.items[0]!
    expect(item.checked).toBeUndefined()
    const p = item.children[0]
    expect(p?.t === 'paragraph' && plainText(p.children)).toBe('[x] done')
  })

  test('code blocks keep their text exactly and the language', () => {
    expect(first('```ts\nconst a = "<b>&";\n```')).toEqual({
      t: 'code',
      lang: 'ts',
      text: 'const a = "<b>&";'
    })
  })

  test('quotes nest blocks and tables carry alignment', () => {
    const quote = first('> quoted\n> text')
    expect(quote.t).toBe('quote')
    const table = first('| a | b |\n|:--|--:|\n| 1 | 2 |')
    expect(table).toMatchObject({ t: 'table', align: ['left', 'right'] })
    if (table.t === 'table') {
      expect(table.header.map(plainText)).toEqual(['a', 'b'])
      expect(table.rows[0]?.map(plainText)).toEqual(['1', '2'])
    }
  })

  test('the extensions list narrows the features', () => {
    expect(optionsFromExtensions(undefined)).toEqual({ tables: true, tasklist: true, code: true })
    expect(optionsFromExtensions(['tables'])).toEqual({ tables: true, tasklist: false, code: false })
    expect(first('| a |\n|---|\n| 1 |', { tables: false }).t).toBe('paragraph')
    expect(first('```\nx\n```', { code: false }).t).toBe('paragraph')
  })
})

describe('Markdown is never markup', () => {
  test('a block of raw HTML is shown as the text it is', () => {
    const blocks = parseMarkdown('<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>')
    expect(blocks.every((b) => b.t === 'paragraph')).toBe(true)
    const shown = blocks.map((b) => (b.t === 'paragraph' ? plainText(b.children) : '')).join('|')
    expect(shown).toContain('<script>alert(1)</script>')
    expect(shown).toContain('<img src=x onerror=alert(1)>')
  })

  test('inline HTML is text between the words around it', () => {
    const p = first('a <b>bold</b> <i onclick="x()">move</i> c')
    if (p.t !== 'paragraph') throw new Error('expected a paragraph')
    expect(p.children.every((c) => c.t === 'text')).toBe(true)
    expect(plainText(p.children)).toBe('a <b>bold</b> <i onclick="x()">move</i> c')
  })

  test('HTML inside code stays code', () => {
    const p = first('use `<div>` here')
    if (p.t !== 'paragraph') throw new Error('expected a paragraph')
    expect(p.children).toContainEqual({ t: 'code', text: '<div>' })
  })

  test('only http, https and mailto links keep their href', () => {
    expect(safeHref('https://example.com/a?b=1')).toBe('https://example.com/a?b=1')
    expect(safeHref('mailto:a@b.co')).toBe('mailto:a@b.co')
    for (const bad of [
      'javascript:alert(1)',
      ' JavaScript:alert(1)',
      'data:text/html,<b>x</b>',
      'file:///etc/passwd',
      '/relative',
      '//host/path',
      'java\nscript:alert(1)',
      ''
    ])
      expect(safeHref(bad)).toBeNull()
    expect(safeHref(undefined)).toBeNull()
  })

  test('a link with an unsafe target keeps its text and loses the link', () => {
    const p = first('[click](javascript:alert(1)) and [ok](https://example.com)')
    if (p.t !== 'paragraph') throw new Error('expected a paragraph')
    const links = p.children.filter((c) => c.t === 'link')
    expect(links).toHaveLength(1)
    expect(links[0]).toMatchObject({ href: 'https://example.com' })
    expect(plainText(p.children)).toBe('click and ok')
  })

  test('images are never loaded: the alt text stands in', () => {
    const p = first('![a cat](https://example.com/cat.png)')
    if (p.t !== 'paragraph') throw new Error('expected a paragraph')
    expect(p.children).toEqual([{ t: 'text', text: 'a cat' }])
  })

  test('nothing in the tree is a string of markup the renderer would have to trust', () => {
    // Every node is one of the closed shapes: text only ever appears as { t: 'text' | 'code', text }.
    const json = JSON.stringify(
      parseMarkdown('# <u>x</u>\n\n> <iframe src=//evil></iframe>\n\n- <a href="javascript:1">y</a>')
    )
    expect(json).not.toContain('"href":"javascript')
    expect(json).not.toContain('"t":"html"')
  })
})
