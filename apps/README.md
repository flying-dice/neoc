# Writing a vos app

An app is a **web client and a Luau backend**. The client is the app's screens, written in any web framework (Svelte,
React, plain JavaScript) and shipped as one prebuilt script; the host loads it into a frame of the app's window in the
shell. The backend is **typed Luau** (`--!strict`) that runs in the app's own VM on the host: its data, its tools for
agents, its background work. The client calls the backend by name, and the backend can tell the client when something
changed. The client reaches nothing else.

**This page describes the transport an app runs on: how a client script and a Luau backend meet in the host.** An app
is meant to be built as a Vite project whose RPC is defined once and from which a vos Vite plugin generates the typed
client, the Luau router and the handler interface (NEOC-300, next change); until it lands, the client script and the
handler table are written by hand, as below, and that is the layer the generated code will sit on. Don't treat the
hand-built layout as the final workflow.

Two apps show the model: `e2e/apps/tasks-svelte` and `e2e/apps/tasks-react` (the same task list written once in each
framework, on the same backend; they are test fixtures, not in Discover). The older view-tree apps, `apps/calculator`
and `apps/agents`, describe their screens in Luau instead and have no client (see "View-tree apps" below; NEOC-298
ports them and retires that model). The backend API is typed in `types/generated/vos.d.luau` (generated from the Rust
schema by `bun run codegen`; never edit it): every `app.*` call and every `std:`/`lib:`/`vnd:` module an app may
`require`. It is the reference; hover docs show in an editor running luau-lsp with
`luau-lsp.types.definitionFiles = { "@vos": "types/generated/vos.d.luau" }` and `luau-lsp.platform.type = "standard"`.

## The package

```
apps/<id>/                    (or e2e/apps/<id> for a test fixture)
  app.json             the package manifest
  app.luau             the backend: the entry (`main`), returns the handlers
  *.luau               modules the entry loads with require("./calc")
  package.json         the client's package: `bun run build` writes dist/client.js
  src/                 the client's source, in whatever framework
  JOURNEYS.md          4 to 8 key user journeys (goal, start state, observable "Done when"; never steps)
  e2e/journeys.spec.ts their automated twins, one test per journey id
  tests/, negative/, e2e/   never shipped
```

`app.json`: `id` (3 to 40 characters: lowercase letters, digits, dashes; namespaces the app's data and tools), `name`,
`version` (`major.minor.patch`; bump it to ship a change), `main` (the backend), `client` (the client script's name in the
app ref, ending in `.js`), `window` (`{ w, h, minW, minH }`: the default size and the **minimum**), `icon`, `tint`,
`description`, `requires.host` and optional `migrations`. The shell needs the window and the client's name before any code
runs, so they are in `app.json` and the backend declares nothing about itself (no `app.manifest`).

**An app with a client is a Bun workspace package** (`apps/*` and `e2e/apps/*` are in the root `workspaces`): its
framework is an ordinary dependency, installed by the root `bun install`.

`bun run build:apps` builds each app. For the backend it bundles the entry with darklua into one `app.luau` (relative
`require`s inlined, types and comments stripped, statements kept on their source lines). For the client it runs `bun run
build` in the app's folder and takes `dist/client.js`. Both go into an **app ref** (`app-ref.json` with a sha256 per
file) under `dist/apps/<id>/<version>/`, plus `dist/apps/registry.json`. The host installs an app from an app ref URL
(`file://`, `http://`, `https://`), verifies every sha256 (the client's too) and runs the migrations. `bun run dev`
rebuilds on save and the host reloads the app's VM, keeping its database; reopen the window to load a new client.
`darklua` 0.18.0 must be installed; the build fails without it.

## The client

The client is **one classic script** that defines the global `vosApp` with `mount(el, { rpc, host })`, which draws into
`el` and returns a function that unmounts it. With Vite, in each framework:

```js
// the entry (Svelte shown; React is createRoot(el).render(...) and root.unmount())
import { mount as mountApp, unmount } from 'svelte'
import App from './App.svelte'
export function mount(el, props) {
  const app = mountApp(App, { target: el, props })
  return () => unmount(app)
}

// vite.config: a library build to one IIFE named vosApp
build: { lib: { entry: 'src/main.ts', name: 'vosApp', formats: ['iife'], fileName: () => 'client.js' } }
```

Keep the **styles inside the script** (Svelte: `compilerOptions: { css: 'injected' }`; React: a `<style>` you add when
`mount` runs) and set `data-testid` on every interactive element yourself (`<appId>-<thing>`).

What `mount` gets:

| | |
|---|---|
| `rpc(method, args)` | calls the backend handler `method` with `args` (any JSON); resolves with its result, rejects with an `Error` whose message is the handler's (`ctx:fail(...)`, or the error it raised) |
| `host.on(name, fn)` | hears an event, returns a function that stops listening: one the backend sent with `app.emit(name, data)`, or the host's own, whose names start with `host:` |
| `host.theme` | `"light"` or `"dark"`: the shell's appearance now. `host:theme` is the event when it changes, and `<html data-theme>` and `color-scheme` follow it, so style dark as `html[data-theme="dark"]` |

**What the frame is.** The app's window is an `<iframe sandbox="allow-scripts allow-forms">` that loads `/frame/<id>` from the host. The
frame makes no request of its own (its content security policy allows none: `fetch`, WebSocket, scripts, images and
fonts from anywhere but the host fail), has no access to the shell's page, cookie or storage (`localStorage`,
`document.cookie` and `parent.document` throw), and has one way out: `rpc` reaches **this app's** backend, and the host
knows which app is calling from the frame the message came from, never from anything the client says. The client's styles
and scripts stay in its frame. The sandbox also stops it opening windows (`window.open` returns `null`), navigating the
shell (`window.top.location` throws). **Forms work** (the sandbox has `allow-forms`): Enter in a field, a submit button,
the `submit` event, validation and `FormData` behave as in any page, and your framework's `onSubmit` runs. A submission
that nothing prevented navigates the frame away: the policy's `form-action 'none'` refuses to send the data, but the page is
gone and the window says the app left its page, so call `preventDefault()` in your submit handler, as a single-page app does
anyway. What the policy cannot stop is the frame
navigating **itself** (`location.href = ...`): the window then shows a page the shell did not load, and says the app left
its page and offers to open it again (a refused form submission is not that: nothing loads). This is sensible isolation, so apps do not trip over each other
or over the shell; it is not a security boundary. Lasting data belongs in the backend (the frame has no storage).

