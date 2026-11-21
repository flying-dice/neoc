-- Deliberately wrong uses of the generated `ui.*` constructors and the `app` API. `bun run lint:lua` requires
-- lua-language-server to report a diagnostic on every line marked `expect-error`, and none elsewhere. If the
-- generated definitions stop parsing or stop constraining these calls, they check clean and the gate fails.
local app = require("vos:app")
local ui = require("vos:ui")

local _noKey = ui.Button { title = "Go" } -- expect-error
local _badEnum = ui.Text { "x", style = "huge" } -- expect-error
local _badPositional = ui.Text { 5 } -- expect-error
local _literalBinding = ui.TextEditor { key = "b", text = "literal", label = "Note" } -- expect-error
local _badHandler = ui.Button { key = "k", title = "x", onTap = 5 } -- expect-error
local _badSpacing = ui.Stack { spacing = "big" } -- expect-error
local _badSlot = ui.Screen { toolbar = { trailing = 5 } } -- expect-error
local _noSuchComponent = ui.Marquee {} -- expect-error
local _badAction = ui.act.set(5, 1) -- expect-error
local _badWindow = app.window("main", 5) -- expect-error
local _badTool = app.tool { name = "t", run = "no" } -- expect-error
local _badManifest = app.manifest { id = "x" } -- expect-error
local _fine = ui.Button { key = "k", title = "this line is correct" }
