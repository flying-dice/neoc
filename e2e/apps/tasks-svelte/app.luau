--!strict
-- The backend of a fixture app whose client is written in Svelte (`e2e/apps/tasks-react` is the same app in React):
-- a task list in the app's own database. The entry returns the handlers a client reaches with `rpc(name, args)`;
-- `add_task` is an agent's tool, which changes the same data and tells the open window with `app.emit`.
local app = require("vos:app")
local task = require("std:task")
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

local function list(): { any }
  local tasks = {}
  for _, row in db:all("select id, title, done from tasks order by id") do
    table.insert(tasks, { id = row.id, title = row.title, done = row.done == 1 })
  end
  return tasks
end

local function titleOf(args: any): string
  if type(args) == "table" and type(args.title) == "string" then
    return string.match(args.title, "^%s*(.-)%s*$") :: string
  end
  return ""
end

-- `hang` waits until `release` is called (the journeys' stand-in for a handler that waits on something that never answers)
local generation = 0

local handlers: vos_Handlers = {
  list = function(_ctx, _args)
    return list()
  end,
  add = function(ctx, args)
    local title = titleOf(args)
    if title == "" then
      return ctx:fail("A task needs a title.")
    end
    db:exec("insert into tasks (title) values (:title)", { title = title })
    return list()
  end,
  toggle = function(ctx, args)
    if type(args) ~= "table" or type(args.id) ~= "number" then
      return ctx:fail("toggle needs the id of a task.")
    end
    db:exec("update tasks set done = 1 - done where id = :id", { id = args.id })
    return list()
  end,
  hang = function(_ctx, _args)
    local mine = generation
    while generation == mine do
      task.wait(0.05)
    end
    return "released"
  end,
  release = function(_ctx, _args)
    generation += 1
    return "ok"
  end,
  whoami = function(ctx, _args)
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

-- the way out of a hung app for the journeys: a tool is not a call of the client, so it does not wait behind the eight that hang
app.tool {
  name = "release_all",
  title = "Release the hung calls",
  description = "Let every call of `hang` that is waiting return.",
  input = {},
  run = function(_input, _ctx)
    generation += 1
    return { released = true }
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
