--!strict
-- The backend of a fixture app whose client is written in React (`e2e/apps/tasks-svelte` is the same app in Svelte): a task list in
-- the app's own database. The RPC is defined once, in openrpc.json; the plugin generates `.app/rpc.luau` from it, with the
-- `Handlers` interface this file implements; it returns the handlers, and the generated entry puts the router in front. `add_task` is an agent's tool, which
-- changes the same data and tells the open window with `app.emit`.
local app = require("vos:app")
local task = require("std:task")
local rpc = require("../.app/rpc")
local t = app.t
local db = app.db

db:migrate {
  [1] = [[
    create table tasks (
      id integer primary key,
      title text not null,
      done integer not null default 0
    );
  ]],
}

local function list(): { rpc.Task }
  local tasks: { rpc.Task } = {}
  for _, row in db:all("select id, title, done from tasks order by id") do
    table.insert(tasks, { id = row.id, title = row.title, done = row.done == 1 })
  end
  return tasks
end

local function trim(text: string): string
  return string.match(text, "^%s*(.-)%s*$") :: string
end

-- `hang` waits until `release_all` (an agent's tool) lets it go: the journeys' stand-in for a handler that waits on something that never answers
local generation = 0

local handlers: rpc.Handlers = {
  list = function(_ctx)
    return list()
  end,
  add = function(ctx, params)
    local title = trim(params.title)
    if title == "" then
      return ctx:fail("A task needs a title.")
    end
    db:exec("insert into tasks (title) values (:title)", { title = title })
    return list()
  end,
  toggle = function(_ctx, params)
    db:exec("update tasks set done = 1 - done where id = :id", { id = params.id })
    return list()
  end,
  hang = function(_ctx)
    local mine = generation
    while generation == mine do
      task.wait(0.05)
    end
    return "released"
  end,
  release = function(_ctx)
    generation += 1
    return "ok"
  end,
  whoami = function(ctx)
    return { kind = ctx.actor.kind, app = app.id }
  end,
}

app.tool {
  name = "add_task",
  title = "Add a task",
  description = "Add a task to the list.",
  input = { title = t.string { description = "What has to be done" } },
  run = function(input, _ctx)
    db:exec("insert into tasks (title) values (:title)", { title = input.title })
    app.emit("tasks.changed")
    return { tasks = #list() }
  end,
}

app.tool {
  name = "list_tasks",
  title = "List the tasks",
  readOnly = true,
  description = "The tasks on the list, with whether each is done.",
  input = {},
  run = function(_input, _ctx)
    return { tasks = list() }
  end,
}

return handlers
