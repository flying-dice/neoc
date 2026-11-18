# Building AgentOS apps

An AgentOS app is a **server-side Luau package** that runs inside the Rust host. It holds its state, data, tools,
notifications and a description of its UI. It ships no client code: the managed client renders the view trees the app
produces and sends events back.

```
 Browser: managed client                         Rust host (crates/agentos-host)
 ┌──────────────────────────────┐  JSON-RPC/WS   ┌─────────────────────────────────────────┐
 │ shell: windows, Dock         │◀──── /rpc ────▶│ one sandboxed Luau VM per app           │
 │ renderer: view trees → kit   │                │   app.window → view tree → client       │
 │ client state, drafts         │  AG-UI / SSE   │   app.db / app.kv  (per-app SQLite)     │
 │ no app code, ever            │◀──── /agui ────│   app.tool ──▶ /mcp ◀── agents          │
 └──────────────────────────────┘                │ app refs ◀── file:// http(s):// ────────│
                                                 └─────────────────────────────────────────┘
```

The reference apps are `apps/calculator` (a keypad window, shared state, three agent tools) and `apps/agents` (a
SQLite-backed chat list around the `ChatTranscript` component). Read them next to this page. The architecture is in
`docs/architecture/` (`host-stack.md` for the host, `ui-system.md` for the UI protocol and the Luau API); the typed API
is `types/generated/agentos.d.luau`; the components are listed in `docs/reference/components.md`. The last two are
generated from the Rust schema (`bun run codegen`): never edit them.

## The package

```
apps/<id>/
  app.json         the package manifest (below)
  app.luau         the entry (`main` in app.json): declares the manifest, windows, tools, data
  *.luau           modules the entry loads with require("./calc") (calc.luau in the Calculator)
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
  "main": "app.luau",
  "icon": "plus",
  "tint": "orange",
  "permissions": ["data"],
  "requires": { "host": ">=1.0" }
}
```

| Field | Rule |
|---|---|
| `id` | 3 to 40 characters: lowercase letters, digits, dashes, not starting with a dash. Namespaces the app's data and its tools (`<id>__<tool>`). |
| `name`, `version` | `version` is `major.minor.patch` digits (`0.1.0`); bump it to ship a change. |
| `main` | the Luau entry file (default `app.luau`). |
| `icon`, `tint`, `description` | what Discover and the add-by-URL preview show *before* the app runs. `icon` is a symbol name, `tint` a palette role. |
| `permissions` | any of `data`, `notify`, `shell`, `agents`. Nothing else is granted (see "Security"). |
| `requires.host` | the host API range the app needs (checked against the host at install). |
| `migrations` | optional list of `{ version, name, file }` SQL files; most apps call `db:migrate` in Luau instead. |

`app.json` and the `app.manifest { ... }` call in the entry must agree on `id`, `icon` and `tint`: `bun run build:apps`
fails when they differ. `app.manifest` additionally carries the window (`w`, `h`, `minW`, `minH`) and may repeat
`permissions`.

### App refs, registries and install

`bun run build:apps` (`scripts/build-apps.ts`) turns every `apps/*/app.json` into an **app ref** under
`dist/apps/<id>/<version>/`: `app-ref.json` (`{ manifest, files: [{ path, url, sha256 }] }`, file URLs relative to it)
plus the Luau files and migrations, copied as they are, and one `dist/apps/registry.json`
(`{ "apps": ["calculator/0.1.0/app-ref.json", ...] }`). The package is every `.luau` file except in `tests`, `e2e`,
`negative` and `node_modules` folders.

- **A registry** is a list of app-ref URLs. The host's default registry is `dist/apps/registry.json` over `file://`
  when it exists; `registries` in `agentos.toml` (or `AGENTOS_REGISTRIES`) adds more. Discover lists what they offer.
- **An app is installed from an app ref URL** (`file://`, `http://`, `https://`): the host fetches it, verifies every
  file's sha256, runs the migrations, loads the app in its VM and publishes its tools. Discover previews an app ref
  first (manifest, source host, permissions) without running anything, then asks for confirmation. `install` in
  `agentos.toml` makes sure listed app refs are installed at every start.
- **Lifecycle:** install, enable, disable (VM unloaded, tools withdrawn, data kept), delete data, uninstall. At start
  the host re-fetches and re-verifies every installed app. An app that fails to load is marked failed and the rest
  start. All of it is in `docs/architecture/host-stack.md` ("Apps: lifecycle").
- **Dev loop:** `bun run dev` builds the apps, starts the host with `dev_reload` and Vite. Save a `.luau` file, run
  `bun run build:apps`: the host rebuilds that app's VM and keeps its database (the old version keeps running if the new
  one does not load).

