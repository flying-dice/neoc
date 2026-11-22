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
  index.html              the client's page (a normal Vite app: <div id="root"> and a module script)
  vite.config.ts          plugins: [<your framework>(), vos()]
  package.json            scripts: generate, build, dev, typecheck, lint, format (a Bun workspace package)
  src/main.ts             the client: starts itself, in the page's root element
  server/main.luau        the backend: one handler per method of the contract; tools, data, background work
  server/*.luau           modules the backend loads with require("./thing")
  .app/                   GENERATED (not committed): rpc.d.ts, rpc.js, rpc.luau, entry.luau
  dist/                   BUILT (not committed): static/ (the client), app.luau, app-ref.json
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
export default defineConfig({ plugins: [svelte({ configFile: false }), vos()] })
```

The project's `tsconfig.json` includes `".app/rpc.d.ts"` (the types of `app:rpc`; TypeScript does not pick up a dot folder
by itself).

`vos()` takes optional `{ definition, generated, backend }` (defaults `openrpc.json`, `.app`, `server/main.luau`). Vue needs `isCustomElement: (tag) => tag.startsWith('ui-')` in its plugin options and Angular
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
(`A` is `B` is `A`) is refused with the cycle named. Migrations are `x-app.migrations`, numbered 1, 2, 3, ... in the order they run; each `file` is a plain relative name inside the
project (no `..`, no absolute path, no hidden segment; a link must lead to a file of the project), no file or version is given
twice, and two files may not share a name. The build reads each from the project and writes it to `dist/migrations/<its name>`,
which nothing the build generates can share, and the app ref lists it there. Every type the generated files declare must have a name
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
it could not say the app will load. `vos({ check: false })` turns the check off, for someone who knows what they are doing (the build then says in one line that nothing
verifies the handlers, every time; the host still refuses to load an app that does not implement every method). A build that fails at any step leaves `dist/` exactly as the
last good build made it: the build is made in a folder of its own, `.app/build-XXXXXX`, and moved into `dist/` only when every step has succeeded.

## The client

The client is **a normal Vite app**: an `index.html` at the project root (a root element and a module script), your framework,
your styles and assets, code splitting as usual. There is no `mount` function and no generated entry: the page starts itself.
`vite build` writes it to `dist/static/` with relative URLs, and the plugin injects one tag into `index.html` at build time, the
host's bridge script (`/apps/bridge.js`, first in the head: the host serves one copy, the app never bundles it). Everything in
`dist/static/` is listed in the app ref with its sha256.

```html
<!-- index.html -->
<div id="root"></div>
<script type="module" src="/src/main.ts"></script>
```

```ts
// src/main.ts: a normal Vite entry
import { mount } from 'svelte'
import App from './App.svelte'

mount(App, { target: document.getElementById('root')! })
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
| `host.on(name, fn)` | hears an event the backend sent with `app.emit(name, data)`, or one of the host's own, whose names start with `host:` (`host:reconnected`: the connection to the host dropped and came back, so fetch again: a change made meanwhile reached nobody); returns a function that stops listening |
| `host.theme`, `host.context` | the appearance (`"light"` or `"dark"`) and the host context (MCP Apps' `hostContext`: `theme`, `displayMode`, `availableDisplayModes`, `locale`, `timeZone`, ...). `host.onContext(fn)` hears a change. The bridge sets `<html data-theme>` and `color-scheme` before first paint and keeps them in step, so style dark as `html[data-theme="dark"]` |
| `host.ready`, `host.capabilities`, `host.hosted` | the handshake is done; what the host offers (MCP Apps' `hostCapabilities`); and whether the host page draws UI of its own (true in the shell, false on the app's own page) |
| `host.openLink(url)`, `host.requestDisplayMode(mode)` | `ui/open-link` (http and https only) and `ui/request-display-mode` (answers with the mode the host ended in) |
| `host.onTeardown(fn)` | `fn` runs when the host is about to remove the page (`ui/resource-teardown`), for an app that must wrap up; an app that ignores it simply has its frame removed |

**What the frame is.** An app is served as static files at `/apps/<id>/static/` and runs in an `<iframe sandbox="allow-scripts allow-forms">`
of a host page. Every file is sent with the host's headers: a content security policy under which the page makes no request of
its own (`fetch`, WebSocket, and scripts, images and fonts from anywhere but the host fail, unless the app declared an origin
for images, media, fonts or styles: see below), `nosniff`, and a content type from a fixed table by extension (only the entry
page is ever a document). The page has no access to the host page, its cookie or storage (`localStorage`, `document.cookie`
and `parent.document` throw), and one way out: `api` reaches **this app's** backend through the host page, and the host knows
which app is calling from the frame the message came from, never from anything the client says. The sandbox also stops it
opening windows (`window.open` returns `null`) and navigating the host page (`window.top.location` throws). **Forms work**
(the sandbox has `allow-forms`): Enter in a field, a submit button, the `submit` event, validation and `FormData` behave as in
any page, and your framework's `onSubmit` runs. A submission that nothing prevented is prevented for you, after your handlers
have run, so a form whose handler forgot `preventDefault()` stays on its page (a `<form method="dialog">` still closes its
dialog). Two things still navigate the frame, which the policy's `form-action 'none'` refuses to send data for but cannot keep
on the page: `form.submit()` from script (it fires no event) and a form inside a shadow root of your own (`submit` does not
cross it). What the policy cannot stop is the frame navigating **itself** (`location.href = ...`): the window then shows a page
the shell did not load, says the app left its page and offers to open it again. This is sensible isolation, so apps do not trip
over each other or over the host; it is not a security boundary. Lasting data belongs in the backend (the frame has no
storage). **An app's static files are public** (a sandboxed page sends no cookie, so the host cannot ask for a session): don't
put secrets or a person's data in them. They are kept in the host's memory while the app is installed.

**Limits of a call.** An `api` call's arguments may be at most 1,000,000 bytes of JSON and 100 levels deep, and a method name is 1 to
128 characters: a call over a limit rejects at once with a message that says which. A result may be at most 4 MiB, and a
larger one comes back as an error. A call the host page cannot read is rejected too, so a promise never hangs. Calls in
flight are limited so that one window can neither be refused by the host's own connection limit for an ordinary burst (a
call per item of a list) nor crowd out the shell or another app: 8 at a time per app and 32 across all apps, the
rest wait their turn (apps take turns); an app with 1,000 calls waiting gets an error for the next one. A place is taken for
as long as the host is running the call: a call has no time limit on the client, and its place comes back when the handler
answers or the connection to the host goes, so a handler that never returns takes up to eight places of its own app and no
one else's. Closing the window, uninstalling the app or replacing it ends the calls the host is running for it (the host is told
to cancel them, with MCP's `notifications/cancelled`; only an `app.call` is cancellable, every other host request, an install
or an uninstall included, runs to completion whatever the connection does), and gives their places back. The shell's queue is a courtesy; the limit
itself is the host's: an app runs at most 8 handler calls at once, whoever sends them over however many connections, and the
ninth is answered at once with `-32000` (busy) and the reason. The shell's own calls always keep working, and so do the other apps while fewer than four apps have hung handlers: the shell's
32 places are shared by all apps, so when four apps hold eight each, a fifth app's calls wait until a place comes back.

A call has no time limit on the client, so a handler that waits on something unreliable (a long poll, a service on the LAN that
may be down) should bound the wait itself: calls to an app whose handlers never return queue behind its hung ones, up to eight
at a time, and every call after them waits too.

**What a client can rely on about its page.** Its page, and everything it holds in it (fields, scroll positions, in-memory
state), lives as long as its window does: opening another app, switching stages or minimising does not reload it (the window
is hidden, not removed, and takes no focus, pointer events or screen reader attention while it is), and closing the window
ends it. It is not told when it is hidden or shown, and nothing suspends or reclaims it while hidden (with many windows open every
app's page stays alive). Anything that must outlive the window belongs in the backend.

**When a page fails to start**, the window shows the reason as text with a "Try again" button: the error the page raised while
it loaded (a top-level throw, a missing import, a syntax error: its message and where, as the browser reports it for a script
loaded with `crossorigin` from the host; a script it reports as `Script error.` is named as failing, with the console as the
place to look), or one the host did not accept.

**What the page may load.** Nothing but its own files, unless it says so in `x-app`, in the shape of MCP Apps' `_meta.ui`:

```json
"x-app": {
  "id": "feeds",
  "csp": { "resourceDomains": ["https://cdn.example.com", "https://*.images.example.org"] },
  "permissions": { "camera": {} }
}
```

`csp.resourceDomains` lets images, media, fonts and styles come from those origins (real hosts such as `https://cdn.example.com`
or one domain's subdomains such as `https://*.example.com`; a bare scheme, a `*` or a wildcard over a top-level domain is refused
at install). **Scripts never come from elsewhere**: an app's code comes only from the host. `connectDomains`, `frameDomains`
and `baseUriDomains` are accepted and **not applied**: every call an app makes goes through the host to its own backend, which
can make web requests itself. `permissions` (`camera`, `microphone`, `geolocation`, `clipboardWrite`) go into the iframe's
`allow` attribute; a browser may still ask the person. This is a declaration the host applies, readable here; it is not a consent
prompt or a permission system.

**Two ways to run, one bridge.** The same page runs **in the shell** (a window of the shell holds the frame) and **on its own**:
`GET /apps/<id>` is a host page with one full-viewport iframe holding the same app, and nothing else drawn (it is not the shell).
Either host page speaks one protocol to the frame: JSON-RPC 2.0 over `postMessage`, a dialect of
[MCP Apps](https://modelcontextprotocol.io/extensions/apps) (`ui/initialize`, `ui/notifications/host-context-changed`,
`ui/request-display-mode`, `ui/open-link`, `ui/resource-teardown`; vos's own names are under `vos/`; your own methods travel as
plain JSON-RPC and the host forwards them to your backend). Every call a page makes goes through the host page, never directly to
a backend, and both host pages offer the same host services, from the same code: your backend calls and events, the host context
(drawn from the system settings), links and display modes. What only the shell draws or arranges (window chrome, sheets, alerts,
toasts: NEOC-252) is absent on the app's own page, and the handshake says so: check `host.hosted` before showing a control that
needs it, and a call the host does not offer fails with `This host does not offer <method>`. A path or query after `/apps/<id>`
is not passed to the app yet.

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
| `bun run build` | `vite build`: generate, the client (a normal Vite build into `dist/static/`), the backend bundled by darklua, and the app ref the host installs, all in `dist/` (`app-ref.json` lists `app.luau` and every static file, each with a sha256) |
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

So the backend you test is the real Luau on the real runtime. Open the app on its own page, `/apps/<id>` on the host (paired
device), to work on it without the shell, and reload the page to load a new client; in the shell, reopen the app's window.
There is no `vite dev` server: the host is the dev host, so what you run is what is installed.

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
  is not connected never gets it (nothing is queued). The shell of each client draws it as a banner above its windows,
  with the app's name and icon from the shell's own app list: it is not drawn on a client where one of the app's windows
  is in front, and is drawn on every other client, whether or not the app has a window open. The person can dismiss it
  or choose it to open the app's main window; it goes by itself after ten seconds of the page being on screen (held while the keyboard is on it or the pointer is
  being moved over it), and at most three show at once (a fourth replaces the same app's oldest first, so one app's
  burst does not push another's off; a tab in the background does not count as the app being in front, so it is shown).
  A page with no shell (the app opened on its own at `/apps/<id>`) draws none: the banner is the shell's, and the call is
  the same either way. The title is shown on one line (line breaks and control characters become spaces; direction overrides and
  characters that draw nothing are removed, except the joiners and variation selectors that make an emoji or a word,
  such as a family emoji, the flag of England or a Persian word) and the body as lines of text, never as markup; a
  title with nothing visible left shows the app's name. A screen reader hears the sender, the title and the start of
  the body. The journeys are in `client/journeys/notifications.md`. Bad input raises, each message
  starting `app.notify`: no table (`takes a table`), a key other than `title` and `body` (`unknown key`, or `unknown positional entry` for `app.notify {
  "text" }`), no title (`needs`), a title that is blank or too long, a body that is too long, a value that is not a
  string, and a call in `render` or a `readOnly` tool. A call may also have at most 64 notifications waiting to be
  delivered at once (`already has 64 notifications waiting`). The host delivers waiting notifications whenever a call of
  the app ends, and when a call waits (at `task.wait` or a request; a task or request handler pausing; a tool that has
  notified and then works on), so a tool that notifies half way is heard then, not when it returns. The limit guards
  against a runaway loop; it is not a quota to plan around. A client whose connection stalls without closing (a sleeping
  tablet) is dropped by the host, either once more than 32 MiB is waiting to be sent to it or when its socket takes no
  message for 30 s (plus the time the message needs at 16 KiB/s), and reconnects; it does not get what was raised while
  it was away, like any client that was not connected. `at` and `actions` are type errors too.
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

## Web requests

An app calls the web with `vnd:hyper` (`require("vnd:hyper")`, nothing to ask for; the declarations are in
`types/generated/vos.d.luau`). A request is safe to make with no options: it does not hang, does not fill the host's
memory, and follows a moved URL without handing out your credentials.

```lua
local hyper = require("vnd:hyper")
local resp, err = hyper.get("https://example.com/feed.xml", { headers = { Accept = "application/xml" } })
if not resp then
  if hyper.error_kind(err) == "timed_out" then --[[ say "the server is not answering" ]] end
  return
end
print(resp.status, resp.headers["content-type"], resp.body, resp.url) -- url: where it ended, after redirects
hyper.post { url = "https://example.com/api", body = '{"a":1}', headers = { ["Content-Type"] = "application/json" } }
hyper.request { method = "DELETE", url = "https://example.com/items/1" }
```

- **A 4xx or 5xx is a response, not an error.** An error is `nil, "hyper.<function>: <kind>: <message>"`, and
  `hyper.error_kind(err)` returns the kind to branch on: `invalid_input` (an option, header or body refused before anything
  was sent), `invalid_url`, `connect` (no connection), `tls` (usually the certificate), `timed_out`, `too_many_redirects`,
  `too_large`, `connection_lost` (the connection ended before the response did), `bad_response` (not valid HTTP) and `other`.
  The message is for a log; do not parse it.
- **Time.** A request gives up after 10 s of connecting (`connect_timeout`, the TLS handshake included) and after 60 s
  with no data moving in either direction (`idle_timeout`; reset whenever any bytes move, so a long download is fine).
  A server that sends one byte every 59 s never trips the idle timeout: set `timeout` (seconds, no default) to bound the
  whole request, redirects and body included. A call that waits does not stop the app's other calls.
- **Size.** The body is read up to 64 MiB (`max_body`, in bytes): a declared length over it fails at once, and a body that
  grows past it is cut off and the connection dropped (`too_large`). The module never decompresses: it sends no
  `Accept-Encoding` and returns the bytes as the server sent them, so a small response cannot expand past the cap. If you
  decode a body yourself (`lib:compress`), you bound the decoded size.
- **Redirects.** 301, 302, 303, 307 and 308 with a `Location` are followed, 16 at most (`follow_redirects = <n>` changes
  the limit, `false` returns the 3xx as it is). A 303 becomes a GET without a body, a POST becomes a GET on 301 and
  302, and 307 and 308 keep the method and body (after Swift's async-http-client). A hop to another origin (another
  scheme, host or port) drops `Authorization`, `Cookie`, `Proxy-Authorization`, `Origin` and a `Host` you set. A relative
  `Location` is resolved against the current URL; one that is not `http` or `https` is `invalid_url`.
- **Only those headers are dropped.** A credential in a header of your own (`X-Api-Key`, a token in `X-Auth`) is sent to
  every hop, whatever its origin, as in the reference clients. If a redirect could take it somewhere it must not go,
  pass `follow_redirects = false`, look at `resp.headers.location` and follow it yourself.
- **Where a redirect goes is not checked, and there is no permission to check it.** A public URL can redirect to
  `http://192.168.1.1/`, to `http://localhost:7400/` (the host itself) or from `https` to `http`. If your app fetches a URL
  someone else chose, look at `resp.url` and decide, or pass `follow_redirects = false` and follow the `Location` yourself.
- **Response headers.** `resp.headers` has one value per lower-case name. A header the server sent more than once is
  one value joined with `", "`, as Fetch's `Headers.get` does, except `set-cookie`, whose values are joined with `"\n"`
  (a cookie may contain a comma; a header value never contains a line feed), so `for cookie in
  resp.headers["set-cookie"]:gmatch("[^\n]+") do ... end` gives each one. Bytes that are not UTF-8 become U+FFFD. A
  `vnd:hyper` server's `req.headers` follows the same rules.
- **A body with no length.** A response that has neither `Content-Length` nor chunked encoding ends when the server
  closes the connection, so one cut short cannot be told from a complete one (that is HTTP/1.1, not this module). Any
  other cut-off body is an error (`connection_lost`, `timed_out` or `too_large`), never a short body.
- **The request is what you asked for.** A `Content-Length` header that is not the body's length is `invalid_input` (leave it
  out: the module sets it), and so are a header name given twice in different case (`Host` and `host`) and a
  `Transfer-Encoding` header (the module frames the body), and so is a `headers` key that is not a string (`{ "Accept:
  text/plain" }`). A `Host` header replaces the built-in one. A standard method
  in any case is the standard method (`put` is `PUT`); any other is sent as written. Credentials written in the URL
  (`https://user:pass@host/`) are not sent: set an `Authorization` header.
- **Turning a limit off.** The time and size options take positive finite numbers (anything else is `invalid_input`,
  naming the option); a very large one, such as `idle_timeout = 1e9` or `max_body = 2^53`, is in effect no limit.
- **Your own certificate authority.** `ca_pem = "<PEM>"` trusts exactly those certificates, on every hop; without it the
  system's trust store decides. It is checked before anything is sent, on an `http://` URL too: a value that is not a
  string of usable PEM certificates is `invalid_input`.

## Adding an app

**What you need installed first:** Bun, Rust (the host), **darklua 0.18.0** (`cargo install darklua --version 0.18.0 --locked`: bundles the Luau),
**luau-lsp 1.70.1** (the release binary on the PATH or in `~/.local/bin`, or named by `VOS_LUAU_LSP`: the build checks the backend with it and
stops without it unless you set `check: false`) and StyLua (`cargo install stylua --features luau`: `bun run lint`).

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
