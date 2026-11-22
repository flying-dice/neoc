import { describe, expect, test } from 'bun:test'
import { REMINDERS, createFailingBackend, createFakeBackend, todo, type FakeBackend } from './fakeApi'
import { Todos } from './todos.svelte'
import { listView, smart } from './views'

const TODAY = '2026-06-09'
const NOW = (): Date => new Date(2026, 5, 9, 9, 41)
const WORK = { id: 2, name: 'Work', color: 'red' } as const

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Todos> {
  const todos = new Todos(backend.rpc, backend.events, NOW)
  todos.listen()
  await todos.load()
  return todos
}

/** Wait until `done()` holds (an event is answered by a read of the board). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const titles = (todos: Todos): string[] => todos.todos.map((t) => t.title)

async function type(todos: Todos, title: string): Promise<void> {
  todos.draft = title
  await todos.add()
}

describe('a window that has just opened', () => {
  test('shows Today, empty, with the one list a new board has', async () => {
    const todos = await opened()
    expect(todos.lists).toEqual([REMINDERS])
    expect(todos.view).toEqual(smart('today'))
    expect(todos.title).toBe('Today')
    expect(todos.todos).toEqual([])
    expect(todos.today).toBe(TODAY)
    expect(todos.failure).toBeNull()
    expect(todos.browsing).toBe(true)
    expect(todos.addable).toBe(true)
  })

  test('counts what each list holds', async () => {
    const todos = await opened(
      createFakeBackend({
        lists: [REMINDERS, WORK],
        todos: [todo(1, 'Milk', { due: TODAY }), todo(2, 'Report', { list: 2, flagged: true }), todo(3, 'Old', { done: true })]
      })
    )
    expect(todos.counts).toEqual({ today: 1, scheduled: 1, all: 2, flagged: 1, completed: 1 })
    expect(todos.remaining.get(1)).toBe(1)
    expect(todos.remaining.get(2)).toBe(1)
  })
})

describe('adding a todo', () => {
  test('in a list: it goes in that list, with nothing set, and the field is emptied', async () => {
    const todos = await opened(createFakeBackend({ lists: [REMINDERS, WORK] }))
    todos.select(listView(2))
    await type(todos, '  Write the report  ')
    expect(todos.draft).toBe('')
    expect(todos.todos).toEqual([todo(1, 'Write the report', { list: 2 })])
    expect(todos.title).toBe('Work')
  })

  test('in Today: it goes in the first list and is due today', async () => {
    const todos = await opened(createFakeBackend({ lists: [REMINDERS, WORK] }))
    await type(todos, 'Milk')
    expect(todos.todos).toEqual([todo(1, 'Milk', { due: TODAY })])
  })

  test('in Flagged: it is flagged', async () => {
    const todos = await opened()
    todos.select(smart('flagged'))
    await type(todos, 'Passport')
    expect(todos.todos).toEqual([todo(1, 'Passport', { flagged: true })])
  })

  test('nothing is added from a blank field', async () => {
    const todos = await opened()
    await type(todos, '   ')
    todos.select(smart('all'))
    expect(todos.todos).toEqual([])
    expect(todos.failure).toBeNull()
  })

  test('nothing can be added among the completed todos', async () => {
    const todos = await opened()
    todos.select(smart('completed'))
    expect(todos.addable).toBe(false)
    await type(todos, 'Milk')
    expect(todos.draft).toBe('Milk')
    expect(todos.counts.all).toBe(0)
  })
})

describe('a todo', () => {
  const started = (): Promise<Todos> =>
    opened(createFakeBackend({ todos: [todo(1, 'Milk'), todo(2, 'Bread')] })).then((todos) => {
      todos.select(listView(1))
      return todos
    })

  test('ticked off, it leaves its list for Completed; ticked again, it is back', async () => {
    const todos = await started()
    await todos.toggle(todos.todos[0]!)
    expect(titles(todos)).toEqual(['Bread'])
    todos.select(smart('completed'))
    expect(titles(todos)).toEqual(['Milk'])
    await todos.toggle(todos.todos[0]!)
    expect(todos.todos).toEqual([])
    todos.select(listView(1))
    expect(titles(todos)).toEqual(['Milk', 'Bread'])
  })

  test('the most recently completed is first among the completed', async () => {
    const todos = await started()
    await todos.toggle(todos.todos[0]!)
    await todos.toggle(todos.todos[0]!)
    todos.select(smart('completed'))
    expect(titles(todos)).toEqual(['Bread', 'Milk'])
  })

  test('is flagged and unflagged', async () => {
    const todos = await started()
    await todos.flag(todos.todos[0]!)
    expect(todos.todos[0]?.flagged).toBe(true)
    expect(todos.counts.flagged).toBe(1)
    await todos.flag(todos.todos[0]!)
    expect(todos.counts.flagged).toBe(0)
  })

  test('is renamed; a blank title and the same title change nothing', async () => {
    const todos = await started()
    await todos.rename(todos.todos[0]!, ' Oat milk ')
    expect(titles(todos)).toEqual(['Oat milk', 'Bread'])
    await todos.rename(todos.todos[0]!, '   ')
    await todos.rename(todos.todos[0]!, 'Oat milk')
    expect(titles(todos)).toEqual(['Oat milk', 'Bread'])
    expect(todos.failure).toBeNull()
  })

  test('is given notes and a day, and the day is taken away again', async () => {
    const todos = await started()
    await todos.annotate(todos.todos[0]!, 'The blue carton')
    await todos.schedule(todos.todos[0]!, TODAY)
    expect(todos.todos[0]).toEqual(todo(1, 'Milk', { notes: 'The blue carton', due: TODAY }))
    expect(todos.counts.today).toBe(1)
    await todos.schedule(todos.todos[0]!, '')
    expect(todos.counts.today).toBe(0)
  })

  test('a day that is not a day is refused with the reason, and nothing changes', async () => {
    const todos = await started()
    await todos.schedule(todos.todos[0]!, 'soon')
    expect(todos.failure).toBe('A day is written like 2026-06-09.')
    expect(todos.todos[0]?.due).toBe('')
  })

  test('its details open, close, and close when another list is shown', async () => {
    const todos = await started()
    const [milk, bread] = todos.todos
    todos.expand(milk!)
    expect(todos.expanded).toBe(1)
    todos.expand(bread!)
    expect(todos.expanded).toBe(2)
    todos.expand(bread!)
    expect(todos.expanded).toBeNull()
    todos.expand(milk!)
    todos.select(smart('all'))
    expect(todos.expanded).toBeNull()
  })

  test('is deleted, and its open details go with it', async () => {
    const todos = await started()
    todos.expand(todos.todos[0]!)
    await todos.remove(todos.todos[0]!)
    expect(titles(todos)).toEqual(['Bread'])
    expect(todos.expanded).toBeNull()
  })
})

describe('lists', () => {
  test('a new list takes the next colour and is shown', async () => {
    const todos = await opened()
    todos.listDraft = ' Work '
    await todos.addList()
    expect(todos.listDraft).toBe('')
    expect(todos.lists).toEqual([REMINDERS, { id: 2, name: 'Work', color: 'red' }])
    expect(todos.view).toEqual(listView(2))
    expect(todos.title).toBe('Work')
    expect(todos.browsing).toBe(false)
  })

  test('no list is added from a blank name', async () => {
    const todos = await opened()
    todos.listDraft = '  '
    await todos.addList()
    expect(todos.lists).toEqual([REMINDERS])
  })

  test('deleting the list that is shown deletes its todos and goes on to All', async () => {
    const todos = await opened(
      createFakeBackend({ lists: [REMINDERS, WORK], todos: [todo(1, 'Milk'), todo(2, 'Report', { list: 2 })] })
    )
    todos.select(listView(2))
    await todos.removeList(WORK)
    expect(todos.lists).toEqual([REMINDERS])
    expect(todos.view).toEqual(smart('all'))
    expect(titles(todos)).toEqual(['Milk'])
    expect(todos.browsing).toBe(true)
  })

  test('the last list cannot be deleted', async () => {
    const todos = await opened()
    todos.select(listView(1))
    await todos.removeList(REMINDERS)
    expect(todos.failure).toBe('The last list cannot be deleted.')
    expect(todos.lists).toEqual([REMINDERS])
    expect(todos.view).toEqual(listView(1))
  })

  test('a smart list says which list a todo is in', async () => {
    const todos = await opened(createFakeBackend({ lists: [REMINDERS, WORK], todos: [todo(1, 'Report', { list: 2 })] }))
    todos.select(smart('all'))
    expect(todos.listName(todos.todos[0]!)).toBe('Work')
  })

  test('back leaves the todos for the lists, and keeps which list was shown', async () => {
    const todos = await opened()
    todos.select(smart('all'))
    expect(todos.browsing).toBe(false)
    todos.back()
    expect(todos.browsing).toBe(true)
    expect(todos.view).toEqual(smart('all'))
  })
})

describe('changes from outside the window', () => {
  test('an agent adds a todo: the window reads the board again', async () => {
    const backend = createFakeBackend()
    const todos = await opened(backend)
    todos.select(smart('all'))
    await backend.rpc.add({ list: 1, title: 'From the agent' })
    expect(todos.todos).toEqual([])
    backend.emit('todos.changed')
    await until(() => todos.todos.length === 1)
    expect(titles(todos)).toEqual(['From the agent'])
  })

  test('the host connection comes back: the board is read again', async () => {
    const backend = createFakeBackend()
    const todos = await opened(backend)
    todos.select(smart('all'))
    await backend.rpc.add({ list: 1, title: 'While away' })
    backend.emit('host:reconnected')
    await until(() => todos.todos.length === 1)
  })

  test('the list that is shown was deleted elsewhere: the window goes on to All', async () => {
    const backend = createFakeBackend({ lists: [REMINDERS, WORK] })
    const todos = await opened(backend)
    todos.select(listView(2))
    await backend.rpc.removeList({ list: 2 })
    backend.emit('todos.changed')
    await until(() => todos.view.kind === 'smart')
    expect(todos.view).toEqual(smart('all'))
  })

  test('after `listen` is stopped, events are not heard', async () => {
    const backend = createFakeBackend()
    const todos = new Todos(backend.rpc, backend.events, NOW)
    const stop = todos.listen()
    await todos.load()
    stop()
    await backend.rpc.add({ list: 1, title: 'Unheard', due: TODAY })
    backend.emit('todos.changed')
    await Bun.sleep(10)
    expect(todos.todos).toEqual([])
  })

  test('today is read from the clock each time the board is', async () => {
    let now = new Date(2026, 5, 9, 23, 59)
    const backend = createFakeBackend({ todos: [todo(1, 'Tomorrow', { due: '2026-06-10' })] })
    const todos = new Todos(backend.rpc, backend.events, () => now)
    await todos.load()
    expect(todos.todos).toEqual([])
    now = new Date(2026, 5, 10, 0, 1)
    await todos.load()
    expect(todos.today).toBe('2026-06-10')
    expect(titles(todos)).toEqual(['Tomorrow'])
  })
})

describe('a backend that cannot be reached', () => {
  test('loading says why and shows nothing', async () => {
    const todos = await opened(createFailingBackend('The host is not answering'))
    expect(todos.failure).toBe('The host is not answering')
    expect(todos.lists).toEqual([])
    expect(todos.addable).toBe(false)
  })

  test('a todo that could not be added is back in the field, with the reason', async () => {
    const backend = createFakeBackend()
    const todos = new Todos({ ...backend.rpc, add: () => Promise.reject(new Error('Connection lost')) }, backend.events, NOW)
    await todos.load()
    await type(todos, 'Milk')
    expect(todos.draft).toBe('Milk')
    expect(todos.failure).toBe('Connection lost')
  })

  test('a list that could not be added is back in the field', async () => {
    const backend = createFakeBackend()
    const todos = new Todos({ ...backend.rpc, addList: () => Promise.reject(new Error('Connection lost')) }, backend.events, NOW)
    await todos.load()
    todos.listDraft = 'Work'
    await todos.addList()
    expect(todos.listDraft).toBe('Work')
    expect(todos.failure).toBe('Connection lost')
  })

  test('a rejection that is not an Error is shown as text; the next call that works clears it', async () => {
    const backend = createFakeBackend()
    let down = true
    const todos = new Todos({ ...backend.rpc, board: () => (down ? Promise.reject('offline') : backend.rpc.board()) }, backend.events, NOW)
    await todos.load()
    expect(todos.failure).toBe('offline')
    down = false
    await todos.load()
    expect(todos.failure).toBeNull()
  })
})
