--!strict
-- The backend of a fixture app whose client is written in React (`e2e/apps/tasks-svelte` is the same app in Svelte): a task list in
-- the app's own database. It serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`
-- from openrpc.json: the page's methods are private (`_list`, `_add`, ...), and `add_task` is an agent's tool, which
-- changes the same data and tells the open window with the `tasks_changed` notification.
local trim = require("lib:text").trim
local task = require("std:task")
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

local function list(): { tasks_react_Task }
  local tasks: { tasks_react_Task } = {}
  for _, row in db:all("select id, title, done from tasks order by id") do
    table.insert(tasks, { id = row.id, title = row.title, done = row.done == 1 })
  end
  return tasks
end

-- `_hang` waits until `_release` lets it go: the journeys' stand-in for a handler that waits on something that never answers
local generation = 0

RPC.on_request("_list", function(): tasks_react_ListResult
  return list()
end)

RPC.on_request("_add", function(params: tasks_react_AddParams, ctx: vos_app_Context): tasks_react_AddResult
  local title = trim(params.title)
  if title == "" then
    return ctx:fail("A task needs a title.")
  end
  db:exec("insert into tasks (title) values (:title)", { title = title })
  return list()
end)

RPC.on_request("_toggle", function(params: tasks_react_ToggleParams): tasks_react_ToggleResult
  db:exec("update tasks set done = 1 - done where id = :id", { id = params.id })
  return list()
end)

RPC.on_request("_hang", function(): tasks_react_HangResult
  local mine = generation
  while generation == mine do
    task.wait(0.05)
  end
  return "released"
end)

RPC.on_request("_release", function(): tasks_react_ReleaseResult
  generation += 1
  return "ok"
end)

RPC.on_request("_whoami", function(_params: {}, ctx: vos_app_Context): tasks_react_WhoamiResult
  return { kind = ctx.actor.kind, app = app.id }
end)

-- for the Files journeys (e2e/files.journeys.spec.ts): a document for the host's Files page to list, download and remove
RPC.on_request(
  "_save_document",
  function(params: tasks_react_SaveDocumentParams, ctx: vos_app_Context): tasks_react_SaveDocumentResult
    local docs = app.files.documents
    local folder = string.match(params.path, "^(.*)/[^/]+$")
    if folder then
      local _, why = docs:create_dir(folder)
      if why then
        return ctx:fail(`The file could not be written: {why.message}`)
      end
    end
    local _, why = docs:write(params.path, params.text)
    if why then
      return ctx:fail(`The file could not be written: {why.message}`)
    end
    return true
  end
)

RPC.on_request("add_task", function(params: tasks_react_AddTaskParams): tasks_react_AddTaskResult
  db:exec("insert into tasks (title) values (:title)", { title = params.title })
  RPC.send_notification("tasks_changed")
  return { tasks = #list() }
end)

RPC.on_request("list_tasks", function(): tasks_react_ListTasksResult
  return { tasks = list() }
end)
