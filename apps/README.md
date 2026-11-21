# Writing a vos app

An app is server-side code in **typed Luau** (`--!strict`): an `app.json` and `*.luau` files. The host runs it in its own
VM and gives it state, data, tools, notifications and a description of its UI. It ships no client code: the managed
client renders the view trees the app produces and sends events back.

Read the two reference apps next to this page: `apps/calculator` (a keypad window, shared state, three agent tools) and
`apps/agents` (a SQLite-backed chat list around `ChatTranscript`). The API is typed in `types/generated/vos.d.luau`
(generated from the Rust schema by `bun run codegen`; never edit it): every `app.*` call, every `ui.*` constructor with
its props, and every `std:`/`lib:`/`vnd:` module an app may `require`. It is the reference; hover docs show in an editor
running luau-lsp with `luau-lsp.types.definitionFiles = { "@vos": "types/generated/vos.d.luau" }` and
`luau-lsp.platform.type = "standard"`.

## The package

```
apps/<id>/
  app.json             the package manifest
  app.luau             the entry (`main`): app.manifest, windows, tools, data
  *.luau               modules the entry loads with require("./calc")
  JOURNEYS.md          4 to 8 key user journeys (goal, start state, observable "Done when"; never steps)
  e2e/journeys.spec.ts their automated twins, one test per journey id
  tests/, negative/, e2e/   never shipped
```

`app.json`: `id` (3 to 40 characters: lowercase letters, digits, dashes; namespaces the app's data and tools), `name`,
`version` (`major.minor.patch`; bump it to ship a change), `main`, `icon`, `tint`, `description`, `requires.host`, and
optional `migrations`. `app.json` and the `app.manifest { ... }` call in the entry must agree on `id`, `icon` and `tint`
(`bun run build:apps` fails when they differ); `app.manifest` also carries the window: `w`, `h` and the **minimum**
`minW`/`minH`.

`bun run build:apps` bundles each app with darklua into one `app.luau` (relative `require`s inlined, types and comments
stripped, statements kept on their source lines) and writes an **app ref** (`app-ref.json` with a sha256 per file) under
`dist/apps/<id>/<version>/` plus `dist/apps/registry.json`. The host installs an app from an app ref URL (`file://`,
`http://`, `https://`), verifies every sha256 and runs the migrations. `bun run dev` rebuilds on save and the host reloads
the app's VM, keeping its database. `darklua` 0.18.0 must be installed; the build fails without it.

## The `app` API

```lua
--!strict
local app = require("vos:app")
local ui = require("vos:ui")
local calc = require("./calc")   -- a module of this package, inlined at build time
```

| Call | What it does |
|---|---|
| `app.manifest { id, name, icon, tint, window = { w, h, minW, minH } }` | identity and window sizes; call it first |
| `app.window(name, { title, size, client, state, render, onOpen, onClose })` | declare a window kind; `render(w)` returns the view tree |
| `app.state(initial)` | shared in-memory state; reads in `render` are tracked, assignments re-render the windows that read them; lost on restart |
| `app.kv`, `app.db` | small durable key/value store; SQLite per app (`migrate`, `exec`, `all`, `one`, `tx`, live sources `query`/`row`/`scalar`) |
| `app.files.documents`, `.cache`, `.temp` | the app's managed files: `read`, `write`, `list`, `info`, `createDir`, `remove`, `move`, `copy` (see "Files") |
| `app.tool { name, title, description, readOnly, input, run }` | an agent tool (MCP name `<id>__<name>`, local name `[a-z][a-z0-9_]*`) |
| `app.notify { title, body }`, `w:toast { ... }` | a notification (see "Notifications" below), an in-window message |
| `app:openWindow(name, params)`, `app.thread()`, `app.agents:list()` | bring a window forward, a chat source for `ChatTranscript`, the roster (`{ id, name, avatar, role }` only) |

Constructors are table calls, StyLua-formatted: positional entries are children (or the text), named keys are props, slots
and handlers: `ui.Button { key = "key-7", title = "7", onTap = function() press("7") end }`. Apps never style: props take
semantic tokens only.

- **`render(w)` is pure.** A db write, `app:*` call or state assignment inside it raises. The tree is data; handlers stay
  closures in the VM and only a key path crosses the wire.
- **Every interactive node needs a `key`** (the host rejects it otherwise); it becomes the test id
  `<appId>-<keys on the path>` (`calculator-key-7`). Icon-only controls need a `label`; an `Image` needs `alt` or
  `decorative = true`. Rows use stable keys, never indices or display text.
- **Where state lives:** ephemeral UI (focus, scroll, caret) in the client; `client = { name = { init, persist } }` for
  selection and search text; `app.state`/`w.state` for what the app owns; `app.db`/`app.kv` for anything durable. Two-way
  controls take a Binding (`ui.TextField { text = w.client.search }`).
- **Data:** `db:migrate` steps are numbered and run once each (a changed old step is an error); sources return rows in
  windows and live deltas, so an agent's write through a tool updates every open window with no UI code. SQL `ATTACH` is
  denied through `app.db`.
- **Tools:** describe every input field with `app.t` builders (agents read them); the host validates input before `run`.
  `readOnly = true` is enforced for the `app.*` API (no db, kv, files, state, notify, shell or window writes, no registering
  anything; tasks and requests the call starts inherit it) and lets an agent with the default `auto_read_only` policy call
  the tool without a prompt. It does not cover the `std:`/`vnd:` modules. A tool that writes is not `readOnly`.
- **Notifications:** `app.notify { title = "Backup done", body = "12 files" }` tells the person something now. It takes a
  `title` (1 to 200 characters, not blank) and an optional `body` (up to 2000) and no other key; the host sends the app's id
  with it, so the sender is never text the app wrote. It goes to the clients connected at that moment: one that is not connected never
  gets it (nothing is queued). Bad input raises, each message starting `app.notify`: a key other than `title` and `body`
  (`unknown key`), no title (`needs`), a title that is blank or too long, a body that is too long, a value that is not a
  string, a 65th notification in one call (`at most 64 notifications per call`), and a call in `render` or a `readOnly`
  tool.
- **Background work:** start loops with `task.spawn` from `std:task` and let the entry return (the app is not loaded until
  it does). Closing a window leaves the work running; assigning `app.state` from a task re-renders open windows; unloading
  aborts tasks and closes servers (`crates/vos-host/tests/fixtures/ticker` is an example).
- **Luau limits:** a host call (`app.db:*`, `app.kv:*`) cannot wait inside a `string.gsub` callback, a `table.sort`
  comparator, a metamethod other than `__call`, or an `xpcall` handler. No `print`, `os`, `debug`, `load*`; `bit` is
  `bit32`; `string.format("%s", v)` takes strings and numbers only. The root README lists the language differences.
- **Modules:** `lib:test`, `lib:compile` and `std:workers` are not offered (`vos_domain::NOT_FOR_APPS`); naming one is a
  type error. Everything else in `neoc-vm` is, with no permission to ask for. An app is the person's own code on their
  own host: `CLAUDE.md` ("Boundaries") says what the VM does and does not keep apart.
- **Your own modules are `any` in the editor.** luau-lsp resolves a relative `require` only while `require` is its
  built-in; `bun run lint:luau` checks them in a second pass. A wrong call into one is an error there, not in the editor.

## Files

`app.files` gives the app three managed locations, found from its identity: `documents` (what the person would miss),
`cache` (what the app can rebuild) and `temp` (scratch). They are one shared set per app, not per person or device, and
live next to the app's database (`<data>/apps/<id>.files/<location>/`). The app never names a host path:

```lua
local docs = app.files.documents
assert(docs:createDir("notes"))                      -- and the missing directories above it
local ok, err = docs:write("notes/a.txt", "hello")   -- replaces the file in one step; the parent must exist
local text = docs:read("notes/a.txt")                -- the whole file, binary-safe, at most 64 MiB
for _, e in docs:list("notes") or {} do end          -- { name, kind, size, modified }, sorted by name
docs:move("notes/a.txt", "notes/b.txt")              -- also :copy (a file, or a tree to a new path), :info, :remove
docs:remove("notes", { recursive = true })
if app.files.errorKind(err) == "not_found" then end  -- the kinds of std:fs
```

- **Paths** are relative to the location and written with `/`; `""` is the location itself (it can be listed, not
  written, moved or removed). An absolute path, `..`, `:`, a backslash, NUL, or a name over 255 bytes is
  `invalid_input`, and a symbolic link inside a location is never followed (`info` and `list` show it as
  `"symlink"`, `remove` deletes the link, everything else refuses it).
- **Errors** are values: `nil, "files.<method>: <kind>: <path>: <message>"`, without the path when the path itself
  was refused (`invalid_input`) or the host keeps no files (`unsupported`). Never a host path. A wrong argument type, an unknown option
  and a write inside a `readOnly` tool or a render raise.
- **Lifetime:** the files stay when the app is updated, disabled or uninstalled without purging, so a reinstall finds
  them; uninstall with purge and "delete data" remove them with the database.
- A call holds the app's turn while it runs, like `app.db`: a large tree copy or remove, or a big write (writes have
  no size cap), keeps the app's other calls (its windows included) waiting until it ends. It does not need `std:fs`.
