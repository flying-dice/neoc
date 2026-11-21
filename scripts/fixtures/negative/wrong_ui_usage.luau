--!strict
-- Deliberately wrong uses of the generated `ui.*` constructors. `bun run lint:luau` requires luau-lsp to report a
-- finding on every line that ends in an expect-error marker and none elsewhere. If the generated declarations stop
-- loading or stop constraining these calls, they check clean and the gate fails.
local app = require("vos:app")
local ui = require("vos:ui")

local function nothing(): vos_Node?
  return nil
end
local function five(): number
  return 5
end

local _noKey = ui.Button { title = "Go" } -- expect-error: missing field 'key'
local _badEnum = ui.Text { "x", style = "huge" } -- expect-error: style: "huge"
local _badPositional = ui.Text { 5 } -- expect-error: Expected this to be 'string | vos_Fn | vos_Ref<string>', but got 'number'
local _literalBinding = ui.TextEditor { key = "b", text = "literal", label = "Note" } -- expect-error: Expected this to be exactly 'vos_TextEditorProps'
local _badHandler = ui.Button { key = "k", title = "x", onTap = 5 } -- expect-error: onTap: number
local _badSpacing = ui.Stack { spacing = "big" } -- expect-error: spacing: string
local _badSlot = ui.Screen { toolbar = { trailing = 5 } } -- expect-error: toolbar: {| trailing: number |}
local _noSuchComponent = ui.Marquee {} -- expect-error: Key 'Marquee' not found in table 'vos_ui'
local _badAction = ui.act.set(5, 1) -- expect-error: Expected this to be 'string | vos_Ref<any>', but got 'number'
local _badEach = ui.each(5, nothing) -- expect-error: but got 'number'
local _badRow = ui.Row { tag = 1, title = 5 } -- expect-error: title: number
local _badFrame = ui.VStack { frame = { minW = "wide" } } -- expect-error: frame: {| minW: string |}
local _badKey = ui.Button { key = 5, title = "x" } -- expect-error: key: number
app.window("main", { render = five }) -- expect-error: Expected this to be exactly 'vos_WindowSpec'
local _fine = ui.Button { key = "k", title = "this line is correct" }
