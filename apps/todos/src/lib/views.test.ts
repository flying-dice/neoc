import { describe, expect, test } from 'bun:test'
import { todo } from './fakeApi'
import { listView, sameView, shown, smart } from './views'

const TODAY = '2026-06-09'

// as a board lists them: the open ones in the order they were added, then the completed ones
const TODOS = [
  todo(1, 'Plain'),
  todo(2, 'Next week', { due: '2026-06-16' }),
  todo(3, 'Overdue', { due: '2026-06-01', flagged: true }),
  todo(4, 'Today', { due: TODAY }),
  todo(5, 'In another list', { list: 2, flagged: true }),
  todo(6, 'Done', { done: true, due: TODAY, flagged: true })
]

const titles = (view: Parameters<typeof shown>[1]): string[] => shown(TODOS, view, TODAY).map((t) => t.title)

describe('what a view shows', () => {
  test('a list: its own todos that are still to do', () => {
    expect(titles(listView(1))).toEqual(['Plain', 'Next week', 'Overdue', 'Today'])
    expect(titles(listView(2))).toEqual(['In another list'])
    expect(titles(listView(3))).toEqual([])
  })

  test('Today: what is due today, and what is overdue', () => {
    expect(titles(smart('today'))).toEqual(['Overdue', 'Today'])
  })

  test('Scheduled: everything with a day, in order of the day', () => {
    expect(titles(smart('scheduled'))).toEqual(['Overdue', 'Today', 'Next week'])
  })

  test('All: everything still to do, from every list', () => {
    expect(titles(smart('all'))).toEqual(['Plain', 'Next week', 'Overdue', 'Today', 'In another list'])
  })

  test('Flagged: the flagged todos still to do', () => {
    expect(titles(smart('flagged'))).toEqual(['Overdue', 'In another list'])
  })

  test('Completed: only what is done', () => {
    expect(titles(smart('completed'))).toEqual(['Done'])
  })
})

describe('two views', () => {
  test('are the same when they are the same kind of the same list', () => {
    expect(sameView(smart('all'), smart('all'))).toBe(true)
    expect(sameView(smart('all'), smart('today'))).toBe(false)
    expect(sameView(listView(1), listView(1))).toBe(true)
    expect(sameView(listView(1), listView(2))).toBe(false)
  })
})