**Limits of a call.** `rpc`'s arguments may be at most 1,000,000 bytes of JSON and 100 levels deep, and a method name is 1 to
128 characters: a call over a limit rejects at once with a message that says which. A result may be at most 4 MiB, and a
larger one comes back as an error. A call the shell cannot read is rejected too, so a promise never hangs. Calls in
flight are limited so that one window can neither be refused by the host's own connection limit for an ordinary burst (a
call per item of a list) nor crowd out the shell or another window: 8 at a time per window and 32 across all windows, the
rest wait their turn (windows take turns); a window with 1,000 calls waiting gets an error for the next one.

**When a client fails to start**, the window shows the reason as text with a "Try again" button: the error the page raised
while it loaded (a top-level throw, a missing import, a syntax error: the script comes from another origin than the
frame's document, so the browser hides the message, and the window says that the script failed and that the browser console
has the error), a client that does not define
`vosApp.mount`, or an error thrown by `mount`.

The shell draws sheets, alerts and toasts above the frame (the shared UI kit and its tokens are NEOC-252; the kit will be
served at `/kit/tokens.css` and `/kit/kit.js`).

## The backend

The entry runs once when the app loads and **returns a table of handlers**, the functions a client reaches with
`rpc(name, args)`. A handler is called as `handler(ctx, args)` and returns any JSON value (`nil` is `null`):

```lua
--!strict
local app = require("vos:app")
local db = app.db

db:migrate { [1] = "create table tasks(id integer primary key, title text not null)" }

local handlers: vos_Handlers = {
  list = function(_ctx, _args)
    return db:all("select id, title from tasks order by id")
  end,
  add = function(ctx, args)
    if type(args) ~= "table" or type(args.title) ~= "string" then
      return ctx:fail("A task needs a title.") -- the client's rpc rejects with exactly this
    end
    db:exec("insert into tasks(title) values (:title)", { title = args.title })
    return db:all("select id, title from tasks order by id")
  end,
}
return handlers
```

