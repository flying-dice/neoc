import { describe, expect, test } from 'bun:test'
import { column, suggestKey, tree } from './catalog'
import { issue, page } from './fakeApi'

describe('a column of the board', () => {
  const issues = [
    issue(1, 'Low', { priority: 'low' }),
    issue(2, 'Medium'),
    issue(3, 'High', { priority: 'high' }),
    issue(4, 'Also high', { priority: 'high' }),
    issue(5, 'Under way', { status: 'doing' })
  ]

  test('holds the issues of its status, the most pressing first, then in the order they were added', () => {
    expect(column(issues, 'todo').map((i) => i.title)).toEqual(['High', 'Also high', 'Medium', 'Low'])
    expect(column(issues, 'doing').map((i) => i.title)).toEqual(['Under way'])
    expect(column(issues, 'done')).toEqual([])
  })
})

describe('the pages of a project', () => {
  test('are listed each followed by the pages under it, saying what it is under', () => {
    const pages = [page(1, 'Design'), page(2, 'Colours', { parent: 1 }), page(3, 'Engineering'), page(4, 'Dark mode', { parent: 2 })]
    expect(tree(pages).map((l) => [l.page.title, l.depth, l.path.join('/')])).toEqual([
      ['Design', 0, ''],
      ['Colours', 1, 'Design'],
      ['Dark mode', 2, 'Design/Colours'],
      ['Engineering', 0, '']
    ])
  })

  test('keep a page whose parent is gone, at the top', () => {
    expect(tree([page(2, 'Orphan', { parent: 9 })]).map((l) => [l.page.title, l.depth])).toEqual([['Orphan', 0]])
  })

  test('keep pages that are under each other in a circle', () => {
    const pages = [page(1, 'A', { parent: 2 }), page(2, 'B', { parent: 1 })]
    expect(tree(pages).map((l) => l.page.title).sort()).toEqual(['A', 'B'])
  })
})

describe('the key a name is offered', () => {
  test.each([
    ['Website', 'WEB'],
    ['Web Site', 'WS'],
    ['mobile app redesign', 'MAR'],
    ['a b c d e f g', 'ABCDE'],
    ['2026 Plans', 'P'],
    ['', ''],
    ['!!!', '']
  ])('%p gives %p', (name, key) => {
    expect(suggestKey(name)).toBe(key)
  })
})