## The `app` API

An app file starts like this (the Calculator):

```lua
--!strict
local app = require("@agentos/app")   -- the host module; also a global named `app`
local ui = require("@agentos/ui")     -- component constructors and helpers, generated from the component schema
local calc = require("./calc")        -- a module of this package: require is bundle-only
```

`require` accepts `./x`, `../x`, `@agentos/app` and `@agentos/ui`: nothing else, and never the filesystem. The
definitions in `types/generated/agentos.d.luau` are the contract; `bun run lint:luau` type-checks every app against
them in strict mode.

| Call | What it does |
|---|---|
| `app.manifest { id, name, icon, tint, window = { w, h, minW, minH }, permissions }` | identity and the window's preferred and **minimum** size. Call it first. |
| `app.window(name, { title, size, client, state, render, onOpen, onClose })` | declare a window kind. `render(w)` returns the view tree. |
| `app.state(initial)` | shared in-memory state for the whole app; reads in `render` are tracked, assignments re-render the windows that read them. Lost on restart: persist what matters with `app.kv` or `app.db`. |
| `app.kv:get(key)`, `:set(key, value)`, `:delete(key)` | small durable key/value store (needs `data`) |
| `app.db` | `migrate`, `exec`, `all`, `one`, `tx`, and the live sources `query`, `row`, `scalar` (needs `data`) |
| `app.tool { ... }` (alias `app.tools.register`) | declare an agent tool |
| `app.t.string / integer / number / boolean / array / object / enum` | input schema builders for tools |
| `app.notify { title, body, at, actions }` | a notification (needs `notify`) |
| `app:openWindow(name, params)`, `app:windows(name)` | bring a declared window forward (an undeclared name raises), or iterate the open instances |
| `app.thread()` | declare a chat-thread source for `ChatTranscript` (needs the `agents` permission; a literal agent must be on the roster) |
| `app.agents:list()` | the agent contacts, as `{ id, name, avatar, role }` (needs `agents`) |
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
- **Handlers** are a Luau function `function(e) ... end` (`e.key`, `e.row`, `e.value`, `e.payload`, `e.actor`), a
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
- **`app.kv`** is for small values (the Calculator keeps its history there). SQL `ATTACH` is denied: an app's SQL
  cannot leave its own file.

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
- **`readOnly = true` is enforced, not a hint.** The tool runs with app writes blocked: `db:exec`, writes in `db:tx`,
  DDL, `app.kv:set/delete` and `db:migrate` raise, and so do assigning `app.state`, `app.notify`, `app.shell.*` and
  `openWindow` (a window's `onOpen` could write). In return, an agent whose approval policy is `auto_read_only` (the
  default) may run it without a permission prompt. A tool that must write (the Calculator's `calculate` records
  history) is not `readOnly`.
- **Who can call it:** an agent sees and may call only the tools of the apps it was granted (deny by default; see
  `host-stack.md`, "Agents ... identity and permissions"), and every call is attributed to it. A tool can bring the UI
  forward: `show_on_calculator` sets the shared state and calls `app:openWindow("main")`, and every open window
  re-renders by itself.

## Notifications and shell

`app.notify { title = "...", body = "...", actions = {...} }` (or `app:notify { ... }`) needs the `notify` permission.
`app.shell.badge(value)`, `app.shell.command { id, title, shortcut, run }` and `app.shell.deepLink(pattern, handler)`
need `shell`; they are declarations the shell does not act on yet. `w:toast { ... }` is the in-window message.

## Agents and chats

