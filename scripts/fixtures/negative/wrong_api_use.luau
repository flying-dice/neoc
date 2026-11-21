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
app.manifest { id = "x", name = "X" } -- expect-error: missing fields 'icon', and 'window'
app.data.query("SELECT 1") -- expect-error: Key 'data' not found in table 'vos_AppApi'
app.window("main", 5) -- expect-error: Expected this to be 'vos_WindowSpec', but got 'number'
local state = app.state { count = 1 }
state.count = "text" -- expect-error: Expected this to be 'number', but got 'string'
app.t.string { description = 5 } -- expect-error: Expected this to be 'vos_SchemaOptions?'
local _name: string = json.nope -- expect-error: Key 'nope' not found in table 'lib_json'
app.notify { title = "fine", body = "this line is correct" }
app.tool {
  name = "fine",
  input = { id = app.t.integer { min = 1 } },
  run = function(_, ctx)
    return ctx:fail("never returns")
  end,
}
