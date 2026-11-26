# Unified RPC: one typed contract for pages, apps and agents

Status: plan, not started. Branch `plan/unified-rpc`.

## Proof of concept on this branch

- `packages/vite-plugin`: the plugin of this plan (the v1 plugin is deleted). package.json is the app, `openrpc.json` (1.4) its API;
  it writes types only (below). The sections after this one are the plan as it was written, with its earlier names
  (`sendRequest`, a generated `rpc` per app); what is built is this section.
- `apps/app1` acts (add, clear) and notifies (`added`, `cleared` public; `_saved` private); `apps/app2` depends on it
  (`vos({ dependencies: ['../app1'] })`) and logs what it hears.
- **One RPC bus** (`AppService::request` / `publish`, the `AppBus` port). Every app request and notification, from a
  page or a backend, goes through it. A request reaches every handler registered for its method, the app's backend's
  (`RPC.on_request`, or its entry's table) and its open pages' (`RPC.on_request` in a page, kept per connection by the
  `/rpc` adapter, the `PageHandlers` port); the first answer wins, the others are dropped or told it is settled
  (`app.requestSettled`), and a late answer is refused as expired (`-32013`). With no handler anywhere it is method not
  found. A notification reaches the app's pages, other apps' subscribed pages (public names only) and every listening
  backend; nothing waits for it. No marker in the contract says who serves what: registration does.
- **The same four functions everywhere, in the house style (snake case)**: the global `RPC`, with `RPC.send_request`,
  `RPC.on_request`, `RPC.send_notification` and `RPC.on_notification`, in the page and in the backend (`vos:rpc`). A backend handler is `fn(params, ctx)`, `ctx.actor` saying who asked (the person, an agent, or an app).
- **Ambient, no generated runtime.** The page's whole API is two globals the host's bridge script defines
  (`/apps/bridge.js`): `RPC.send_request(app, method, params)`, `RPC.on_request(method, fn)`,
  `RPC.send_notification(name, params)`, `RPC.on_notification(app, name, fn)`, plus `vos.host`, `vos.tracing` and
  `RPC.RpcError`. The page talks only to its window (`parent.postMessage`, MCP Apps' dialect); the host page puts it on
  the bus. The backend's is the global `RPC` too (beside `app`), with the same four functions and arguments. Every call names its app, its
  own included. Nothing generated runs on either side: the build writes types only, into the app's `types/` (which replaces
  `.app/`):
  `types/vos.d.ts` (the globals `RPC` and `vos`, typed over the `VosApps` and `VosSelf` interfaces) and `types/<id>.d.ts` per app (its own with every method,
  each dependency with its public ones, types under `VosApp.<Id>`), and for the backend `types/vos.d.luau`: a luau-lsp
  definitions file, the host's declarations with the global `RPC`'s functions overloaded by app and method and each
  app's types as globals (`app1_Item`, `app1_AddParams`). The backend requires nothing. Dependencies are
  `vos({ dependencies: ['../app1'] })`, and only decide which apps get types.
- **The host owns the contract.** The app ref carries `openrpc.json` (manifest `contract`); the bus compiles each
  method's params and result into JSON Schema validators (components become `$defs`) and holds every request, answer and
  notification to them, whoever sends it (`crates/vos-application/src/contract.rs`): a method or notification the
  contract does not declare goes nowhere, bad params are `-32602`, a wrong answer `-32603`.
- **Agent tools are the public handlers on the bus**: the host publishes every public method that has a handler on the
  bus (a backend's or an open page's) as the MCP tool
  `<app>__<method in snake case>`, described by the contract (summary, description, params schema, `x-readOnly`), again
  whenever handlers come or go. An agent's tool call is a bus request from that
  agent: the first answer wins, as for any request; a read-only handler of the backend runs with writes refused.
- **Wire**: page → host `app.call`, `app.notify`, `app.handle` / `app.unhandle`, `app.answer`; host → page
  `app.event`, `app.request`, `app.requestSettled`. Frame ↔ shell: `vos/handle`, `vos/answer`, `vos/notify`,
  `vos/serve`, `vos/settled`, `vos/subscribe`, `vos/request` (`client/src/shell/hostBridge.ts`, `bridge.js`).
- Tests: `crates/vos-lua/tests/it/peers.rs` (backends on the bus), `crates/vos-host/tests/it/bus.rs` (pages serve, first
  answer wins, late answer expired, page notification reaches the backend, agent tools from bus handlers), `client/src/shell/hostBridge.test.ts`.
- Demo: App 1's `clear` asks `_client_confirm`, which every open App 1 window serves; App 2 talks to App 1 from its page
  and its backend. A backend listener or handler annotates its `params` (`rpc.App1_AddedParams`): today's luau-lsp
  checks it against the overloads but cannot infer it.

## Why

An app speaks over three unrelated paths today, each typed differently:

| Path | Declared in | Typed? |
| --- | --- | --- |
| page → own backend | `openrpc.json` `methods` → `api.foo(params)` → `app.call` → `rpc.Handlers` | yes, both ends |
| backend → own pages | `app.emit(name, data)` → `host.on(name, fn)` | no (`data: unknown`) |
| agent / other app → backend | `app.tool { name, input = { t.string{...} }, run }` in Luau → MCP `<app>__<tool>`, `app.tools.call` | input only, in Luau, not in the contract |

So the same operation is written twice: Keychain has an `add` method for its page (`apps/keychain/openrpc.json`) and an
`add_keychain_item` tool for agents (`apps/keychain/server/main.luau:52`), with two schemas, two handlers and two
descriptions. Events have no types at all.

## The model

**One contract per app, `openrpc.json`, types everything the app's backend serves and sends.** The four verbs split by
ownership: an app's methods and notifications belong to that app alone.

| Verb | The app (its backend) | Its page, other apps, agents |
| --- | --- | --- |
| `onRequest(method, handler)` | yes, for its own methods only | no |
| `sendNotification(method, params)` | yes, its own declared notifications only | no |
| `sendRequest(method, params) -> result` | to peers | yes |
| `onNotification(method, handler)` | from peers | yes |

No one registers a handler for another app's method, and no one emits a notification in another app's name; the host
enforces both (a request is routed only to the app that declares it, and a notification is accepted only from the
backend of the app that declares it). Notifications flow one way, out of the app; a page that wants to tell its backend
something sends a request (a result of `null` is fine).

**Visibility is in the name, Python-style:**

- `foo` is public: the app's page, other apps and agents may use it. A public request is an agent tool.
- `_foo` is private: meant for the app's own page. Other apps and agents *can* reach it, but it is not in their types or
  tool lists, and the host logs each such call. It is a convention with friction, not a wall.

| Peer | Public `foo` | Private `_foo` |
| --- | --- | --- |
| own page | typed | typed |
| other app | typed (from the peer's contract) | untyped escape hatch, logged |
| agent (MCP) | listed as `<app>__foo` | not listed; callable by name, logged, still subject to grants |

## The app and its contract

What the app *is* comes from its `package.json`: the id is the package name without its scope, plus `version`,
`description`, and under `vos` its `title`, `icon`, `tint` and `window`. `openrpc.json` (OpenRPC 1.4) is the app's
API, and nothing else.

```jsonc
// package.json
{ "name": "@vos-apps/keychain", "version": "0.1.0", "description": "...", "vos": { "title": "Keychain", "icon": "key", "tint": "blue" } }
```

In the contract, a method with a `result` is a request the app serves. A method without one is a notification: OpenRPC
1.4 says "If undefined, the method MUST only be used as a notification" (a JSON-RPC message with no `id` and no
result). OpenRPC says nothing about which side sends what; the ownership rule above settles it: a notification in an
app's contract is one that app sends.

```jsonc
{
  "openrpc": "1.4.0",
  "info": { "title": "Keychain", "version": "0.1.0" },
  "methods": [
    { "name": "overview", "x-readOnly": true, "summary": "...", "params": [], "result": { "name": "overview", "schema": { "$ref": "#/components/schemas/Overview" } } },
    { "name": "add", "summary": "Keep a password in the keychain", "description": "...agent-facing text...", "params": [...], "result": {...} },
    { "name": "_reveal", "params": [...], "result": {...} },          // page only
    { "name": "changed", "params": [] },                              // notification: no result
    { "name": "_tick", "params": [{ "name": "count", "schema": { "$ref": "#/components/schemas/Count" } }] }
  ]
}
```

Rules the generator enforces (`packages/vite-plugin/src/contract.ts`):

- `params` is required, as OpenRPC says (`[]` for none); params are by name.
- `x-readOnly: true` on a request replaces `app.tool { readOnly = true }`: MCP `readOnlyHint`, and the runtime runs it
  read-only when an agent calls it. It is refused on a notification.
- `summary` is the tool title, `description` the tool description. A public request `addSite` is the tool `add_site`.
- `foo` and `_foo` may not both exist: one name is public or private, not both.
- A public request whose params a tool cannot hold (a recursive schema, a null) is a generate error that says to make
  it private or change the schema.

## The API, per side

The generated modules export a typed `rpc` object. Types are a method-keyed map, so autocomplete on the method string
does the work the old `api.foo()` proxy did, and there is one way to call.

### Page (TypeScript, `@<app id>/rpc`)

```ts
import { rpc, type Overview } from '@keychain/rpc'

const overview = await rpc.sendRequest('overview')                // Promise<Overview>
const secret = await rpc.sendRequest('_reveal', { item })         // private: fine from the own page
const stop = rpc.onNotification('changed', () => refresh())      // params typed per notification
```

Generated types:

```ts
export interface Requests { overview: { params: void; result: Overview }; add: { params: AddParams; result: Overview }; _reveal: {...} }
export interface Notifications { changed: { params: void }; _tick: { params: TickParams } }

sendRequest<M extends keyof Requests>(method: M, ...params: ParamsArg<Requests[M]['params']>): Promise<Requests[M]['result']>
```

The page's `rpc` has `sendRequest` and `onNotification` only. `host` keeps the non-RPC surface (`theme`, `context`, `open_link`, `on_teardown`, `host:reconnected`). `api` and
`host.on` go.

### Backend (Luau, the global `RPC`; types in `types/vos.d.luau`)

The backend uses the globals the host's VM gives it and requires nothing. The build types those globals: it writes
`types/vos.d.luau`, the host's own declarations (`types/generated/vos.d.luau`) with

- each known app's types as global types under its id, as the host names its own (`vos_app_Context`): `app1_Item`,
  `app1_AddParams`, `app1_AddResult`. The app's own every method, a dependency's its public ones;
- `RPC.send_request`, `RPC.on_request`, `RPC.send_notification` and `RPC.on_notification` overloaded by app id and method,
  so a wrong app, method, params or result is a type error.

```luau
RPC.on_request("add", function(params: app1_AddParams, ctx: vos_app_Context): app1_AddResult
  local item: app1_Item = { id = nextId, text = params.text, by = by(ctx) }
  RPC.send_notification("added", { item = item })
  return item
end)
```

Check it with `luau-lsp analyze --definitions:@vos=types/vos.d.luau server/main.luau` (an editor points luau-lsp's
definitions at the same file). Handlers are optional: a method nobody serves is method not found on the bus.

- One handler serves every caller; `ctx.actor` says who (the page's person, `{ kind = "app", id = "app2" }`, an agent),
  so a handler can refuse a kind it does not want.
- The bus checks params, results and notifications against the installed contract whatever the types say.
- Private notifications reach only the app's own pages and backend; public ones also reach other apps (backends and
  pages) that listen with `RPC.on_notification`.

### Other apps (inter-app)

An app lists the apps it talks to as `dependencies` of `vos()`, by where they are, as `bun add ../my-lib` does:

```ts
// vite.config.ts
vos({ dependencies: ['../ledger', '../notes'] })
```

- Now: each entry is a path relative to the project. A folder means `<folder>/openrpc.json`; a file is the contract
  itself. One that is not there is a generate error naming it.
- Later, without changing the shape: other schemes such as `github:owner/repo/path`.

The generated client, on the page and in the backend alike, then knows the app's own API and each dependency's, under
the dependency's app id (from its `package.json`, the id the host routes by):

```ts
// page
import { rpc as ledger } from '@ledger/rpc'
const total = await ledger.sendRequest('add', { amount: 3 })
const stop = ledger.onNotification('changed', () => refresh())
```

```luau
-- backend: globals only, typed by types/vos.d.luau
local total, err = RPC.send_request("ledger", "add", { amount = 3 })
RPC.on_notification("ledger", "changed", function(params: ledger_ChangedParams, ctx: vos_app_Context) ... end)
RPC.send_request("ledger", "_audit" :: any, {})           -- private: not typed, the host logs it
```

- From another app's contract only the public part is typed: its `foo` requests and `foo` notifications.
- Another app has `sendRequest` and `onNotification` only: an app cannot serve ledger's methods or emit ledger's
  notifications.
- Dependencies are an option of the generator, not part of the contract, and nothing is fetched from a host.
- An app not running at call time is an error the caller reads, as `app.tools.call` is today.

Replaces `app.tools.call` / `app.tools.all`; the Agents app's "offer every tool to my agent" keeps a dynamic, untyped
`rpc.tools()` listing of public requests across apps.

### Agents (MCP)

Every public request of a running app is a tool `<app>__<method>`, its input schema from the method params, title and
description from the contract, `readOnlyHint` from `x-readOnly`. `app.tool {}` goes. Private requests are absent from
`tools/list`; `tools/call` on one works if the agent's grants cover it and is logged. Notifications are not exposed to
agents in v1.

## Wire

Page ↔ host socket (`crates/vos-http/src/hub.rs`, `rpc.rs`):

- `app.call {app, method, args}` → `app.request {app, method, params}` (cancellable, as now via `notifications/cancelled`)
- `app.emit` events → `app.notification {app, method, params}` (host → page)

Inside the host, `AppService::call_handler` and `AppTools::call` fold into one
`dispatch(app, caller: Caller, method, params)`; the visibility check (private + non-page caller ⇒ log) and read-only
check live there once. Outbound notifications go through one `publish(app, method, params)` that only the app's own
runtime can reach: there is no socket message or peer call by which a page, another app or an agent sends one. The app ref carries the full contract, not only the manifest it carries today, so the host derives tools and
validates without running app code.

## Plan

Pre-release, so no compatibility shims: each step lands with the old path removed and the e2e journeys green.

1. **Retire the `vos` CLI first** (see "Retiring the `vos` CLI"): generation in `configResolved`, `lint:luau` over
   `apps/`, kit types from `@vos/kit`, the `vos-lua` TypeScript fixture off `vos bundle`, app scripts trimmed, `bin`,
   `luau-tools.ts`, `find-tool.ts` and the `check` option deleted. Lands on its own with the gate green, so the RPC
   steps build on the plugin alone.
2. **Spike Luau typing of the sending verbs.** Handlers are a plain typed table, so serving needs no trick. Sending
   does: check `luau-lsp --strict` (the strict-Luau check in `neoc-vm`) types `rpc.sendNotification("changed", params)`
   and `ledger.sendRequest("add", params)` through an intersection of singleton-string overloads, and gives a readable
   error for a wrong method or param. If it does not, the fallback is a generated typed table of senders, still no
   per-method magic on the page side. Check TypeScriptToLua emits it cleanly. (~half a day)
3. **Contract**: notifications (methods without a result), `x-readOnly`, visibility rules and the
   `foo`/`_foo` clash in `definition.ts`, with tests in `definition.test.ts`. Ship the full contract in the app ref.
4. **Generators**: `emit-client.ts` (page `rpc`), `emit-luau.ts` + `luau-runtime.ts` (backend `Handlers` over its
   requests, `rpc.serve`, typed `sendNotification`), `typescript-backend.ts` (`entry.ts` still makes a
   missing handler a type error). Update `generate.test.ts`,
   `typescript-backend.test.ts`.
5. **Host**: `dispatch` with `Caller`, `app.request` / `app.notification` on the socket, `publish` reachable only from the app's runtime, bridge.js,
   outbound notifications replacing `app.emit` (keep its size and queue limits), visibility logging.
6. **Agents**: derive MCP tools from the contract in `vos-application` (`publish`), drop `app.tool` and `ToolDecl` from
   `vos-lua`, regenerate `types/generated` (`bun run codegen`).
7. **Inter-app**: a `dependencies` option of `vos()` (in `Options`). Generation is the Vite plugin's alone and reads
   `dependencies` from `vos()`; resolution of each entry (relative folder or file, missing is an error) with its own tests;
   the public part of each dependency under its app id; the global `vos` on the page and `types/<id>` in the backend, listening to other apps' public notifications, drop `app.tools.*`; Agents app
   moves to `rpc.tools()`.
8. **Migrate apps** one at a time (calculator, containers, dns, keychain, services, processes, ..., e2e fixtures ticker,
   notifier, memos, tasks-*): fold each `app.tool` into a public method, rename page-only methods to `_foo`, turn
   `app.emit` into declared notifications. Keychain is the first, as the duplication example.
8. `cargo test -p neoc-vm`, `bun run codegen`, `bun run gate`, e2e.

## Migrating a v1 app

Done: v1 (`@vos/vite-plugin` as it was, with its generated `@<id>/rpc`, `@vos/host` and `@vos/tracing` modules and the
`vos` CLI) is deleted and the plugin of this plan took its name (`packages/vite-plugin`). The recipe for moving an app (package.json
and contract, backend, page) lived here; it is in this file at commit `3c47b172`, with Keychain (`apps/keychain`,
commit `26724ea6`) as the worked example.

## Retiring the `vos` CLI

Done: the `vos` bin (`bin/vos.ts`, `luau-tools.ts`) went with v1. vos is the Vite plugin, and the plugin's job is to
generate the Luau and TypeScript client code and build the app ref. What each command became:

| Command | Now |
| --- | --- |
| `vos generate` | the plugin, on any Vite run (`configResolved`); `packages/docker` writes the kit's types with `writeKitTypes` |
| `vos typecheck`, Luau backend | `bun run lint:luau` (`scripts/lint-luau.ts`), over `apps/*/server` and `e2e/apps/*/server`, each with its generated `types/vos.d.luau` |
| `vos typecheck`, TypeScript backend | `vite build`: compiling the backend with TypeScriptToLua is the check |
| `vos lint`, `vos format` | `bun run lint:luau`, `bun run lint:stylua` and the root `bun run format` |
| `vos bundle <out>` | the app's own `vite build` (`dist/app.luau`); the TypeScript fixture of `crates/vos-lua` is a small app project built that way |

Linting, formatting and checking Luau are the individual tools' (luau-lsp, StyLua), run by the repo's scripts; a Luau
toolchain is neoc's to grow into later, not the plugin's. What stays in the plugin, as build output: bundling the Luau
backend into `app.luau` (darklua) and compiling a TypeScript backend (TypeScriptToLua). `vite build` does not type check a
Luau backend.

## Open questions

None. (Backend → page requests are out by the ownership rule: only an app serves its own methods, so a page serves
none.)
