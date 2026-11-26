--!strict
-- Deliberately wrong uses of the `app` API. `bun run lint:luau` requires luau-lsp to report a finding on every line
-- that ends in an expect-error marker (the marker names the text the message must contain; a line with two findings
-- carries two markers) and none elsewhere. If the generated declarations stop loading or stop constraining `app`,
-- these lines check clean and the gate fails.
local app = require("vos:app")
local RPC = require("vos:rpc")
local json = require("lib:json")

app.notify { title = 123 } -- expect-error: Expected this to be exactly 'vos_app_Notification'
app.db:exec("DELETE FROM notes", "not a list") -- expect-error: Expected this to be '{ [string]: any }?', but got 'string'
app.nope() -- expect-error: Key 'nope' not found in table 'vos_app'
local _rows: number = app.db:all("SELECT 1") -- expect-error: Expected this to be 'number'
RPC.on_request("x", "not a function") -- expect-error: Expected this to be '(any, vos_app_Context) -> any', but got 'string'
app.data.query("SELECT 1") -- expect-error: Key 'data' not found in table 'vos_app'
-- what a host that drew apps' windows itself offered: an app declares nothing in Luau and draws its own page
app.manifest { id = "x", name = "X", icon = "gear" } -- expect-error: Key 'manifest' not found in table 'vos_app'
app.window("main", { render = function() end }) -- expect-error: Key 'window' not found in table 'vos_app'
local _state = app.state { count = 1 } -- expect-error: Key 'state' not found in table 'vos_app'
local _open = app:windows("main") -- expect-error: Key 'windows' not found in table 'vos_app'
local _live = app.db:query("select 1 as id", { key = "id" }) -- expect-error: Key 'query' not found in external type 'vos_app_Db'
app.open_window(5) -- expect-error: Expected this to be '{ [string]: any }?', but got 'number'
app:notify { title = "a method call passes app first" } -- expect-error: not compatible with type 'vos_app_Notification' -- expect-error: Function 'app.notify' expects 1 argument, but 2 are specified
RPC.send_notification(5) -- expect-error: Expected this to be 'string', but got 'number'
RPC.on_request("y", function()
  return 1
end, { read_only = "yes" }) -- expect-error: Expected this to be 'vos_rpc_About?'
local _name: string = json.nope -- expect-error: Key 'nope' not found in table 'lib_json'
app.files.documents:write("a.txt") -- expect-error: expects 3 arguments, but only 2 are specified
app.files.documents:remove("a", { recursive = "yes" }) -- expect-error: Expected this to be 'vos_app_RemoveOptions?' but got
app.files.photos:read("a.jpg") -- expect-error: Key 'photos' not found in external type 'vos_app_Files'
app.files.cache:create_dir(5) -- expect-error: Expected this to be 'string', but got 'number'
app.timer.every(60, function() end) -- expect-error: Key 'timer' not found in table 'vos_app'
app.shell.deepLink("note/:id", function() end) -- expect-error: Key 'shell' not found in table 'vos_app'
app.on("event", function() end) -- expect-error: Key 'on' not found in table 'vos_app'
local _kind: string = app.files.errorKind(nil) -- expect-error: Key 'errorKind' not found in external type 'vos_app_Files'
app.tools.register { name = "x", run = function() end } -- expect-error: Key 'register' not found in table 'vos_app_toolsStatics'
app.notify { title = "fine", body = "this line is correct" }
app.open_window()
RPC.send_notification("fine", { changed = true })
RPC.on_request("fine", function(_, ctx)
  return ctx:fail("never returns")
end, { description = "a tool", input_schema = { type = "object" }, read_only = true })

-- what a handler says of itself is `about`: a title is text
RPC.on_request("z", function()
  return 1
end, { title = 5 }) -- expect-error: Expected this to be 'vos_rpc_About?'

-- a handler of the app's page: a private method, `fn(params, ctx)`
RPC.on_request("_fine", function(args, ctx)
  return { by = ctx.actor.name, args = args }
end)