- **`ctx`** is `{ actor, now, fail }`: `actor.kind` is `"user"` (it is the person's own client calling), `now` an ISO time,
  `ctx:fail(message)` ends the call with a message that has no file or line. Any other error the handler raises reaches
  the client as `app.luau:12: message`.
- **Services.** A handler, a tool and background work all reach the host the same way, through the `app` global:
  `app.db`, `app.kv`, `app.files`, `app.notify`, `app.log`, `app.agents:list()`, `app.emit`. There is nothing special about
  `ctx`. A handler may write: the person's client is calling. The entry may also return nothing (an app with only tools).
- **Events.** `app.emit(name, data)` tells the app's open clients something changed (they hear it with `host.on`): after
  an agent's tool changed data, or from a background task. `name` is 1 to 64 letters, digits and `. _ : -` (not `host:...`),
  `data` JSON of at most 64 KiB, at most 256 events waiting; a `readOnly` tool may not emit.
- **Calls** from a client, a tool and a task all queue behind one another: one runs at a time in an app's VM. A call to a
  handler that does not exist, or to an app that is not running, rejects with a message.
- **Tools** are separate from handlers (agents call tools over MCP, the client calls handlers), and unchanged:
  `app.tool { name, title, description, readOnly, input, run }` (MCP name `<id>__<name>`, local name `[a-z][a-z0-9_]*`);
  describe every input field with `app.t` builders (agents read them); the host validates input before `run`.
  `readOnly = true` is enforced for the `app.*` API (no db, kv, files, notify, emit or shell writes, no registering
  anything; tasks and requests the call starts inherit it) and lets an agent with the default `auto_read_only` policy
  call the tool without a prompt. It does not cover the `std:`/`vnd:` modules. A tool that writes is not `readOnly`.
  Handlers have no `readOnly`.
- **Background work:** start loops with `task.spawn` from `std:task` and let the entry return (the app is not loaded until
  it does). Closing a window leaves the work running, `app.emit` from a task reaches the open windows, and unloading aborts
  tasks and closes servers (`crates/vos-host/tests/fixtures/ticker` is an example).
- **Luau limits:** a host call (`app.db:*`, `app.kv:*`) cannot wait inside a `string.gsub` callback, a `table.sort`
  comparator, a metamethod other than `__call`, or an `xpcall` handler. No `print`, `os`, `debug`, `load*`; `bit` is
  `bit32`; `string.format("%s", v)` takes strings and numbers only. The root README lists the language differences.
- **Modules:** `lib:test`, `lib:compile` and `std:workers` are not offered (`vos_domain::NOT_FOR_APPS`); naming one is a
  type error. Everything else in `neoc-vm` is, with no permission to ask for. An app is the person's own code on their
  own host: `CLAUDE.md` ("Boundaries") says what the VM does and does not keep apart.
- **Your own modules are `any` in the editor.** luau-lsp resolves a relative `require` only while `require` is its
  built-in; `bun run lint:luau` checks them in a second pass. A wrong call into one is an error there, not in the editor.
- **Data:** `db:migrate` steps are numbered and run once each (a changed old step is an error). SQL `ATTACH` is denied
  through `app.db`.

## View-tree apps (retired by NEOC-298)

The Calculator and Agents are **view-tree apps**: they have no client; their Luau describes the screens and the managed
client renders them. Everything below, up to "Files", is that model; it keeps working beside the new one and is not extended.

### The package

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

### The `app` API

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
- **Notifications:** `app.notify { title = "Backup done", body = "12 files" }` tells the person something now. It takes
  a `title` (1 to 200 characters, not blank) and an optional `body` (up to 2000) and no other key; the host sends the
  app's id with it, so the sender is never text the app wrote. It goes to the clients connected at that moment: one that
  is not connected never gets it (nothing is queued). Bad input raises, each message starting `app.notify`: no table
  (`takes a table`), a key other than `title` and `body` (`unknown key`, or `unknown positional entry` for `app.notify {
  "text" }`), no title (`needs`), a title that is blank or too long, a body that is too long, a value that is not a
  string, and a call in `render` or a `readOnly` tool. A call may also have at most 64 notifications waiting to be
  delivered at once (`already has 64 notifications waiting`). The host delivers waiting notifications whenever a call of
  the app ends or a task or request handler waits, so the limit guards against a runaway loop; it is not a quota to plan
  around. `at` and `actions` are type errors too.
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

An app with a client:

1. **Package.** Copy `e2e/apps/tasks-svelte` (or `tasks-react`) to `apps/<id>`; edit `app.json` (version `0.1.0`: the
   `window` minimum is the smallest size where every control is fully visible and at least 44 pt) and `app.luau`.
2. **Backend** as handlers (logic in its own module, testable without the client), tools with a described schema and a
   truthful `readOnly`; `app.emit` wherever data changes outside the client.
3. **Client** with `data-testid` on every interactive element, working by keyboard, in light and dark and at the minimum
   window; `bun run build:apps` builds it.
4. **Clean.** `bun run lint:luau` and `bun run lint:stylua` pass, not skipped; `bun run format` fixes formatting.
5. **Tests:** a Rust test of the handlers and tools next to `crates/vos-lua/tests/handlers.rs`; a host test in
   `crates/vos-host/tests` for what needs the data layer or agents; `JOURNEYS.md` and `e2e/journeys.spec.ts` with a test of
   the same id per journey, reached through `page.frameLocator('[data-testid="<id>-frame"]')` (an error case, a minimum
   window, keyboard only, and an agent driving the app where it applies). The generic `desktop` project opens every
   installed app in the real shell (sizes, layouts, light and dark, axe, snapshots).
6. **Try it for real** (`verify-change`), then **design it** with `app-design` until its exit criteria hold, and ship it
   through a PR whose review applies the `pr-review` section "vos checks". Until then the app stays in its PR.

A view-tree app:

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
tests), never a side channel (the frame route and the frame's messages are part of the protocol), and call the platform
change out in the PR.