- Scoping is a convention that keeps apps out of each other's files by accident, **not a sandbox**: `std:fs` reaches
  any file the host can. `crates/vos-lua/src/files.rs` says how links and races are handled.

## Adding an app

1. **Package.** Copy `apps/calculator` to `apps/<id>`; edit `app.json` (version `0.1.0`) and `app.luau` (it starts with
   `--!strict`, the two `require`s and `app.manifest { ... }`).
2. **Window.** `window = { w, h, minW, minH }`: the minimum is the smallest size where every control is fully visible and at
   least 44 pt (add a comment saying why when it is more than 320x240). The host clamps the window to it.
3. **Logic** as typed functions in their own module (the Calculator's `calc.luau`), so it is testable without a window;
   the view as `render`; the tools with a described schema and a truthful `readOnly`.
4. **Clean.** `bun run lint:luau` (luau-lsp strict against the declarations) and `bun run lint:stylua` pass, not skipped;
   `bun run format` fixes formatting. Fix what the checker reports; do not silence it.
5. **Rust test** of the logic and tools next to `crates/vos-lua/tests/calculator_app.rs` (it loads the shipped package,
   calls tools with `call_tool`, asserts results and `readOnly`); a host test in `crates/vos-host/tests` for what needs the
   data layer, views or agents.
6. **`JOURNEYS.md` and `e2e/journeys.spec.ts`** with a test of the same id per journey, driven by test ids on the real host
   (include an error case, a minimum-window case, a keyboard-only case and, where it applies, an agent driving the app).
   An app that writes shared state runs its journeys serially and clears that state first.
7. **States.** The generic `desktop` project opens the app in the real shell (floating light and dark, minimum and large
   windows, every layout, axe, snapshots). For a state no journey reaches, add a JSON-tree story to the client Storybook
   (`client/src/view/stories`).
8. **Try it for real** (`verify-change`), then **design it** with `app-design` until its exit criteria hold, and ship it
   through a PR whose review applies the `pr-review` section "vos checks". Until then the app stays in its PR. To change a
   released app, bump `version`.

Missing a capability? Extend the host API (domain type and schema, application port and use case, the Lua adapter, codegen,
tests), never a side channel, and call the platform change out in the PR.
