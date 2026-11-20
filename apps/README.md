# Building AgentOS apps

An AgentOS app is a **server-side package, written in plain Lua 5.1**, that runs inside the Rust host. It holds its state, data, tools,
notifications and a description of its UI. It ships no client code: the managed client renders the view trees the app
produces and sends events back.

```
 Browser: managed client                         Rust host (crates/agentos-host)
 ┌──────────────────────────────┐  JSON-RPC/WS   ┌─────────────────────────────────────────┐
 │ shell: windows, Dock         │◀──── /rpc ────▶│ one Lua VM per app (isolated)          │
 │ renderer: view trees → kit   │                │   app.window → view tree → client       │
 │ client state, drafts         │  AG-UI / SSE   │   app.db / app.kv  (per-app SQLite)     │
 │ no app code, ever            │◀──── /agui ────│   app.tool ──▶ /mcp ◀── agents          │
 └──────────────────────────────┘                │ app refs ◀── file:// http(s):// ────────│
                                                 └─────────────────────────────────────────┘
```

The reference apps are `apps/calculator` (a keypad window, shared state, three agent tools; [its walkthrough](calculator/README.md) covers files, state, view, tools, running and testing) and `apps/agents` (a
SQLite-backed chat list around the `ChatTranscript` component). Read them next to this page. The architecture is in
`docs/architecture/` (`host-stack.md` for the host, `ui-system.md` for the UI protocol and the Lua API); the generated
LuaCATS definitions of the API are in `types/generated/luals/`; the components are listed in `docs/reference/components.md`. The last two are
generated from the Rust schema (`bun run codegen`): never edit them.

## The package

```
apps/<id>/
  app.json         the package manifest (below)
  app.lua          the entry (`main` in app.json): declares the manifest, windows, tools, data
  *.lua            modules the entry loads with require("./calc") (calc.lua in the Calculator)
  JOURNEYS.md      4 to 8 key user journeys
  e2e/journeys.spec.ts   their automated twins
  tests/, negative/, e2e/   never shipped: the build leaves these folders out of the package
```

### `app.json`

```json
{
  "id": "calculator",
  "name": "Calculator",
  "version": "0.1.0",
  "description": "Arithmetic for you and your agents: type or tap a sum, or let an agent put a result on it.",
  "main": "app.lua",
  "icon": "plus",
  "tint": "orange",
  "requires": { "host": ">=1.0" }
}
```

| Field | Rule |
|---|---|
| `id` | 3 to 40 characters: lowercase letters, digits, dashes, not starting with a dash. Namespaces the app's data and its tools (`<id>__<tool>`). |
| `name`, `version` | `version` is `major.minor.patch` digits (`0.1.0`); bump it to ship a change. |
| `main` | the Lua entry file (default `app.lua`). |
| `icon`, `tint`, `description` | what Discover and the add-by-URL preview show *before* the app runs. `icon` is a symbol name, `tint` a palette role. |
| `requires.host` | the host API range the app needs (checked against the host at install). |
| `migrations` | optional list of `{ version, name, file }` SQL files; most apps call `db:migrate` in Lua instead. |

`app.json` and the `app.manifest { ... }` call in the entry must agree on `id`, `icon` and `tint`: `bun run build:apps`
fails when they differ. `app.manifest` additionally carries the window (`w`, `h`, `minW`, `minH`). Apps have no
permissions: a `permissions` key in `app.json` or in `app.manifest` (from before they were removed) is ignored.

### App refs, registries and install

