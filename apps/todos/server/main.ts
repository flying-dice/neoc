// The Todos app's backend: lists of things to do, in the app's own database (store.ts). Every call that changes
// something answers with the whole board as it now stands, so the window that asked draws what the backend has.
//
// An agent can do what a person does most: read the todos (`list_todos`), add one (`add_todo`) and tick one off
// (`complete_todo`). A tool that changed something tells the open windows with a `todos.changed` event, and they read
// the board again.
//
// It is written in TypeScript: the build compiles it to the Luau the host runs (`vos()` in vite.config.ts).
import { emit, t, tool } from 'vos:app'
import type { Board, Context, Handlers } from '../.app/server/rpc'
import * as store from './store'

/** The board as it now stands, unless the change was refused: then the call fails with why. */
const answer = (ctx: Context, problem: string | undefined): Board => (problem === undefined ? store.board() : ctx.fail(problem))

const handlers: Handlers = {
  board: () => store.board(),

  addList: (ctx, params) => answer(ctx, store.addList(params.name, params.color)),

  removeList: (ctx, params) => answer(ctx, store.removeList(params.list)),

  add: (ctx, params) => {
    const added = store.add(params.list, params.title, { due: params.due, flagged: params.flagged })
    return answer(ctx, 'problem' in added ? added.problem : undefined)
  },

  update: (ctx, params) =>
    answer(ctx, store.update(params.todo, { title: params.title, notes: params.notes, due: params.due, flagged: params.flagged })),

  toggle: (ctx, params) => {
    const todo = store.todo(params.todo)
    if (todo === undefined) return ctx.fail('There is no such todo.')
    return answer(ctx, store.complete(todo.id, !todo.done))
  },

  remove: (ctx, params) => answer(ctx, store.remove(params.todo))
}

tool({
  name: 'list_todos',
  title: 'List the todos',
  readOnly: true,
  description:
    'The todos that are still to do, with the list each is in, its notes, the day it is due (2026-06-09, empty for none) and whether it is flagged. Give `list` for one list only, and `completed` for the completed todos instead.',
  input: {
    list: t.string({ optional: true, description: 'The name of a list; every list when left out' }),
    completed: t.boolean({ optional: true, description: 'True for the completed todos instead of the open ones' })
  },
  run: (input: { list?: string; completed?: boolean }, ctx) => {
    const names = new Map(store.lists().map((list) => [list.id, list.name]))
    let only: number | undefined
    if (input.list !== undefined) {
      const list = store.listNamed(input.list)
      if (list === undefined) return ctx.fail(`There is no list called ${input.list}.`)
      only = list.id
    }
    const found = input.completed ? store.completed() : store.open()
    // a todo as an agent reads it: its list by name
    const todos = found
      .filter((todo) => only === undefined || todo.list === only)
      .map((todo) => ({
        id: todo.id,
        title: todo.title,
        list: names.get(todo.list) ?? '',
        notes: todo.notes,
        due: todo.due,
        flagged: todo.flagged,
        done: todo.done
      }))
    return { todos }
  }
})

tool({
  name: 'add_todo',
  title: 'Add a todo',
  description: 'Add a todo to a list (the first list when none is named), with optional notes, a day it is due and a flag.',
  input: {
    title: t.string({ description: 'What has to be done', maxLength: 500 }),
    list: t.string({ optional: true, description: 'The name of the list to add it to; the first list when left out' }),
    notes: t.string({ optional: true, description: 'Anything worth remembering about it', maxLength: 2000 }),
    due: t.string({ optional: true, description: 'The day it is due, as 2026-06-09' }),
    flagged: t.boolean({ optional: true, description: 'True to flag it' })
  },
  run: (input: { title: string; list?: string; notes?: string; due?: string; flagged?: boolean }, ctx) => {
    const list = input.list !== undefined ? store.listNamed(input.list) : store.lists()[0]
    if (list === undefined) return ctx.fail(`There is no list called ${input.list ?? ''}.`)
    const added = store.add(list.id, input.title, { notes: input.notes, due: input.due, flagged: input.flagged })
    if ('problem' in added) return ctx.fail(added.problem)
    emit('todos.changed')
    return { id: added.id, list: list.name }
  }
})

tool({
  name: 'complete_todo',
  title: 'Complete a todo',
  description: 'Tick a todo off, by its id (`list_todos` gives the ids).',
  input: { id: t.integer({ description: 'The id of the todo' }) },
  run: (input: { id: number }, ctx) => {
    const todo = store.todo(input.id)
    if (todo === undefined) return ctx.fail('There is no such todo.')
    const problem = store.complete(todo.id, true)
    if (problem !== undefined) return ctx.fail(problem)
    emit('todos.changed')
    return { id: todo.id, title: todo.title, done: true }
  }
})

export default handlers
