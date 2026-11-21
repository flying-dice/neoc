# Luau apps in vos (`apps/<id>/*.luau`)

An app is server-side **typed Luau** (`--!strict`) in its own VM (no permissions, no budgets). Its contract is the `app` table
and the `ui` constructors, described by the Luau declarations in `types/generated/vos.d.luau` (generated: never edit it;
`ui.lua` next to it is the runtime module). `apps/calculator` and `apps/agents` are the models; the API is described in
`apps/README.md`.

Gates: `bun run lint:luau` (`luau-lsp analyze`, strict, against the generated declarations, plus the usage fixture and the
`negative/` fixtures), `bun run lint:stylua` (syntax `Luau`), `bun run format`.

## File shape

```lua
--!strict
-- One line on what the app is and where its state lives.
local app = require("vos:app")
local ui = require("vos:ui")
local calc = require("./calc")      -- a module of this package (bundle-only require; `any` in the editor, typed by `lint:luau`'s second pass)
local t = app.t

type Node = vos_Node               -- the declarations name their types vos_<Name>
type Window = vos_WindowContext

app.manifest { id = "calculator", name = "Calculator", icon = "plus", tint = "orange",
  window = { w = 340, h = 560, minW = 320, minH = 500 } }   -- say why the minimum is above 320x240
```

- **Every file starts with `--!strict`** (the lint checks it). Annotate parameters and results of functions, and the
  tables whose shape matters (`type KeyDef = { ... }`); let locals be inferred. Fix what luau-lsp reports, don't silence
  it: a `::` cast or an `assert(x ~= nil)` needs an invariant established just above it, or a one-line comment saying
  why the checker can't see it (a `tonumber(...)` the code knows is a number: `tonumber(text) :: number`). A `pcall` of a
  function that returns nothing is typed `(boolean)` only: cast the function (`f :: (string) -> any`) to get the error
  as its second result. Don't write annotations that only repeat the obvious.
- **Luau is the language**: `+=`, `continue`, `if` expressions, string interpolation, generalized iteration (`for _, v in t do`),
  generics and type aliases are all fine, as are Luau's library members (`string.split`, `table.find`, `math.clamp`,
  `table.clone`). Use what makes the code shorter and clearer, not for its own sake. The app environment lacks `print`,
  `os`, `debug`, `buffer`, `load*`, `getfenv`/`setfenv`, `collectgarbage` (the declarations make each a type error); `typeof`
  and `utf8` come from `lua/prelude.lua`.
- `app.json` and `app.manifest` agree on `id`, `icon` and `tint` (the build checks).
- `require` takes `vos:app`, `vos:ui`, the package's own modules (bundled at build time) and the host's `std:`,
  `lib:` and `vnd:` modules (all typed, except the refused `lib:test`, `lib:compile` and `std:workers`, which are type errors, and
  never a misspelt name: `lint:luau` fails it); there is no bare `io`, `os` or `debug` and no filesystem `require`: files and the network
  come through `std:fs`, `std:net`, `lib:http` and the rest. Don't reach for globals; the per-app VM leaves out the
  ones that load code or reach the engine.

## Structure

- **Logic is plain functions with no UI in them** (`calc.luau`: `evaluate`, `format`), so it is testable from Rust and
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
  A Ref has no value on the host: `tostring(ref)` and `"x" .. ref` (or an interpolated string) raise, use `ui.concat`. Pass a
  list of nodes with `unpack(items)` or `ui.each`. Write `cond and a or b` only when `a` can't be `false` or `nil`
  (`if cond then a else b` is Luau's expression form for plain values).
- A Binding prop (`TextField.text`, `List.selection`) takes a Ref (`w.client.x`, a source field), never a plain value; the
  checker flags the plain value.
- Declare client state in the window (`client = { threadId = { init = "", persist = "app" } }`) for selection and similar UI
  state, rather than keeping it in `app.state`.

## Formatting (StyLua)

`stylua.toml`: `syntax = "Luau"`, 120 columns, 2-space indent, Unix line endings, `quote_style = "AutoPreferDouble"`,
`call_parentheses = "NoSingleTable"` (so `ui.VStack { ... }` stays a table call), no collapsed simple statements. Run
`bun run format`; don't hand-format against it. Build text with `..`; an
`if` statement beats a nested `and`/`or` chain.

## Comments

Say why, not what: the reason for a minimum window size, a cast, an invariant, a host rule that surprised you
(`-- below 500 tall the keypad no longer fits`). A section divider line (`-- ── View ──`) is fine. No commented-out code.

## Negative fixtures

To prove a type rule holds, add a file under a `negative/` folder (see `scripts/fixtures/negative/`) with
`-- expect-error: <text from the message>` at the end of each line that must be flagged (one marker per finding);
`lint:luau` demands exactly those findings, and `NEGATIVE_MARKERS` in `scripts/lint-luau.ts` pins the count per file: a
deleted line is an edit of that table. (luau-lsp checks the types of values and the shape of tables, but not an unknown
key in a table literal.)