An app with the `agents` permission can read the roster with `app.agents:list()` (`{ id, name, avatar, role }`: never
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
SQLite (`apps/agents/app.luau`).

## Security

- **Sandbox.** One Luau VM per app, `sandbox(true)`: builtins read-only, no `io`, `os.execute`, `dofile` or `debug`, no
  filesystem `require`, no network. A **memory limit** (16 MiB by default) and a **CPU budget** per call (`Limits::cpu_ticks`, 5 000 000
  VM interrupts, about 250 ms of Luau on an idle machine, so a loaded host does not trip it; a 10 s wall-clock
  backstop, `cpu_wall`, catches the rest; sticky so `pcall` cannot swallow it; time waiting on the host does not count) kill a
  runaway app, not the host. A **per-call budget** that host calls do not refill (25 000 000 ticks, 30 s including host
  waits) stops a loop of host calls; at most 32 effects (notifications, windows, shell commands) per call; values
  leaving Luau are capped at 100 000 nodes and 4 MiB and may not contain cycles (view trees: 10 000 nodes, depth 64).
  Queries return at most 10 000 rows and 16 MiB; BLOBs come out as base64 text. `loadstring`, `getfenv` and
  `setfenv` are not available. A blocking host call has its own timeout. SQL `ATTACH` is denied and a statement past
  its wall-clock budget is interrupted.
- **Permissions** are what `app.json` declares and the user confirmed at install; the host checks them at each call
  (`permission denied: app.notify needs the notify permission in the package manifest`), so an app cannot grant itself
  more by editing its manifest call.
- **Supply chain.** The host verifies the sha256 of every file in an app ref before loading it. That guards against
  tampering in transit, not against a malicious author: the add-by-URL preview shows the source and permissions and asks
  first, and an app's tools reach every agent granted to that app.
- **Agents are untrusted callers, and so is what they write.** Tool input is validated against the declared schema;
  `readOnly` is enforced by the data layer; strings an agent or a tool returns reach the client as text, never HTML.
  Do not build SQL by concatenating them: use named params.
- **The client is not trusted either.** Events carry a key path and values; the host looks the handler up in the VM, so
  a client cannot invoke anything the tree did not offer.
- **Nothing reaches another app but tools.** An app cannot read another app's data or call its functions.

## Adding an app

1. **Create the package.** Copy `apps/calculator` to `apps/<id>` and edit `app.json` (id, name, version `0.1.0`,
   description, icon, tint, the smallest `permissions` list that works). Rewrite `app.luau`, starting with
   `--!strict`, the two `require`s and `app.manifest { ... }`.
2. **Set the window.** `window = { w, h, minW, minH }`. The minimum is the smallest size at which every control is
   fully visible and at least 44 pt (the Calculator's 320x500 is justified by the keypad: say why in a comment when
   it is more than 320x240). The host clamps the window to it.
3. **Write the logic as plain Luau functions** (the Calculator's `calc.luau`), the view as `render`, and the tools.
   Every interactive node gets a `key`; every tool gets a described schema and a `readOnly` that is true when the tool
   does not write.
4. **Make it clean.** `bun run lint:luau` (strict `luau-lsp analyze` against the generated definitions) and
   `bun run lint:stylua` pass; `bun run format` fixes formatting. Fix the types, do not cast them away.
5. **Test the logic and tools in Rust.** Add a test beside `crates/agentos-luau/tests/calculator_app.rs` that loads the
   shipped package under the real runtime, calls its tools (`call_tool`) and asserts results, permission denials and
   `readOnly`. A host-level test (`crates/agentos-host/tests/agents_app.rs`) is for what needs the data layer, views or
   agents. Run `cargo nextest run -p agentos-luau`.
6. **Write `JOURNEYS.md`** (4 to 8 journeys: id, who and what they want, start state, an observable "Done when"; never
   steps, including an error case, a minimum-window case, a keyboard-only case and, where it applies, an agent driving
   the app) **and `e2e/journeys.spec.ts`** with a test of the same id per journey, driven by test ids on the real host
   (`apps/calculator/e2e/journeys.spec.ts`: `openDesktop`, `callTool` for the agent journey). An app that writes shared
   state runs its journeys serially and clears that state first.
7. **Cover the states.** The `desktop` project opens your app in the real shell on its own (floating light/dark, its
   minimum and a large window, every layout, axe, snapshots). For a state a journey does not reach, add a JSON-tree
   story to the client Storybook (`client/src/renderer/src/view/stories`); the kit's own components have stories in
   `packages/ui`.
8. **Try it for real.** `bun run dev`, open `http://127.0.0.1:5173`, pair with the code the host prints, install the app
   from Discover and use it with the keyboard, in light and dark, at its minimum size. Then the `verify-change` skill,
   and `bun run test:e2e` on a Mac (update the baselines deliberately and look at them).
9. **Design it.** Run the `app-design` loop (`ux-research` rounds) until the exit criteria hold, with the report at
   `docs/ux/<id>/round-<n>.md`.
10. **Ship it** through a PR with the full gate and the readiness table in `agentos-code-review` §6. Until then the app
    stays in its MR. To change a released app, bump `version` in `app.json`.

## Rules that are enforced

- `bun run lint:luau` and `lint:stylua` (types and format of every app), `bun run lint:testids` (test ids in the
  kit and the client), `bun run codegen:check` (generated types match the Rust schema).
- In the host, always: app ids, manifests and app refs are validated and checksum-verified at install; permissions are
  checked at each capability call; tool input is validated against the schema; `readOnly` blocks writes.
- Missing a capability? Extend the host API (domain type and schema, application port and use case, the Luau adapter,
  codegen, tests), never a side channel. That is a platform change: call it out in the MR.
