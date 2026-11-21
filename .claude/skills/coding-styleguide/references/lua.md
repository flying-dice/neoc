# Lua apps in AgentOS (`apps/<id>/*.lua`)

An app is server-side **plain Lua 5.1** in its own VM (no permissions, no budgets): no Luau syntax (no `+=`, `continue`, backtick strings, `if`
expressions, `for k, v in t do`, type annotations in signatures). Its contract is the `app` table and the `ui` constructors,
described by the LuaCATS definitions in `types/generated/luals/` (generated: never edit them; `ui.lua` next to them is the
runtime module). `apps/calculator` and `apps/agents` are the models; the API is described in `apps/README.md`.

Gates: `bun run lint:lua` (`lua-language-server --check` against the generated definitions, plus the `negative/` fixtures),
`bun run lint:stylua`, `bun run format`.

## File shape

```lua
-- One line on what the app is and where its state lives.
local app = require("vos:app")
local ui = require("vos:ui")
local calc = require("./calc")      -- a module of this package (bundle-only require)
local t = app.t

app.manifest { id = "calculator", name = "Calculator", icon = "plus", tint = "orange",
  window = { w = 340, h = 560, minW = 320, minH = 500 } }   -- say why the minimum is above 320x240
```

- Annotations are optional LuaCATS comments (`---@param`, `---@return`, `---@type`, `---@cast`); write one where it helps a
  reader or the checker (a `tonumber(...)` the code knows is a number: `---@cast rounded number`). Fix what
  lua-language-server reports, don't silence it: a `---@cast` needs an invariant established just above it, or a one-line
  comment saying why the checker can't see it. Don't write annotations that only repeat the obvious.
- `app.json` and `app.manifest` agree on `id`, `icon` and `tint` (the build checks).
- `require` takes `vos:app`, `vos:ui`, the package's own modules (bundled at build time) and the host's `std:`,
  `lib:` and `vnd:` modules; there is no bare `io`, `os` or `debug` and no filesystem `require`: files and the network
  come through `std:fs`, `std:net`, `lib:http` and the rest. Don't reach for globals; the per-app VM leaves out the
  ones that load code or reach the engine.

## Structure

- **Logic is plain Lua functions with no UI in them** (`calc.lua`: `evaluate`, `format`), so it is testable from Rust and
  readable on its own. State that must be shared by every window is one `app.state { ... }` table; anything that must
  survive a restart goes to `app.kv` or `app.db`.
- **`render` is pure.** It reads state and returns a tree. No `app.db:exec`, no `app:openWindow`, no state assignment in it
  (the host raises). Do the work in handlers (`onTap`, `onSend`) and tools.
- **Keys:** every interactive node has an explicit `key`; test ids derive from the key path (`calculator-key-7`), so keys are
  stable ids (row ids, not indices) and never display text.
- **Handlers** are small closures that call a named function (`onTap = function() press(k.kind, k.arg) end`). Wrap work that
  can fail in `pcall` only where the failure is part of the feature (the Calculator turns a divide by zero into "Error").
- **Tools** (`app.tool`): a local name `[a-z][a-z0-9_]*`, a `title`, a `description` an agent can act on, an `input` of
  `app.t` builders with `description`, bounds and `default`s, `readOnly = true` whenever it writes nothing (it is
  enforced, and it lets agents run it without a prompt), and a `run` that returns a table or calls `ctx:fail(message)`.
  Say why a tool is not `readOnly` in a comment.
- **SQL:** named `:params`, never string concatenation of values; numbered `db:migrate` steps, never edited after release
  (add a new number); a windowed `db:query` ends its `order by` with the key.
- **No permissions:** `app.json` and `app.manifest` carry none; an app may use `app.*` and every `std:`/`lib:`/`vnd:` module.
  What it starts (tasks, servers, workers) belongs to it and is released on unload.

## The view style

Table-call constructors, positional entries as children, named keys as props, slots and handlers:

```lua
ui.Button { key = "new-chat", icon = "square.and.pencil", label = "New chat", shortcut = "cmd+n", onTap = function() ... end }
ui.Grid { columns = 4, spacing = 2, ui.each(KEYS, keyView) }
```

- Props are semantic tokens (`style = "display"`, `fill = "gray4"`, `foreground = "secondary"`, `spacing = 2`): apps never use
  raw colours, sizes or CSS. Check the component's props in `docs/reference/components.md`.
- Icon-only controls need a `label`; an `Image` needs `alt` or `decorative = true`; the accessible name keeps the visible
  text (WCAG 2.5.3).
- `and`, `or`, `not` are Lua keywords: in a tree use `ui.all`, `ui.any`, `ui.negate`, and `ui.when(cond, a, b)` for a value.
  A Ref has no value on the host: `tostring(ref)` and `"x" .. ref` raise, use `ui.concat`. Pass a list of nodes with
  `unpack(items)` or `ui.each`. Write `cond and a or b` only when `a` can't be `false` or `nil`.
- A Binding prop (`TextField.text`, `List.selection`) takes a Ref (`w.client.x`, a source field), never a plain value; the
  checker flags the plain value.
- Declare client state in the window (`client = { threadId = { init = "", persist = "app" } }`) for selection and similar UI
  state, rather than keeping it in `app.state`.

## Formatting (StyLua)

`stylua.toml`: `syntax = "Lua51"`, 120 columns, 2-space indent, Unix line endings, `quote_style = "AutoPreferDouble"`,
`call_parentheses = "NoSingleTable"` (so `ui.VStack { ... }` stays a table call), no collapsed simple statements. Run
`bun run format`; don't hand-format against it. Build text with `..`; an
`if` statement beats a nested `and`/`or` chain.

## Comments

Say why, not what: the reason for a minimum window size, a cast, an invariant, a host rule that surprised you
(`-- below 500 tall the keypad no longer fits`). A section divider line (`-- ── View ──`) is fine. No commented-out code.

## Negative fixtures

To prove a type rule holds, add a file under a `negative/` folder (see `scripts/fixtures/negative/`) with
`-- expect-error` on each line that must be flagged; `lint:lua` demands exactly those lines are flagged. (lua-language-server
checks the types of values and the shape of tables, but not an unknown key in a table literal.)
