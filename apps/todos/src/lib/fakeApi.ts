/**
 * A stand-in for the Todos app's backend, for stories and unit tests: the lists and the todos kept in the page, with no
 * host. It follows the backend's rules (server/store.luau): a blank title or name and a day that is not a day are
 * refused with the same sentences, a new board starts with one list called Reminders, the last list cannot be deleted,
 * and a board lists the open todos in the order they were added, then the completed ones, the latest first.
 */
import type { Board, List, Todo } from '@todos/rpc'
import type { TodosEvents, TodosRpc } from './todos.svelte'

export interface FakeBackend {
  readonly rpc: TodosRpc
  readonly events: TodosEvents
  /** Send an event to the window, as the backend (`todos.changed`) or the host (`host:reconnected`) does. */
  emit(name: string, data?: unknown): void
}

export interface FakeOptions {
  /** The lists that are already there; one called Reminders when left out, as on a new board. */
  lists?: readonly List[]
  /** The todos that are already there, in the order they were added. */
  todos?: readonly Todo[]
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {}

export const REMINDERS: List = { id: 1, name: 'Reminders', color: 'blue' }

/** A todo for a story's or a test's starting point: in Reminders, with nothing set but what is given. */
export function todo(id: number, title: string, given: Partial<Omit<Todo, 'id' | 'title'>> = {}): Todo {
  return { id, list: REMINDERS.id, title, notes: '', due: '', flagged: false, done: false, ...given }
}

const isDay = (text: string): boolean => text === '' || /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/.test(text)

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let lists: List[] = (options.lists ?? [REMINDERS]).map((l) => ({ ...l }))
  let todos: Todo[] = (options.todos ?? []).map((t) => ({ ...t }))
  // the completed todos' ids, the most recently completed first
  let completed: number[] = todos.filter((t) => t.done).map((t) => t.id)
  let nextList = 1 + Math.max(0, ...lists.map((l) => l.id))
  let nextTodo = 1 + Math.max(0, ...todos.map((t) => t.id))
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const board = (): Board => ({
    lists: lists.map((l) => ({ ...l })),
    todos: [...todos.filter((t) => !t.done), ...completed.flatMap((id) => todos.filter((t) => t.id === id))].map((t) => ({ ...t }))
  })
  const find = (id: number): Todo => {
    const found = todos.find((t) => t.id === id)
    if (!found) throw new Refused('There is no such todo.')
    return found
  }
  const checked = (title: string, due: string): string => {
    if (title.trim() === '') throw new Refused('A todo needs a title.')
    if (!isDay(due)) throw new Refused('A day is written like 2026-06-09.')
    return title.trim()
  }

  const rpc: TodosRpc = {
    board: async () => board(),
    addList: async ({ name, color }) => {
      if (name.trim() === '') throw new Refused('A list needs a name.')
      lists = [...lists, { id: nextList++, name: name.trim(), color }]
      return board()
    },
    removeList: async ({ list }) => {
      if (!lists.some((l) => l.id === list)) throw new Refused('There is no such list.')
      if (lists.length <= 1) throw new Refused('The last list cannot be deleted.')
      lists = lists.filter((l) => l.id !== list)
      todos = todos.filter((t) => t.list !== list)
      completed = completed.filter((id) => todos.some((t) => t.id === id))
      return board()
    },
    add: async ({ list, title, due = '', flagged = false }) => {
      const titled = checked(title, due)
      if (!lists.some((l) => l.id === list)) throw new Refused('There is no such list.')
      todos = [...todos, { id: nextTodo++, list, title: titled, notes: '', due, flagged, done: false }]
      return board()
    },
    update: async ({ todo: id, title, notes, due, flagged }) => {
      const found = find(id)
      found.title = checked(title ?? found.title, due ?? found.due)
      found.notes = notes ?? found.notes
      found.due = due ?? found.due
      found.flagged = flagged ?? found.flagged
      return board()
    },
    toggle: async ({ todo: id }) => {
      const found = find(id)
      found.done = !found.done
      completed = found.done ? [id, ...completed] : completed.filter((c) => c !== id)
      return board()
    },
    remove: async ({ todo: id }) => {
      find(id)
      todos = todos.filter((t) => t.id !== id)
      completed = completed.filter((c) => c !== id)
      return board()
    }
  }

  const events: TodosEvents = {
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
    }
  }

  return { rpc, events, emit: (name, data) => listeners.get(name)?.forEach((fn) => fn(data)) }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: { board: refuse, addList: refuse, removeList: refuse, add: refuse, update: refuse, toggle: refuse, remove: refuse },
    events: { on: () => () => {} },
    emit: () => {}
  }
}
