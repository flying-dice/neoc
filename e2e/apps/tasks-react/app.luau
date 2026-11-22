--!strict
-- The backend of a fixture app whose client is written in React (`e2e/apps/tasks-svelte` is the same app in Svelte):
-- a task list in the app's own database. The entry returns the handlers a client reaches with `rpc(name, args)`;
-- `add_task` is an agent's tool, which changes the same data and tells the open window with `app.emit`.
local app = require("vos:app")
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
