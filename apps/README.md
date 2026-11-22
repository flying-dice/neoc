# Writing a vos app

An app is **one Vite project**: a web client (the app's screens, in any framework: Svelte, React, plain JavaScript) and a
**Luau backend** (typed, `--!strict`) that runs in the app's own VM on the host: its data, its tools for agents, its
background work. The two meet in one place, the app's **contract**: an OpenRPC (JSON-RPC 2.0) document written once, in a file,
that says which methods the client may call, with which params and what they return. The vos Vite plugin generates from it
the typed `api` client, the Luau router, and the Luau interface the backend implements. The developer writes
the client, and the implementation of the handlers; nothing else on either side of the RPC.

Two apps show it: `e2e/apps/tasks-svelte` and `e2e/apps/tasks-react`, the same task list written once in each framework, on
the same contract and the same backend (they are test fixtures, not in Discover). The older view-tree apps,
`apps/calculator` and `apps/agents`, have no client: their Luau describes the screens instead (see "View-tree apps" below;
NEOC-298 ports them and retires that model). The backend API is typed in `types/generated/vos.d.luau` (generated from the
Rust schema by `bun run codegen`; never edit it): every `app.*` call and every `std:`/`lib:`/`vnd:` module an app may
`require`. It is the reference; hover docs show in an editor running luau-lsp with
`luau-lsp.types.definitionFiles = { "@vos": "types/generated/vos.d.luau" }` and `luau-lsp.platform.type = "standard"`.

## The project

```
apps/<id>/                    (or e2e/apps/<id> for a test fixture)
  openrpc.json            the contract (OpenRPC): the methods, and `x-app`: the app's id, icon, tint, window
  vite.config.ts          plugins: [<your framework>(), vos()]
  package.json            scripts: generate, build, dev, typecheck, lint, format (a Bun workspace package)
  src/main.ts             the client: exports mount(el), returns an unmount function
  server/main.luau        the backend: one handler per method of the contract; tools, data, background work
  server/*.luau           modules the backend loads with require("./thing")
  .app/                   GENERATED (not committed): rpc.d.ts, rpc.js, entry.js, rpc.luau, entry.luau
  dist/                   BUILT (not committed): client.js, app.luau, app-ref.json
  JOURNEYS.md, e2e/       journeys and their specs, one test per journey id
```

**`openrpc.json` is the only descriptor.** `info.title` is the app's name and `info.version` its version
(`major.minor.patch`; bump it to ship a change); `x-app` holds what the shell needs before any code runs: `id` (3 to 40
characters: lowercase letters, digits, dashes; namespaces the app's data and tools), `icon`, `tint`, `description`,
`window` (`{ w, h, minW, minH }`: the default size and the **minimum**, the smallest size where every control is fully
visible and at least 44 pt), `requires.host` and optional `migrations`. A project has no `app.json` (the older apps do:
see "View-tree apps").

**An app is a Bun workspace package** (`apps/*` and `e2e/apps/*` are in the root `workspaces`): its framework and the plugin
are ordinary dependencies, installed by the root `bun install`.

```ts
// vite.config.ts
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'
export default defineConfig({ plugins: [svelte({ configFile: false, compilerOptions: { css: 'injected' } }), vos()] })
```

The project's `tsconfig.json` includes `".app/rpc.d.ts"` (the types of `app:rpc`; TypeScript does not pick up a dot folder
by itself).

`vos()` takes optional `{ definition, generated, client, backend }` (defaults `openrpc.json`, `.app`, `src/main.ts`,
`server/main.luau`). Vue needs `isCustomElement: (tag) => tag.startsWith('ui-')` in its plugin options and Angular
`CUSTOM_ELEMENTS_SCHEMA` for the kit's tags (NEOC-252); React and Svelte need nothing.

## The contract

`openrpc.json` is an [OpenRPC](https://spec.open-rpc.org/) document (the standard description of a JSON-RPC 2.0 service).
Requests, results and errors are JSON-RPC 2.0: a call is `{ method, params }`, params are **by name**, a result is any
JSON value, an error has a `code`, a `message` and optional `data`.

```json
{
  "openrpc": "1.3.2",
  "info": { "title": "Tasks", "version": "0.1.0" },
  "x-app": { "id": "tasks", "icon": "checkmark", "window": { "w": 420, "h": 480, "minW": 320, "minH": 320 } },
  "methods": [
    {
      "name": "add",
      "summary": "Add a task, and answer with the whole list.",
      "params": [{ "name": "title", "required": true, "schema": { "type": "string", "maxLength": 200 } }],
      "result": { "name": "tasks", "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Task" } } },
      "errors": [{ "code": -32010, "message": "A task needs a title." }]
    }
  ],
  "components": {
    "schemas": {
      "Task": {
        "type": "object",
        "properties": { "id": { "type": "integer" }, "title": { "type": "string" }, "done": { "type": "boolean" } },
        "required": ["id", "title", "done"]
      }
    }
  }
}
```

A deliberate subset, and an error that names where when the document goes beyond it: method and param names are identifiers
(they become `api.add({ title })` and `add = function(ctx, args)`); schemas are `type` (`string`, `number`, `integer`,
`boolean`, `null`, `array`, `object`, or a list of one of these and `"null"`), `enum` of strings, `minLength`, `maxLength`,
`minimum`, `maximum`, `items`, `properties` with `required`, `description` and `$ref` to `#/components/schemas/<Name>`. An
object takes no property it does not declare (`additionalProperties: false` is accepted and means the same). **Null and absent
are one value** (a Luau nil is an absent key): a property or a param whose schema allows null is never required to be
present, so TypeScript types it optional too (`tag?: string | null`), Luau types it `string?`, and the router lets it be
absent, whether or not the contract lists it under `required`.

Names: a param or property may be any identifier (`type`, `default`, `delete`, `new`: the generators quote it where a language
reads it as syntax); a method may not be a Luau reserved word (`end`, `function`, ...); a schema may not be a reserved word or a
builtin type of TypeScript or Luau (`string`, `buffer`, `class`, `await`, ...) but may be called `Record`, `Array` or `Promise`
(the generated types reach those through names a schema cannot shadow); `__proto__`, `constructor` and `prototype` are never a
name, and a schema never starts with `__vos`. A `$ref` cycle that does not pass through an array or an object property
(`A` is `B` is `A`) is refused with the cycle named. A migration's `file` is a plain relative name inside the project (no `..`,
no absolute path, no hidden segment): the build reads it from the project and writes it into `dist/`, and nothing else. Every type the generated files declare must have a name
of its own: a schema called `Host`, `Api`, `RpcError`, `Context` or `Handlers`, one named like a method's `<Method>Params` or
`<Method>Result` (`ListResult` for `list`), or two methods that are one name in PascalCase (`add_task` and `addTask`) are
refused when the contract is read, with the names that clash. Text from the contract (titles, summaries, enum values) is
written into the generated code as an escaped string or comment, never as code.

## What the plugin generates

`vos()` writes `.app/` when `vite build` starts (and again when the contract or a Luau file changes in watch mode), and
`bun run generate` does it on its own for the editor. Every script of the project generates first, so nothing in `.app/` is
committed.

- `.app/rpc.d.ts` and `.app/rpc.js`: the `app:rpc` module. `import { api, host, type Task } from 'app:rpc'`: a type for every
  schema, `api` (one typed function per method), `host`, and `RpcError`.
- `.app/entry.js`: what the build bundles. It binds `app:rpc` to the host and then calls the app's `mount(el)`.
- `.app/rpc.luau`: the same types in Luau, **`Handlers`** (the interface the backend implements), the schemas and
  **`router(handlers)`**. The router receives a call, holds its params to the definition (a refused call is the JSON-RPC
  error `-32602 Invalid params: params.title is required`, and the handler never runs; a param the contract does not
  declare is refused too), dispatches it, and holds what the handler returns to the definition as well (a mismatch is
  `-32603`, a bug in the backend); a method the contract does not have is `-32601 Method not found`.
- `.app/entry.luau`: the backend's entry, which the build bundles and the host runs: the handlers of `server/main.luau`
  behind the router.

The build checks `server/main.luau` against the contract with luau-lsp (strict, the same check as `bun run typecheck`): a
declared method with no handler, or a handler of the wrong shape, fails the build with the checker's message, and valid Luau
of any shape passes (a handler assigned from a local function, defined in a module you `require`, added to the table later).
A function in the table that the contract does not declare is never called. **The build needs luau-lsp** (1.70.1, on the PATH or
in `~/.local/bin`, or named by `VOS_LUAU_LSP`), as it needs darklua: without it the build stops and says how to install it, because
it could not say the app will load. `vos({ check: false })` turns the check off, for someone who knows what they are doing (the host
still refuses to load an app that does not implement every method). A build that fails at any step leaves `dist/` exactly as the
last good build made it: the build is made in `.app/build` and moved into `dist/` only when every step has succeeded.

## The client

`src/main.ts` exports `mount(el)`, draws into `el` and returns a function that unmounts it. The plugin builds it, behind its
generated entry, into **one classic script** (`dist/client.js`, an IIFE that defines the global `vosApp`, any CSS folded in;
a module script would need CORS in the app's frame):

```ts
import { mount as mountApp, unmount } from 'svelte'
import App from './App.svelte'

export function mount(el: HTMLElement): () => void {
  const app = mountApp(App, { target: el })
  return () => void unmount(app)
}
```

```ts
// anywhere in the client, in any framework
import { api, host } from 'app:rpc'

const tasks = await api.list()
await api.add({ title: 'Buy milk' }) // a call the contract does not allow is a type error
host.on('tasks.changed', () => reload())
```

Set `data-testid` on every interactive element yourself (`<appId>-<thing>`).

| | |
|---|---|
| `api.add({ title })` | the typed call: resolves with the result, or rejects with an `RpcError` (`code`, `message`, `data`) |
| `host.on(name, fn)` | hears an event, returns a function that stops listening: one the backend sent with `app.emit(name, data)`, or the host's own, whose names start with `host:` |
| `host.theme` | `"light"` or `"dark"`: the shell's appearance now. `host:theme` is the event when it changes, and `<html data-theme>` and `color-scheme` follow it, so style dark as `html[data-theme="dark"]` |

**What the frame is.** The app's window is an `<iframe sandbox="allow-scripts allow-forms">` that loads `/frame/<id>` from the host. The
frame makes no request of its own (its content security policy allows none: `fetch`, WebSocket, scripts, images and
fonts from anywhere but the host fail), has no access to the shell's page, cookie or storage (`localStorage`,
`document.cookie` and `parent.document` throw), and has one way out: `api` reaches **this app's** backend, and the host
knows which app is calling from the frame the message came from, never from anything the client says. The client's styles
and scripts stay in its frame. The sandbox also stops it opening windows (`window.open` returns `null`), navigating the
shell (`window.top.location` throws). **Forms work** (the sandbox has `allow-forms`): Enter in a field, a submit button,
the `submit` event, validation and `FormData` behave as in any page, and your framework's `onSubmit` runs. A submission
that nothing prevented is prevented for you, after your handlers have run, so a form whose handler forgot `preventDefault()` stays on
its page (a `<form method="dialog">` still closes its dialog). Two things still navigate the frame, which the policy's
`form-action 'none'` refuses to send data for but cannot keep on the page: `form.submit()` from script (it fires no event) and a form
inside a shadow root of your own (`submit` does not cross it); the window then says the app left its page. What the policy cannot stop is the frame
navigating **itself** (`location.href = ...`): the window then shows a page the shell did not load, and says the app left
its page and offers to open it again (a refused form submission is not that: nothing loads). **A client cannot reload its own
document in this version:** `location.reload()` asks the host for the frame's document again from an opaque origin, which
carries no session cookie, so the host refuses it, the window says the app left its page and Try again brings the app back;
this changes with the next pull request, where the app's own files are public. This is sensible isolation, so apps do not trip over each other
or over the shell; it is not a security boundary. Lasting data belongs in the backend (the frame has no storage).

**Limits of a call.** An `api` call's arguments may be at most 1,000,000 bytes of JSON and 100 levels deep, and a method name is 1 to
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

The policy lets a classic same-host script define custom elements and a shadow root carry `<style>`, so the shell's UI kit (Lit web components, NEOC-252) renders in it.

The shell draws sheets, alerts and toasts above the frame (NEOC-252; `host` has no `toast` yet); the kit will be served at
`/kit/tokens.css` and `/kit/kit.js`.

## The backend

`server/main.luau` implements the generated `Handlers` interface and **returns the handlers**: one function per method of the
contract, `handler(ctx, args)`. The generated entry puts the router in front of them.

```lua
--!strict
local app = require("vos:app")
local rpc = require("../.app/rpc")
local db = app.db

db:migrate { [1] = "create table tasks(id integer primary key, title text not null, done integer not null default 0)" }

local function list(): { rpc.Task }
  ... -- reads the rows (see e2e/apps/tasks-svelte/server/main.luau)
end

local handlers: rpc.Handlers = {
  list = function(_ctx)
    return list()
  end,
  add = function(_ctx, args) -- args.title is a string: the router checked it
    if args.title == "" then
      error("RPC-32602 Title cannot be empty", 0) -- the client's api.add rejects with code -32602 and this message
    end
    db:exec("insert into tasks(title) values (:title)", { title = args.title })
    return list()
  end,
}

return handlers
```

- **Failing a call.** `error("RPC-32602 Title cannot be empty", 0)` fails it with that JSON-RPC code (`RPC`, a whole number,
  a space, the message: no file or line). `ctx:fail(message)` fails it with the application error `-32010` and a message.
  Any other error the handler raises is `-32010` with `<id>/app.luau:<line>: message`, where the line is a line of the bundled `dist/app.luau` (the host reports where the bundle failed, not the source file); raise with `error(message, 0)` or `ctx:fail(message)` for a message without a position. A handler that raises a table or a function (not a string) is reported as `the script raised a value that is not a message`: nothing of the host's memory is ever sent to a client.
- **`ctx`** is `{ actor, now, fail }`: `actor.kind` is `"user"` (it is the person's own client calling), `now` an ISO time.
  It holds what is particular to the call. The host's services are not on it: a handler, a tool and background work (which
  has no `ctx`) all reach them the same way, through `vos:app`: `app.db`, `app.kv`, `app.files`, `app.notify`, `app.log`,
  `app.agents:list()`, `app.emit`. A handler may write: the person's client is calling.
- **Events.** `app.emit(name, data)` tells the app's open clients something changed (they hear it with `host.on`): after
  an agent's tool changed data, or from a background task. `name` is 1 to 64 letters, digits and `. _ : -` (not `host:...`),
  `data` JSON of at most 64 KiB, at most 256 events waiting; a `readOnly` tool may not emit.
- **Calls** from a client, a tool and a task all queue behind one another: one runs at a time in an app's VM.
- **Tools** are separate from handlers (agents call tools over MCP, the client calls handlers):
  `app.tool { name, title, description, readOnly, input, run }` (MCP name `<id>__<name>`, local name `[a-z][a-z0-9_]*`);
  describe every input field with `app.t` builders (agents read them); the host validates input before `run`.
  `readOnly = true` is enforced for the `app.*` API (no db, kv, files, notify, emit or shell writes, no registering
  anything; tasks and requests the call starts inherit it) and lets an agent with the default `auto_read_only` policy
  call the tool without a prompt. It does not cover the `std:`/`vnd:` modules. A tool that writes is not `readOnly`.
  Handlers have no `readOnly`.
- **Background work:** start loops with `task.spawn` from `std:task` in `server/main.luau` (the app is not loaded until it
  returns). Closing a window leaves the work running, `app.emit` from a task reaches the open windows, and unloading
  aborts tasks and closes servers (`crates/vos-host/tests/fixtures/ticker` is an example).
- **Luau limits:** a host call (`app.db:*`, `app.kv:*`) cannot wait inside a `string.gsub` callback, a `table.sort`
  comparator, a metamethod other than `__call`, or an `xpcall` handler. No `print`, `os`, `debug`, `load*`; `bit` is
  `bit32`; `string.format("%s", v)` takes strings and numbers only. The root README lists the language differences.
- **Modules:** `lib:test`, `lib:compile` and `std:workers` are not offered (`vos_domain::NOT_FOR_APPS`); naming one is a
  type error. Everything else in `neoc-vm` is, with no permission to ask for. An app is the person's own code on their
  own host: `CLAUDE.md` ("Boundaries") says what the VM does and does not keep apart.
- **Data:** `db:migrate` steps are numbered and run once each (a changed old step is an error). SQL `ATTACH` is denied
  through `app.db`.

## The scripts and the dev loop

| Script | What it does |
|---|---|
| `bun run generate` | `vos generate`: write `.app/` from the contract |
| `bun run build` | `vite build`: generate, the client script, the backend bundled by darklua, and the app ref the host installs, all in `dist/` (`app-ref.json`, `app.luau`, `client.js`, each with a sha256) |
| `bun run dev` | `vite build --watch`: rebuilds `dist/` when the contract, a Luau file or the client changes |
| `bun run typecheck` | `vos typecheck` (generate, then luau-lsp strict over the Luau against `types/generated/vos.d.luau`) and the client's own type check (`svelte-check` or `tsc`) |
| `bun run lint` | `vos lint`: the same Luau check, and StyLua |
| `bun run format` | `vos format`: StyLua over the Luau (not `.app/`) |

**The dev loop** is two terminals: `bun run dev` in the project, and the real host with the built app installed and
`dev_reload` on, which rebuilds the app's VM (keeping its database) whenever `dist/` changes:

```sh
APP=$PWD                                       # the app's own folder
cd <the repository root> && VOS_DEV_RELOAD=1 VOS_INSTALL="file://$APP/dist/app-ref.json" bun run host
```

So the backend you test is the real Luau on the real runtime. Reopen the app's window to load a new client script. There is
no stand-alone dev page: the shell is the dev host.

The repository's `bun run lint`, `typecheck` and `test` run every project's scripts. luau-lsp 1.70.1, StyLua and darklua
0.18.0 must be installed: `bun run lint` and `typecheck` skip the Luau check of a missing luau-lsp or StyLua on a developer machine and
fail in the gate (`VOS_GATE=1`), but **`bun run build` needs luau-lsp and darklua and stops without them**.
`bun run build:apps` builds every app under `apps/`, a Vite project by running its `build` and copying `dist/`, and writes the
registry. The host installs an app from an app ref URL (`file://`, `http://`, `https://`), verifies every sha256 (the
client's too) and runs the migrations.

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

An app that is a Vite project:

1. **Project.** Copy `e2e/apps/tasks-svelte` (or `tasks-react`) to `apps/<id>` without its `node_modules`, `.app` and `dist`.
   Rename the package in its `package.json` (`"name": "@vos-apps/<id>"`: two workspace packages with one name make
   `bun install` fail), then set `x-app` (id, icon, tint, window), the title and version `0.1.0` in `openrpc.json`. Keep the
   project's `.gitignore` (`.app` and `dist`: the repository's own ignores `dist` but not `.app`). Run `bun install` in the
   repository root so the new package is linked, and `bun run generate` in the project so the editor sees `.app/`.
2. **Define the contract first** in `openrpc.json` (with its `x-app`), then `bun run generate`.
3. **Implement the handlers** in `server/main.luau` (logic in its own module, testable without the client); tools with a described
   schema and a truthful `readOnly`; `app.emit` wherever data changes outside the client.
4. **Client** with `data-testid` on every interactive element, working by keyboard, in light and dark and at the minimum
   window.
5. **Clean.** `bun run lint` and `bun run typecheck` in the project pass, not skipped; `bun run format` fixes formatting.
6. **Tests:** a Rust test of the router and handlers next to `crates/vos-lua/tests/router.rs`; a host test in
   `crates/vos-host/tests` for what needs the data layer or agents; `JOURNEYS.md` and `e2e/journeys.spec.ts` with a test of
   the same id per journey, reached through `page.frameLocator('[data-testid="<id>-frame"]')` (an error case, a minimum
   window, keyboard only, and an agent driving the app where it applies). The generic `desktop` project opens every
   installed app in the real shell (sizes, layouts, light and dark, axe, snapshots).
7. **Try it for real** (`verify-change`), then **design it** with `app-design` until its exit criteria hold, and ship it
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
