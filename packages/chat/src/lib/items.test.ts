import { describe, expect, test } from 'bun:test'
import { entriesOf, iconOf, statusOf, summaryOf } from './items'
import { conversation } from './samples'
import type { Item } from '../types'

const tool = (name: string, done: boolean): Partial<Item> & Pick<Item, 'kind'> => ({ kind: 'tool', name, data: JSON.stringify({ kind: 'execute', status: done ? 'completed' : 'in_progress' }), done })
const running = (...said: Parameters<typeof conversation>[2]) => ({ ...conversation('s', 'Status', [['user', 'Go'], ...said]), busy: true })

describe('the line over the composer', () => {
  test('says nothing when no turn runs', () => {
    expect(statusOf(conversation('s', 'Status', [['user', 'Go'], ['assistant', 'Gone.']]))).toBeNull()
  })

  test('says the agent is thinking while its turn runs with no tool', () => {
    expect(statusOf(running())).toBe('Thinking…')
    expect(statusOf(running(['assistant', 'Half of it']))).toBe('Thinking…')
  })

  test('names the tool that is running', () => {
    expect(statusOf(running(tool('List the projects', true), tool('Run the tests', false)))).toBe('Thinking… · Run the tests')
  })

  test('names a tool with no title as a tool', () => {
    expect(statusOf(running(tool('', false)))).toBe('Thinking… · A tool')
  })

  test('counts the tools when more than one is running', () => {
    expect(statusOf(running(tool('Read a.md', false), tool('Read b.md', false), tool('Read c.md', true)))).toBe('Thinking… · 2 tools running')
  })

  test('says nothing while the turn waits for the person to answer the agent', () => {
    expect(statusOf({ ...running(tool('Delete old.md', false)), waiting: true })).toBeNull()
  })
})

describe('the icon of a tool call', () => {
  test('says what kind of thing it does', () => {
    expect(iconOf('execute')).toBe('terminal')
    expect(iconOf('search')).toBe('magnifyingglass')
    expect(iconOf('fetch')).toBe('magnifyingglass')
    expect(iconOf('edit')).toBe('square.and.pencil')
    expect(iconOf('read')).toBe('doc')
  })

  test('is the generic one for any other kind, or none', () => {
    expect(iconOf('other')).toBe('gear')
    expect(iconOf(undefined)).toBe('gear')
  })
})

const step = (id: number, kind: string | undefined, rest: Partial<Item> = {}): Item => ({
  id,
  chat: 's',
  kind: 'tool',
  text: '',
  name: `Step ${id}`,
  data: JSON.stringify({ ...(kind ? { kind } : {}), status: 'completed' }),
  done: true,
  ...rest
})
const said = (id: number, kind: Item['kind']): Item => ({ id, chat: 's', kind, text: 'Words', name: '', data: '', done: true })
const created = (id: number): Item =>
  step(id, 'edit', { data: JSON.stringify({ kind: 'edit', status: 'completed', content: [{ type: 'diff', path: '/a.md', patch: '+a\n', new_file: true }] }) })
const failed = (id: number, kind: string): Item => step(id, kind, { data: JSON.stringify({ kind, status: 'failed' }) })
const thought = (id: number): Item => ({ ...said(id, 'reasoning'), text: 'Hmm.' })

describe('the chat as it is shown', () => {
  test('makes each run of two or more steps one row, and leaves one step on its own', () => {
    const items = [said(1, 'user'), step(2, 'read'), thought(3), step(4, 'execute'), said(5, 'assistant'), step(6, 'edit'), said(7, 'assistant')]
    const entries = entriesOf(items)
    expect(entries.map((e) => (e.steps ? e.steps.map((s) => s.id) : e.item.id))).toEqual([1, [2, 3, 4], 5, 6, 7])
  })

  test('ends a run at a permission, a plan, a note, an error and what the person says', () => {
    for (const kind of ['permission', 'plan', 'note', 'error', 'user'] as const) {
      const entries = entriesOf([step(1, 'read'), step(2, 'read'), said(3, kind), step(4, 'read'), step(5, 'read')])
      expect(entries.map((e) => (e.steps ? e.steps.length : e.item.kind))).toEqual([2, kind, 2])
    }
  })

  test('keeps a run open at the end, as it grows during the turn', () => {
    expect(entriesOf([said(1, 'user'), step(2, 'read'), step(3, 'read', { done: false })]).at(-1)?.steps?.length).toBe(2)
  })
})

describe('the row of a run of steps', () => {
  test('counts what the steps did, in the order each first came', () => {
    expect(summaryOf([step(1, 'execute'), step(2, 'read'), step(3, 'execute'), step(4, 'read'), step(5, 'execute')]).text).toBe('Ran 3 commands · Read 2 files')
    expect(summaryOf([created(1), step(2, 'edit'), step(3, 'edit'), step(4, 'delete'), step(5, 'delete')]).text).toBe(
      'Created 1 file · Edited 2 files · Deleted 2 files'
    )
  })

  test('says one and many as English does', () => {
    expect(summaryOf([step(1, 'search'), step(2, 'search')]).text).toBe('Searched 2 times')
    expect(summaryOf([step(1, 'search'), step(2, 'fetch')]).text).toBe('Searched once · Fetched 1 page')
    expect(summaryOf([step(1, 'move'), step(2, 'move'), step(3, 'fetch'), step(4, 'fetch')]).text).toBe('Moved 2 files · Fetched 2 pages')
    expect(summaryOf([step(1, 'execute'), step(2, 'delete')]).text).toBe('Ran 1 command · Deleted 1 file')
  })

  test('counts a tool of another kind, or none, as a tool used, and a thought as thinking', () => {
    expect(summaryOf([step(1, 'other'), step(2, undefined), thought(3)]).text).toBe('Used 2 tools · Thought once')
    expect(summaryOf([thought(1), step(2, 'think'), thought(3)]).text).toBe('Thought 3 times')
  })

  test('of one step is its title', () => {
    expect(summaryOf([step(1, 'execute')]).text).toBe('Step 1')
    expect(summaryOf([thought(1)]).text).toBe('Thought')
  })

  test('counts the steps that failed', () => {
    expect(summaryOf([step(1, 'execute'), failed(2, 'execute'), failed(3, 'read')]).failed).toBe(2)
    expect(summaryOf([step(1, 'execute'), step(2, 'read')]).failed).toBe(0)
  })

  test('has the icon of the kind most of its steps are, the latest of those that tie', () => {
    expect(summaryOf([step(1, 'read'), step(2, 'execute'), step(3, 'execute')]).icon).toBe('terminal')
    expect(summaryOf([step(1, 'execute'), step(2, 'read')]).icon).toBe('doc')
    expect(summaryOf([created(1), step(2, 'read')]).icon).toBe('doc')
    expect(summaryOf([step(1, 'read'), created(2)]).icon).toBe('square.and.pencil')
  })
})
