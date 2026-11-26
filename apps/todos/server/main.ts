// The Todos app's backend: lists of things to do, in the app's own database (store.ts). Every call that changes
// something answers with the whole board as it now stands, so the window that asked draws what the backend has.
//
// The page's methods are private (`_board`, `_add_list`, ...). An agent can do what a person does most: read the todos
// (`list_todos`), add one (`add_todo`) and tick one off (`complete_todo`). A tool that changed something tells the open
// windows with the `todos_changed` notification, and they read the board again.
//
// It is written in TypeScript: the build compiles it to the Luau the host runs (`vos()` in vite.config.ts), against the
// types the plugin writes (`types/vos.backend.d.ts`): the global `RPC` serves and sends on the bus, and a method, params
// or result that the contract (openrpc.json) does not have is a type error.
import * as store from './store'

/** The board as it now stands, unless the change was refused: then the call fails with why. */
const answer = (ctx: vos_app_Context, problem: string | undefined): todos_Board => (problem === undefined ? store.board() : ctx.fail(problem))

RPC.on_request('_board', () => store.board())

RPC.on_request('_add_list', (params, ctx) => answer(ctx, store.addList(params.name, params.color)))

RPC.on_request('_remove_list', (params, ctx) => answer(ctx, store.removeList(params.list)))

RPC.on_request('_add', (params, ctx) => {
  const added = store.add(params.list, params.title, { due: params.due, flagged: params.flagged })
  return answer(ctx, 'problem' in added ? added.problem : undefined)
})

RPC.on_request('_update', (params, ctx) =>
  answer(ctx, store.update(params.todo, { title: params.title, notes: params.notes, due: params.due, flagged: params.flagged }))
)

RPC.on_request('_toggle', (params, ctx) => {
  const todo = store.todo(params.todo)
  if (todo === undefined) return ctx.fail('There is no such todo.')
  return answer(ctx, store.complete(todo.id, !todo.done))
})

RPC.on_request('_remove', (params, ctx) => answer(ctx, store.remove(params.todo)))

RPC.on_request('list_todos', (params, ctx) => {
  const names = new Map(store.lists().map((list) => [list.id, list.name]))
  let only: number | undefined
  if (params.list !== undefined) {
    const list = store.listNamed(params.list)
    if (list === undefined) return ctx.fail(`There is no list called ${params.list}.`)
    only = list.id
  }
  const found = params.completed ? store.completed() : store.open()
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
})

RPC.on_request('add_todo', (params, ctx) => {
  const list = params.list !== undefined ? store.listNamed(params.list) : store.lists()[0]
  if (list === undefined) return ctx.fail(`There is no list called ${params.list ?? ''}.`)
  const added = store.add(list.id, params.title, { notes: params.notes, due: params.due, flagged: params.flagged })
  if ('problem' in added) return ctx.fail(added.problem)
  RPC.send_notification('todos_changed')
  return { id: added.id, list: list.name }
})

RPC.on_request('complete_todo', (params, ctx) => {
  const todo = store.todo(params.id)
  if (todo === undefined) return ctx.fail('There is no such todo.')
  const problem = store.complete(todo.id, true)
  if (problem !== undefined) return ctx.fail(problem)
  RPC.send_notification('todos_changed')
  return { id: todo.id, title: todo.title, done: true }
})