`bun run build:apps` (`scripts/build-apps.ts`) turns every `apps/*/app.json` into an **app ref** under
`dist/apps/<id>/<version>/`: `app-ref.json` (`{ manifest, files: [{ path, url, sha256 }] }`, file URLs relative to it)
plus the app as **one bundled Lua 5.1 file** (`app.lua`) and its migrations, and one `dist/apps/registry.json`
(`{ "apps": ["calculator/0.1.0/app-ref.json", ...] }`). Apps are plain Lua 5.1 and run as written:
[darklua](https://github.com/seaofvoices/darklua) (`.darklua.json`) only bundles: it inlines the package's relative
`require`s into the one file and strips comments; `require("@agentos/app")` and `require("@agentos/ui")` stay runtime
calls. The ref's `manifest.main` is the bundled file; the separate source modules are not shipped. Two things to know
when writing an app: write Lua 5.1 (`unpack`, `string.gsub`, `x = x + 1`, `a and b or c`: no `+=`, `continue`,
backtick strings, `if` expressions or `for k, v in t do`), and use only the library members Lua 5.1 and
`lua/prelude.lua` provide (the prelude adds `typeof`, `table.clone`, `table.move`, `table.pack`, `table.unpack` and
`utf8.*`); `bun test scripts` fails on Luau syntax and on `table.find`, `math.clamp`, `bit32`, `buffer` and the like.
`darklua` 0.18.0 must be installed (see AGENTS.md). The bundle keeps each statement on its source line
(`retain_lines`), so an error at `app.lua:N` is line N of `app.lua` for an app with no relative `require`; an app with
modules is shifted by the size of the modules bundled before it (the Calculator: `calc.lua` +2, `app.lua` +183).

- **A registry** is a list of app-ref URLs. The host's default registry is `dist/apps/registry.json` over `file://`
  when it exists; `registries` in `agentos.toml` (or `AGENTOS_REGISTRIES`) adds more. Discover lists what they offer.
- **An app is installed from an app ref URL** (`file://`, `http://`, `https://`): the host fetches it, verifies every
  file's sha256, runs the migrations, loads the app in its VM and publishes its tools. Discover previews an app ref
  first (manifest, source host) without running anything, then asks for confirmation. `install` in
  `agentos.toml` makes sure listed app refs are installed at every start.
- **Lifecycle:** install, enable, disable (VM unloaded, tools withdrawn, data kept), delete data, uninstall. At start
  the host re-fetches and re-verifies every installed app. An app that fails to load is marked failed and the rest
  start. All of it is in `docs/architecture/host-stack.md` ("Apps: lifecycle").
- **Dev loop:** `bun run dev` builds the apps, starts the host with `dev_reload` and Vite, and **watches `apps/**`**
  (`*.lua`, `app.json`, migrations): save a file and it re-runs `build:apps`; the host (which cannot run darklua)
  watches the built refs under `dist/apps`, rebuilds that app's VM and keeps its database (the old version keeps
  running if the new one does not load).

## The `app` API

An app file starts like this (the Calculator):

```lua
local app = require("@agentos/app")   -- the host module; also a global named `app`
local ui = require("@agentos/ui")     -- component constructors and helpers, generated from the component schema
local calc = require("./calc")        -- a module of this package: inlined into the bundle at build time
```

In the source, `require` accepts `./x`, `../x` (bundled into the one built file when the app is built), `@agentos/app`
and `@agentos/ui`. At run time the built file can `require` `@agentos/app`, `@agentos/ui` and the host's `std:`, `lib:` and `vnd:` modules
(the build leaves those as `require` calls, see "Every library module is yours" below), and never the filesystem
(`require("./x")` in a running app is an error that says so). The
LuaCATS definitions in `types/generated/luals/` (generated, `---@meta`) are the contract: lua-language-server reads
them through `.luarc.json` (so an editor with the Lua extension knows `app`, `ui.*` and `require("@agentos/ui")`), and
`bun run lint:lua` checks every app against them. Annotations in app code (`---@param`, `---@type`) are optional;
write one where it helps a reader or silences a false alarm. The `std:`, `lib:` and `vnd:` modules are described too:
`types/luals/<namespace>/<name>.lua` (`lib/json.lua`, `std/fs.lua`, ...) is the definition file of `require("<namespace>:<name>")`
(found through `types/luals-plugin.lua`),
generated by `bun run codegen` from each module's recorded surface (`neoc-vm`, `facade.rs`). Completion lists a
module's functions and the methods and fields of what it returns; a misspelt function, a wrong argument type or a
missing argument is flagged by `bun run lint:lua`. A fallible function returns `value, err` (`local s, err =
base64.decode(x)`); a value the module cannot type (a JSON tree, a table of options) is `any`. The files are named
without a colon (`lib/json.lua` for `lib:json`), so a checkout works on Windows too.

