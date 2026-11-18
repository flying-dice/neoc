--!strict
-- A deliberately wrong app. `bun run lint:luau` requires luau-lsp to report a type error on every line
-- marked `expect-error`, and none elsewhere. If the definitions file stops parsing or stops constraining
-- `app`, these lines type-check clean and the gate fails.
local app = require("@agentos/app")
app.notify { title = 123 } -- expect-error
app.db:exec("DELETE FROM notes", "not a list") -- expect-error
app.nope() -- expect-error
local _rows: number = app.db:all("SELECT 1") -- expect-error
app.tool { name = "x", run = "not a function" } -- expect-error
app.manifest { id = "x", name = "X" } -- expect-error
app.data.query("SELECT 1") -- expect-error
app.notify { title = "fine", body = "this line is correct" }
app.tool {
  name = "fine",
  input = { id = app.t.integer { min = 1 } },
  run = function(input, ctx)
    return ctx:fail("never returns")
  end,
}
