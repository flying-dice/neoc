--!strict
-- The Todos app's backend: lists of things to do, in the app's own database (store.luau). Every call that changes
-- something answers with the whole board as it now stands, so the window that asked draws what the backend has.
--
-- An agent can do what a person does most: read the todos (`list_todos`), add one (`add_todo`) and tick one off
-- (`complete_todo`). A tool that changed something tells the open windows with a `todos.changed` event, and they read
-- the board again.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local store = require("./store")
local t = app.t

local handlers: rpc.Handlers = {
  board = function(_ctx)
    return store.board()
  end,

  addList = function(ctx, params)
    local problem = store.addList(params.name, params.color)
    return if problem then ctx:fail(problem) else store.board()
  end,

  removeList = function(ctx, params)
    local problem = store.removeList(params.list)
    return if problem then ctx:fail(problem) else store.board()
  end,

  add = function(ctx, params)
    local _id, problem = store.add(params.list, params.title, { due = params.due, flagged = params.flagged })
    return if problem then ctx:fail(problem) else store.board()
  end,

  update = function(ctx, params)
    local problem = store.update(
      params.todo,
      { title = params.title, notes = params.notes, due = params.due, flagged = params.flagged }
    )
    return if problem then ctx:fail(problem) else store.board()
  end,

  toggle = function(ctx, params)
    local todo = store.todo(params.todo)
    if not todo then
      return ctx:fail("There is no such todo.")
    end
    local problem = store.complete(todo.id, not todo.done)
    return if problem then ctx:fail(problem) else store.board()
  end,

  remove = function(ctx, params)
    local problem = store.remove(params.todo)
    return if problem then ctx:fail(problem) else store.board()
  end,
}

-- a todo as an agent reads it: its list by name
type Listed = { id: number, title: string, list: string, notes: string, due: string, flagged: boolean, done: boolean }

app.tool {
  name = "list_todos",
  title = "List the todos",
  readOnly = true,
  description = "The todos that are still to do, with the list each is in, its notes, the day it is due (2026-06-09, empty for none) and whether it is flagged. Give `list` for one list only, and `completed` for the completed todos instead.",
  input = {
    list = t.string { optional = true, description = "The name of a list; every list when left out" },
    completed = t.boolean { optional = true, description = "True for the completed todos instead of the open ones" },
  },
  run = function(input: { list: string?, completed: boolean? }, ctx)
    local names: { [number]: string } = {}
    for _, list in store.lists() do
      names[list.id] = list.name
    end
    local only: number? = nil
    if input.list ~= nil then
      local list = store.listNamed(input.list)
      if not list then
        return ctx:fail(`There is no list called {input.list}.`)
      end
      only = list.id
    end
    local todos: { Listed } = {}
    local found: { rpc.Todo } = if input.completed then store.completed() else store.open()
    for _, todo in found do
      if only == nil or todo.list == only then
        table.insert(todos, {
          id = todo.id,
          title = todo.title,
          list = names[todo.list] or "",
          notes = todo.notes,
          due = todo.due,
          flagged = todo.flagged,
          done = todo.done,
        })
      end
    end
    return { todos = todos }
  end,
}

app.tool {
  name = "add_todo",
  title = "Add a todo",
  description = "Add a todo to a list (the first list when none is named), with optional notes, a day it is due and a flag.",
  input = {
    title = t.string { description = "What has to be done", maxLength = 500 },
    list = t.string { optional = true, description = "The name of the list to add it to; the first list when left out" },
    notes = t.string { optional = true, description = "Anything worth remembering about it", maxLength = 2000 },
    due = t.string { optional = true, description = "The day it is due, as 2026-06-09" },
    flagged = t.boolean { optional = true, description = "True to flag it" },
  },
  run = function(input: { title: string, list: string?, notes: string?, due: string?, flagged: boolean? }, ctx)
    local list = if input.list ~= nil then store.listNamed(input.list) else store.lists()[1]
    if not list then
      return ctx:fail(`There is no list called {input.list or ""}.`)
    end
    local id, problem =
      store.add(list.id, input.title, { notes = input.notes, due = input.due, flagged = input.flagged })
    if problem then
      return ctx:fail(problem)
    end
    app.emit("todos.changed")
    return { id = id, list = list.name }
  end,
}

app.tool {
  name = "complete_todo",
  title = "Complete a todo",
  description = "Tick a todo off, by its id (`list_todos` gives the ids).",
  input = { id = t.integer { description = "The id of the todo" } },
  run = function(input: { id: number }, ctx)
    local todo = store.todo(input.id)
    if not todo then
      return ctx:fail("There is no such todo.")
    end
    local problem = store.complete(todo.id, true)
    if problem then
      return ctx:fail(problem)
    end
    app.emit("todos.changed")
    return { id = todo.id, title = todo.title, done = true }
  end,
}

return handlers