| Call | What it does |
|---|---|
| `app.manifest { id, name, icon, tint, window = { w, h, minW, minH } }` | identity and the window's preferred and **minimum** size. Call it first. |
| `app.window(name, { title, size, client, state, render, onOpen, onClose })` | declare a window kind. `render(w)` returns the view tree. |
| `app.state(initial)` | shared in-memory state for the whole app; reads in `render` are tracked, assignments re-render the windows that read them. Lost on restart: persist what matters with `app.kv` or `app.db`. |
| `app.kv:get(key)`, `:set(key, value)`, `:delete(key)` | small durable key/value store |
| `app.db` | `migrate`, `exec`, `all`, `one`, `tx`, and the live sources `query`, `row`, `scalar` |
| `app.tool { ... }` (alias `app.tools.register`) | declare an agent tool |
| `app.t.string / integer / number / boolean / array / object / enum` | input schema builders for tools |
| `app.notify { title, body, at, actions }` | a notification |
| `app:openWindow(name, params)`, `app:windows(name)` | bring a declared window forward, or iterate the open instances in the order they were opened (an undeclared name raises in both) |
| `app.thread()` | declare a chat-thread source for `ChatTranscript` (a literal agent must be on the roster) |
| `app.agents:list()` | the agent contacts, as `{ id, name, avatar, role }` |
| `app.shell.badge / command / deepLink` | Dock badge, menu commands, deep links (needs `shell`; declarations the shell does not act on yet) |
| `app.timer.at / every`, `app.log(message)`, `app.id` | host-scheduled timers, logging, the app's id |

Style is table-call constructors, StyLua-formatted (`stylua.toml`: `call_parentheses = "NoSingleTable"`):

```lua
ui.Button { key = "key-7", title = "7", label = "7", onTap = function() press("digit", "7") end }
```

Positional entries are children (or the text of `Text`, `Label`, `Button`, `Row`), named keys are props, slots and
handlers. `nil` and nested lists in child position are flattened; `ui.each(list, fn)` maps a list to children. Apps
never style: universal props (`padding`, `frame`, `background`, `foreground`, `style`) take semantic tokens only.

## Windows and view trees

A window kind is a view factory. Each open window is an **instance** with its own `w`:

```lua
app.window("main", {
  title = "Calculator",
  render = function(w)
    return ui.Scope { scheme = "dark", background = "primary", frame = "fill", ui.VStack { ... } }
  end,
})
```

- **`render(w)` is pure.** It runs as a tracked scope: the state it reads is that window's dependency set, so when a
  handler or a tool changes `app.state` the host re-renders exactly the windows that read it and sends the minimal
  ops. A db write, `app:*` call or state assignment inside `render` raises. One calculator, however many windows are
  open, is the point of `app.state`.
- **The tree is data.** The VM turns the constructors' tables into a wire tree and validates it (every interactive
  node needs an explicit `key`; icon-only controls need a `label`; an `Image` needs `alt` or `decorative = true`).
  Handlers stay closures in the VM, keyed by path and event; only the key path crosses the wire.
- **Handlers** are a Lua function `function(e) ... end` (`e.key`, `e.row`, `e.value`, `e.payload`, `e.actor`), a
  client action (`ui.act.set / toggle / push / pop / popToRoot / focus / scrollTo / copy / emit / seq`) that runs on the
  client with no round trip, or a list mixing both.
