--!strict
-- A deliberately wrong app. `bun run lint:luau` requires luau-lsp to report a type error on every line
-- marked `expect-error`, and none elsewhere. If the definitions file stops parsing or stops constraining
-- `app`, these lines type-check clean and the gate fails.
app.notify { title = 123 } -- expect-error
app.data.exec("DELETE FROM notes", "not a list") -- expect-error
app.nope() -- expect-error
local _rows: number = app.data.query("SELECT 1") -- expect-error
app.notify { title = "fine", body = "this line is correct" }
