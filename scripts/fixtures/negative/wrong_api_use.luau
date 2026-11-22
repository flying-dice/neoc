--!strict
-- Deliberately wrong uses of the `app` API. `bun run lint:luau` requires luau-lsp to report a finding on every line
-- that ends in an expect-error marker (the marker names the text the message must contain; a line with two findings
-- carries two markers) and none elsewhere. If the generated declarations stop loading or stop constraining `app`,
-- these lines check clean and the gate fails.
local app = require("vos:app")
local json = require("lib:json")

app.notify { title = 123 } -- expect-error: Expected this to be exactly 'vos_Notification'
app.db:exec("DELETE FROM notes", "not a list") -- expect-error: Expected this to be 'vos_SqlParams?', but got 'string'
app.nope() -- expect-error: Key 'nope' not found in table 'vos_AppApi'
local _rows: number = app.db:all("SELECT 1") -- expect-error: Expected this to be 'number'
app.tool { name = "x", run = "not a function" } -- expect-error: Expected this to be exactly 'vos_ToolDef'
app.data.query("SELECT 1") -- expect-error: Key 'data' not found in table 'vos_AppApi'
-- what a host that drew apps' windows itself offered: an app declares nothing in Luau and draws its own page
app.manifest { id = "x", name = "X", icon = "gear" } -- expect-error: Key 'manifest' not found in table 'vos_AppApi'
app.window("main", { render = function() end }) -- expect-error: Key 'window' not found in table 'vos_AppApi'
local _state = app.state { count = 1 } -- expect-error: Key 'state' not found in table 'vos_AppApi'
local _open = app:windows("main") -- expect-error: Key 'windows' not found in table 'vos_AppApi'
local _live = app.db:query("select 1 as id", { key = "id" }) -- expect-error: Key 'query' not found in table 'vos_Db'
app:openWindow(5) -- expect-error: Expected this to be 'string?', but got 'number'
app.emit(5) -- expect-error: Expected this to be 'string', but got 'number'
app.t.string { description = 5 } -- expect-error: Expected this to be 'vos_SchemaOptions?'
local _name: string = json.nope -- expect-error: Key 'nope' not found in table 'lib_json'
app.files.documents:write("a.txt") -- expect-error: expects 3 arguments, but only 2 are specified
app.files.documents:remove("a", { recursive = "yes" }) -- expect-error: Expected this to be '{ recursive: boolean? }?' but got
app.files.photos:read("a.jpg") -- expect-error: Key 'photos' not found in table 'vos_FilesApi'
app.files.cache:createDir(5) -- expect-error: Expected this to be 'string', but got 'number'
app.timer.every(60, function() end) -- expect-error: Key 'timer' not found in table 'vos_AppApi'
app.shell.deepLink("note/:id", function() end) -- expect-error: Key 'shell' not found in table 'vos_AppApi'
app.on("event", function() end) -- expect-error: Key 'on' not found in table 'vos_AppApi'
app.notify { title = "later", at = "2026-01-01T09:00:00Z" } -- expect-error: but got '{| at: string, title: string |}'
app.notify { title = "tap", actions = { "Open" } } -- expect-error: but got '{| actions: {string}, title: string |}'
app.notify { title = "fine", body = "this line is correct" }
app:openWindow()
app.emit("fine", { changed = true })
app.tool {
  name = "fine",
  input = { id = app.t.integer { min = 1 } },
  run = function(_, ctx)
    return ctx:fail("never returns")
  end,
}

-- what the entry returns: the handlers its page calls
local handlers: vos_Handlers = { -- expect-error: Expected this to be exactly 'vos_Handlers'
  wrong = "not a function",
  fine = function(ctx, args)
    return { by = ctx.actor.name, args = args }
  end,
}

return handlers