- **`w`**: `w.id`, `w.params`; `w.state` (per-window server state); `w.client` (Refs to client state); `w.env` (Refs:
  `sizeClass`, `heightClass`, `locale`, `timeZone`, `scheme`, `now`); one-shot commands `w:toast { text, icon, kind,
  action = { title, onTap } }`, `w:focus(key)`, `w:scrollTo(key)`, `w:announce(text)`, `w:copy(text)`, `w:close()`,
  `w:openWindow(name, params)`.
- **Test ids come from keys:** `<appId>-<explicit keys on the path>`, skipping auto keys, slot names and the window root
  (`key = "key-7"` is `calculator-key-7`; the display is `calculator-display`). A row template adds `row-<rowKey>`.
  Set a `key` on every interactive node, and use stable ids for rows, never indices or display text.

### Bindings and state

Where a value lives decides who owns it. A prop is a literal, a **Ref**, or a call of the closed expression set
(`ui.eq`, `ui.when`, `ui.concat`, `ui.isEmpty`, `ui.fmt.relative`, ...); the client evaluates it, so a keystroke or a
list refresh never rebuilds the tree.

| State | Owner | Use it for |
|---|---|---|
| Ephemeral UI (focus, scroll, caret, drafts) | client | nothing to declare |
| **Client state** `client = { threadId = { init = "", persist = "app" } }` | client (mirrored to the host when `sync`/`persist`) | selection, search text, the open item. `persist`: `none`, `session`, `app` (kept by the host across restarts). Read the Ref `w.client.threadId` in the tree; `w.client:get("threadId")` in handlers; `w.client:set(...)` to drive it. |
| App and window state (`app.state`, `w.state`) | host | what the app owns: the calculator's entry and operator |
| Data (`app.db`, `app.kv`) | host | everything durable |

Two-way controls take a Binding prop: `ui.TextField { text = w.client.search }`, `ui.List { selection = c.threadId }`,
or a row field from a source (`row:bind("title")`). The client shows an instant draft, then writes with a base revision;
the host merges text and the human wins for scalars.

## Data: `app.db`, live sources, `app.kv`

```lua
db:migrate { [1] = [[create table threads (id text primary key, title text not null)]] }  -- numbered, once each

local threads = db:query([[select id, title from threads order by updated_at desc, id desc]], { key = "id", page = 60 })
local thread  = db:row([[select id, title from threads where id = :id]], { key = "id" })
```

- **Migrations** are numbered steps, each in its own transaction, recorded with a checksum; a changed old step is an
  error, so add a new number instead.
- **`exec`, `all`, `one`, `tx`** are the imperative calls (named `:params`). `tx(body)` commits its statements together
  or not at all.
- **Sources** (`query`, `row`, `scalar`) are prepared once and return a factory. Calling the factory *in `render`*
  with params (values or Refs) gives a handle for `ui.ForEach { source = ..., row = function(r) ... end }` or a field
  Ref (`current.title`). The SQL never leaves the host; the client gets rows in windows and row deltas whenever a
  commit touches a table the query reads, so **an agent's write through a tool updates every open window with no UI
  code**. A windowed query (`page`) needs a total order: end its `order by` with the key.
- **`app.kv`** is for small values (the Calculator keeps its history there). SQL `ATTACH` is denied through
  `app.db`, so `app.db` SQL cannot leave the app's own file (`vnd:rusqlite` can open any file: see Security).

## Tools

```lua
app.tool {
  name = "history",                         -- [a-z][a-z0-9_]*; agents see it as "calculator__history"
  title = "Calculation History",
  description = "List recent calculations, newest first.",   -- agents read this: say what it does
  readOnly = true,
  input = { limit = app.t.integer { default = 10, min = 1, max = 50 } },
  run = function(input, ctx) ... return { entries = ... } end,
}
```

