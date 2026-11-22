import { describe, expect, test } from 'bun:test'
import { inline, parse, type Run } from './markdown'

const PLAIN = { bold: false, italic: false, strike: false, mark: false }
const text = (said: string, style: Partial<typeof PLAIN> = {}): Run => ({ kind: 'text', text: said, style: { ...PLAIN, ...style } })

describe('a line', () => {
  test('plain text is one run', () => {
    expect(inline('Just words.')).toEqual([text('Just words.')])
  })

  test('bold, italic, struck and highlighted', () => {
    expect(inline('a **b** *c* _d_ ~~e~~ ==f==')).toEqual([
      text('a '),
      text('b', { bold: true }),
      text(' '),
      text('c', { italic: true }),
      text(' '),
      text('d', { italic: true }),
      text(' '),
      text('e', { strike: true }),
      text(' '),
      text('f', { mark: true })
    ])
  })

  test('styles nest', () => {
    expect(inline('**bold and *both***')).toEqual([text('bold and ', { bold: true }), text('both', { bold: true, italic: true })])
  })

  test('a mark that is never closed, a snake_case name and a lone star are as written', () => {
    expect(inline('2 * 3 and snake_case_name and **open')).toEqual([text('2 * 3 and snake_case_name and **open')])
  })

  test('code is as written, marks and all', () => {
    expect(inline('run `a **b** [[c]]` now')).toEqual([text('run '), { kind: 'code', text: 'a **b** [[c]]' }, text(' now')])
  })

  test('a link to a note, with shown text and with a heading', () => {
    expect(inline('[[Herbs]] [[Herbs|the bed]] [[Herbs#Basil]]')).toEqual([
      { kind: 'wiki', text: 'Herbs', target: 'Herbs', style: PLAIN },
      text(' '),
      { kind: 'wiki', text: 'the bed', target: 'Herbs', style: PLAIN },
      text(' '),
      { kind: 'wiki', text: 'Herbs#Basil', target: 'Herbs', style: PLAIN }
    ])
  })

  test('a link out is http or https; anything else is text', () => {
    expect(inline('[site](https://example.com/a)')).toEqual([{ kind: 'link', text: 'site', url: 'https://example.com/a', style: PLAIN }])
    expect(inline('[x](javascript:alert(1))')).toEqual([text('[x](javascript:alert(1))')])
  })

  test('a tag starts a word; a number and a # inside a word are text', () => {
    expect(inline('#Garden and c#sharp and #12')).toEqual([{ kind: 'tag', text: '#Garden', tag: 'garden' }, text(' and c#sharp and #12')])
  })

  test('a backslash keeps a mark as it is', () => {
    expect(inline('\\*not italic\\*')).toEqual([text('*not italic*')])
  })

  test('markup in a note is text', () => {
    expect(inline('<img src=x onerror=alert(1)>')).toEqual([text('<img src=x onerror=alert(1)>')])
  })
})

describe('a note', () => {
  test('headings, paragraphs with their line breaks, a rule', () => {
    expect(parse('# Title\n\nOne\ntwo\n\n---\n### Deep')).toEqual([
      { kind: 'heading', level: 1, runs: [text('Title')] },
      { kind: 'paragraph', lines: [[text('One')], [text('two')]] },
      { kind: 'rule' },
      { kind: 'heading', level: 3, runs: [text('Deep')] }
    ])
  })

  test('a heading needs a space: #tag is a tag in a paragraph', () => {
    expect(parse('#tag')).toEqual([{ kind: 'paragraph', lines: [[{ kind: 'tag', text: '#tag', tag: 'tag' }]] }])
  })

  test('a list, nested by indent, with tasks that know their line', () => {
    expect(parse('Shopping\n- [ ] Milk\n- [x] Bread\n  1. Rye\n  2. White\n- Eggs')).toEqual([
      { kind: 'paragraph', lines: [[text('Shopping')]] },
      {
        kind: 'list',
        list: {
          ordered: false,
          items: [
            { runs: [text('Milk')], task: false, line: 1, children: [] },
            {
              runs: [text('Bread')],
              task: true,
              line: 2,
              children: [
                {
                  ordered: true,
                  items: [
                    { runs: [text('Rye')], task: null, line: 3, children: [] },
                    { runs: [text('White')], task: null, line: 4, children: [] }
                  ]
                }
              ]
            },
            { runs: [text('Eggs')], task: null, line: 5, children: [] }
          ]
        }
      }
    ])
  })

  test('a quote keeps its lines', () => {
    expect(parse('> One\n> two\nAfter')).toEqual([
      { kind: 'quote', lines: [[text('One')], [text('two')]] },
      { kind: 'paragraph', lines: [[text('After')]] }
    ])
  })

  test('fenced code is as written; a fence never closed runs to the end', () => {
    expect(parse('```js\nconst a = **1**\n\n# no\n```\nAfter')).toEqual([
      { kind: 'code', text: 'const a = **1**\n\n# no' },
      { kind: 'paragraph', lines: [[text('After')]] }
    ])
    expect(parse('```\nopen')).toEqual([{ kind: 'code', text: 'open' }])
  })

  test('an empty note has no blocks', () => {
    expect(parse('')).toEqual([])
    expect(parse('\n  \n')).toEqual([])
  })
})