- **Names:** the local name is `[a-z][a-z0-9_]*` (no dots); the MCP name is `<appId>__<tool>`.
- **Inputs** are described with `app.t` builders (`description`, `default`, `min`, `max`, `minLength`, `maxLength`,
  `optional`); a field is required unless it is `optional` or has a default. The host validates input before the VM
  runs, so `run` receives well-typed values; still describe every field, because agents read them.
- **`ctx`:** `ctx.actor` (`{ kind = "user" | "agent", name }`), `ctx.now`, `ctx:fail(message)` to report a problem to the
  caller. Raising an error does the same with the error's message.
- **`readOnly = true` is enforced, not a hint, for the `app.*` API.** The tool runs with app writes blocked:
  `db:exec`, writes in `db:tx`, DDL, `app.kv:set/delete` and `db:migrate` raise, and so do assigning `app.state`,
  `app.notify`, `app.shell.*`, `openWindow` (a window's `onOpen` could write) and registering anything
  (`app.on`, `app.window`, `app.tools.register`, `app.thread`). Reads (`db:all`, `db:one`, `db:query`) work. The
  restriction is inherited: a task, deferred or delayed function the tool starts stays read-only for as long as it
  lives, and a request to your own server while a read-only call or task is alive runs read-only too. It does **not**
  cover the neoc modules: `std:fs`, `vnd:rusqlite`, `std:net` and the rest are open to every tool, `readOnly` or not.
  In return, an agent whose approval policy is `auto_read_only` (the
  default) may run it without a permission prompt. A tool that must write (the Calculator's `calculate` records
  history) is not `readOnly`.
- **Who can call it:** an agent sees and may call only the tools of the apps it was granted (deny by default; see
  `host-stack.md`, "Agents ... identity and permissions"), and every call is attributed to it. A tool can bring the UI
  forward: `show_on_calculator` sets the shared state and calls `app:openWindow("main")`, and every open window
  re-renders by itself.

## Notifications and shell

`app.notify { title = "...", body = "...", actions = {...} }` (or `app:notify { ... }`) raises a notification.
`app.shell.badge(value)`, `app.shell.command { id, title, shortcut, run }` and `app.shell.deepLink(pattern, handler)`
are declarations the shell does not act on yet. `w:toast { ... }` is the in-window message.

## Agents and chats

An app can read the roster with `app.agents:list()` (`{ id, name, avatar, role }`: never
grants, tokens or launch commands). It shows a chat with a thread source:

```lua
local chat = app.thread()
-- in render:
ui.ChatTranscript { key = "chat", thread = chat { thread = c.threadId, agent = current.agent_id }, onSend = function(e) ... end }
```

A thread source declares only the thread and the agent (literals or Refs); it runs no query and sends no data. The
client opens the conversation on `/agui` (AG-UI over SSE, mapped from the agent's ACP session), so messages, tool calls,
permission prompts and the agent's settings live with the host, and the app never sees them. The `send` and `stop`
events also reach the app (`e.payload.text`), which is how the Agents app titles a chat and keeps its chat list in
SQLite (`apps/agents/app.lua`).

## Background work

An app can work while nobody has a window open: a counter, a poller, a server for the LAN, a download. The host runs
it in the app's own VM and keeps it going for as long as the app is loaded.

- **Start it at the top level with `task.spawn`, and let the entry return.** The app is not loaded until its entry
  chunk has finished, so a loop written directly in the entry (or a wait that never ends) holds the load up for ever.
  Start the loop in a task instead; servers (`vnd:hyper`, `lib:http`) already start without blocking:

  ```lua
  local task = require("std:task")
  local s = app.state { ticks = 0 }
  task.spawn(function()
    while true do
      task.wait(0.2)
      s.ticks = s.ticks + 1
    end
  end)
  ```

  Use `task.delay` for a job that runs once later. `crates/agentos-host/tests/fixtures/ticker` is a complete example
  (a loop, a window that shows it, a tool that reads it).
- **Closing a window leaves the work running.** A window is a view of the app, not the app. Tools, tasks and servers
  go on with no window open, and a window opened later shows the state as it is by then.
- **Changing `app.state` from the background re-renders open windows.** Every window whose last render read a field you
  assigned is re-rendered and its changes are sent to the client, at the next wait of the task (`task.wait`, the
  network, a worker) or when it ends, with no event or tool call needed. Assigning the value a field already holds
  changes nothing. Keep the loop's slice short: between two waits the app runs one call at a time, so a long
  computation holds up tools, events and renders. Windows also follow your `app.db` writes (live sources) and a
  `w:toast` or `w.client:set` made from a window you hold.
- **Unloading stops and releases it.** Disabling, uninstalling or replacing the app aborts its tasks, closes its servers
  and connections and stops its workers (see Security). What a task was about to do is dropped, not delivered.

## Security

- **Apps keep their Lua state apart; that is not a security boundary.** One Lua VM per app (LuaJIT, running the app's Lua 5.1 source), and one
  **environment table** per app (its globals are its own; two apps share no Lua state, not even the module tables
  below). Through `std:fs` and `vnd:rusqlite` an app can still read and write other apps' data and the host's own
  database under `~/.agentos`, and a runaway app (a loop that never yields, a `std:thread` worker) can hang the host
  and outlive its uninstall: unloading stops an app's work only where it yields to the host. The base functions are
  an allow-list: no `load`, `loadstring`, `dofile`, `loadfile`, `getfenv`, `setfenv`, `newproxy`, `module`,
  `collectgarbage`, `print`, no `string.dump`, no `io`, `os`, `debug` or `package`. Chunks load as text only (Lua 5.1
  bytecode is unverified). `string`, `table`, `math`, `coroutine`, `bit` and `utf8` are **read-only**: assigning to
  one, `rawset` on one, `setmetatable` on one and `getmetatable("")` (the shared string metatable is locked) all fail,
  so an app cannot change what other code in its VM sees. `math.random` has a generator of its own per VM, seeded by
  the host. The functions themselves are Lua's own (C `string` and `table`, including `table.sort` and the pattern
  matcher).
- **Every library module is yours, with no permission to ask for.** Next to `require("@agentos/app")` and
  `require("@agentos/ui")`, an app can `require("std:fs")`, `require("lib:json")`, `require("vnd:hyper")` and the rest
  of `neoc-vm`'s modules, spelled as neoc spells them: `std:` (collections, env, fs, io, net, task, thread, time),
  `lib:` (archive, args, base64, cache, chrono, compress, crypto, csv, diff, glob, hash, hex, hmac, html, http, ini,
  json, log, mime, pkey, random, regex, semver, template, tempfile, text, unicode, url, vcr) and `vnd:` (hyper,
  jsonschema, quick_xml, rusqlite, serde_json, serde_yaml, sqlx_mysql, sqlx_postgres, testcontainers_modules, toml). Not offered: `lib:test`,
  `lib:compile`, `std:workers`. Files, the network, servers, background tasks and worker threads are all available;
  `vnd:hyper` speaks `http://` and `https://` (the operating system's certificate store, or `ca_pem` for your own
  CA). `lib:log` goes to the host's log with the app id. The host has no permission model for apps and no CPU, memory
  or time limit: an app that loops forever costs the host a thread, and one that never yields to the host (a tight loop, a `std:thread` worker) is not stopped by an unload and can hang the host. That is accepted.
- **What an app starts is the app's.** Tasks (`std:task`), servers (`vnd:hyper`, `lib:http`), their connections and
  `std:thread` workers are aborted and their listeners closed when the app is unloaded, disabled, uninstalled or
  replaced (a replacement can bind the port the old version had). A callback that comes from inside the VM, a request
  to your own server or the next slice of a task, runs as one call at a time with your tools and events: between two
  waits (`task.wait`, the network, a worker) it is atomic, and a wait lets the others in. Do not wait inside `db:tx`
  (it raises). See "Background work" below for what a background callback can change.
- **Still enforced.** Values leaving Lua are capped at 100 000 nodes and 4 MiB and may not contain cycles (view trees:
  10 000 nodes, depth 64): a cyclic table is an error, not a crash. Queries return at most 10 000 rows and 16 MiB;
  BLOBs come out as base64 text. SQL `ATTACH` is denied through `app.db`. `readOnly` tools cannot write through
  `app.*` (see above; the neoc modules are not covered). A Lua number cannot panic a module: sizes, sleeps and
  indices are bounded and answer with an error.
- **Lua limits to write around.** A host call (`app.db:*`, `app.kv:*`) may be made inside `pcall`, inside a
  coroutine, in a metamethod, in the iterator function of a `for ... in` and in any handler, but not where the
  engine (LuaJIT) cannot yield: a `string.gsub` callback, a `table.sort` comparator, `string.format("%s")` of a
  `__tostring`, or the handler of an `xpcall` (it fails with "attempt to yield across C-call boundary"). `return app.db:all(...)` (a tail call) loses the line number in an error message. `math.random` of a
  bound that is not a finite number (`1e300`, `NaN`, an infinity) is an error. `pairs` and `next` of
  a library table (`pairs(string)`) see nothing. An error raised by a host function (`app.db`, `app.kv`, and
  `math.random`, which the host implements) reaches a `pcall` as an error object, not a plain string: write
  `tostring(err)` before concatenating or matching it.
- **Coroutines, abandoned calls and transactions.** Three things a script can run into when it is clever with
  coroutines or with `db:tx`:
  - *A host call Lua could not suspend at is over.* If a host call is made where Lua cannot yield (the list above) and
    the script catches the error and goes on, that call is abandoned: whatever it held is released at once, and
    resuming a coroutine that was waiting at it raises `this host call was abandoned`.
  - *You can resume only the coroutines you created yourself.* `coroutine.resume` (and a `coroutine.wrap` function)
    works on a coroutine your own code made with `coroutine.create` or `coroutine.wrap`, and on nothing else.
    Every other thread belongs to the host and is refused: the top-level thread of a call (a tool, an event, a
    window callback, a request handler, a `db:tx` or `Mutex:with_lock` body, the app's entry), a task's thread
    (a `pcall` runs in its caller's thread, so it has none of its own), however you got hold of one (a
    `coroutine.running()` you stashed in a variable and resume from another tool). The refusal is `false, "cannot resume non-suspended
    coroutine"`, and a `coroutine.wrap` function raises it; nothing changes in the thread you tried to resume. A
    coroutine you did create is refused the same way while a host call it is waiting at is in flight (another thread
    is waiting for it) and once you gave it to `task.spawn`, `task.defer` or `task.delay` (the scheduler drives it
    now). Those three take only coroutines you created: `task.spawn(coroutine.running())` raises `cannot schedule a
    coroutine that is not yours`, and scheduling one that is already being driven raises `cannot schedule a
    coroutine that is already running`. Yielding across `pcall` inside your own coroutines works as in Lua.
  - *`db:tx` and reloads.* A transaction ends with the app version that opened it: unloading the app or reloading it
    (a hot reload, an update) rolls an open `db:tx` back, and the call that was inside it gets `the transaction has
    ended` from its next statement or from `db:tx` itself. A statement whose host call was abandoned inside `db:tx`
    (it was started where Lua could not suspend, and the script went on) was still queued at the moment it was
    called, so it runs and **commits with the transaction** if the transaction commits; the abandoned call only loses
    its result.
- **Supply chain.** The host verifies the sha256 of every file in an app ref before loading it. That guards against
  tampering in transit, not against a malicious author: the add-by-URL preview shows the source and asks
  first, and an app's tools reach every agent granted to that app.
- **Agents are untrusted callers, and so is what they write.** Tool input is validated against the declared schema;
  `readOnly` is enforced by the data layer; strings an agent or a tool returns reach the client as text, never HTML.
  Do not build SQL by concatenating them: use named params.
- **The client is not trusted either.** Events carry a key path and values; the host looks the handler up in the VM, so
  a client cannot invoke anything the tree did not offer.
- **Other apps are reachable only through tools by the API; the modules are open.** `app.*` gives an app no way
  to another app's data or functions, but `std:fs` and `vnd:rusqlite` can open the files, so do not treat another
  app's data as private from your own code.

## Adding an app

1. **Create the package.** Copy `apps/calculator` (read [its README](calculator/README.md) first) to `apps/<id>` and edit `app.json` (id, name, version `0.1.0`,
   description, icon, tint). Rewrite `app.lua`, starting with the two `require`s and `app.manifest { ... }`.
2. **Set the window.** `window = { w, h, minW, minH }`. The minimum is the smallest size at which every control is
   fully visible and at least 44 pt (the Calculator's 320x500 is justified by the keypad: say why in a comment when
   it is more than 320x240). The host clamps the window to it.
3. **Write the logic as plain Lua functions** (the Calculator's `calc.lua`), the view as `render`, and the tools.
   Every interactive node gets a `key`; every tool gets a described schema and a `readOnly` that is true when the tool
   does not write.
4. **Make it clean.** `bun run lint:lua` (`lua-language-server --check` against the generated definitions, which
   cover `@agentos/app`, `@agentos/ui` and every `std:`, `lib:` and `vnd:` module you `require`) and
   `bun run lint:stylua` pass; `bun run format` fixes formatting. Fix what the checker reports, do not silence it.
5. **Test the logic and tools in Rust.** Add a test beside `crates/agentos-lua/tests/calculator_app.rs` that loads the
   shipped package under the real runtime, calls its tools (`call_tool`) and asserts results and
   `readOnly`. A host-level test (`crates/agentos-host/tests/agents_app.rs`) is for what needs the data layer, views or
   agents. Run `cargo nextest run -p agentos-lua`.
6. **Write `JOURNEYS.md`** (4 to 8 journeys: id, who and what they want, start state, an observable "Done when"; never
   steps, including an error case, a minimum-window case, a keyboard-only case and, where it applies, an agent driving
   the app) **and `e2e/journeys.spec.ts`** with a test of the same id per journey, driven by test ids on the real host
   (`apps/calculator/e2e/journeys.spec.ts`: `openDesktop`, `callTool` for the agent journey). An app that writes shared
   state runs its journeys serially and clears that state first.
7. **Cover the states.** The `desktop` project opens your app in the real shell on its own (floating light/dark, its
   minimum and a large window, every layout, axe, snapshots). For a state a journey does not reach, add a JSON-tree
   story to the client Storybook (`client/src/view/stories`); the kit's own components have stories in
   `packages/ui`.
8. **Try it for real.** `bun run dev`, open `http://127.0.0.1:5173`, pair with the code the host prints, install the app
   from Discover and use it with the keyboard, in light and dark, at its minimum size. Then the `verify-change` skill,
   and `bun run test:e2e` on a Mac (update the baselines deliberately and look at them).
9. **Design it.** Run the `app-design` loop (`ux-research` rounds) until the exit criteria hold, with the report at
   `docs/ux/<id>/round-<n>.md`.
10. **Ship it** through a PR with the full gate and the readiness table in `agentos-code-review` §6. Until then the app
    stays in its MR. To change a released app, bump `version` in `app.json`.

## Rules that are enforced

- `bun run lint:lua` and `lint:stylua` (types and format of every app), `bun run lint:testids` (test ids in the
  kit and the client), `bun run codegen:check` (generated types match the Rust schema).
- In the host, always: app ids, manifests and app refs are validated and checksum-verified at install; tool input is validated against the schema; `readOnly` blocks writes.
- Missing a capability? Extend the host API (domain type and schema, application port and use case, the Lua adapter,
  codegen, tests), never a side channel. That is a platform change: call it out in the MR.
