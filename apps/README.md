# Building AgentOS apps

AgentOS is two programs and a set of apps:

- **`apps/server`**: the Bun server. It runs app server modules, the tool registry, the MCP endpoint and the ACP agents, and serves the client and the app bundles.
- **`apps/desktop`**: the client, an installable PWA (Vite + Svelte). It's the iPadOS-style shell: windows, Dock and Stage Manager.
- **`apps/<id>`**: **app modules**. Each is built into a bundle, released into this repo's `bundles/` and installed onto a server at runtime, if the server knows it.

```
 PWA client (browser)                           Bun server
 ┌─────────────────────────────┐  JSON-RPC 2.0  ┌────────────────────────────────────┐
 │ shell · windows · Dock      │ ◀─ WebSocket ─▶│ AppRuntime: register / unregister  │
 │ app client.js (ESM)         │     /rpc       │ app server.js (ESM, Bun)           │
 │  Svelte · kit · SDK shared  │                │ ToolRegistry ─▶ /mcp ◀── ACP agents │
 └─────────────────────────────┘                │ SystemDb (bun:sqlite)              │
        ▲ /bundles/<id>/<version>/client.js     │ KnownApps ◀── known-apps.json      │
        └───────────────────────────────────────┴────────────────────────────────────┘
```

Everything an app touches comes from **`@agentos/sdk`**: there is no other contract.

## An app bundle

Each app builds into a directory of plain static files:

```
<id>/bundle.json                    the latest version: { manifest, server: {url, sha256}, client: {url, sha256} }
<id>/<version>/bundle.json          that version (file URLs relative to it)
<id>/<version>/server.js            ESM for Bun: everything bundled; bun:*, node:* and npm deps allowed
<id>/<version>/client.js            browser ESM: Svelte, @agentos/ui and the SDK client come from the host
```

Two build targets (`scripts/build-apps.ts`, schema in `packages/sdk/src/bundle.ts`):

| Command | Writes | For |
|---|---|---|
| `bun run build:apps [ids…]` | `dist/bundles/` (gitignored) | development: mutable, the dev server hot-reloads it |
| `bun run release:apps [ids…]` | `bundles/` (committed) | shipping: immutable, adds `<id>/bundle.json` to `apps/server/known-apps.json` if missing |

- **Client bundles don't ship their own Svelte or kit.** The build maps those imports to the host's shared instances (`SHARED_MODULES` in `@agentos/sdk/build`), so there's one Svelte runtime, one kit and one set of styles. A client bundle is typically tens of kB.
- **Servers verify each file's sha256** against its `bundle.json` before installing it.
- **Builds are byte-deterministic**: the same source gives the same sha256s, so re-running `release:apps` on an unchanged app changes nothing.
- **Released versions are immutable.** `release:apps` refuses to overwrite `bundles/<id>/<version>/` with different bytes (`PublishedVersionError`): bump `version` in `src/manifest.ts` to release a change.
- **Bundles are stored on the server per version and hash**, so an update never reuses a stale module.

## Make an app

### 1. Scaffold

Copy `apps/calculator` to `apps/<id>`. Rename the package to `@agentos/app-<id>`, then edit `src/manifest.ts`:

```ts
import { defineManifest } from '@agentos/sdk'

export default defineManifest({
  id: 'notes', // lowercase, digits, dashes; namespaces RPC, tools and storage
  version: '0.1.0', // semver; bump it to ship an update
  name: 'Notes',
  icon: 'doc', // an IconName from @agentos/ui
  background: 'linear-gradient(180deg, #ffd60a, #ff9f0a)',
  window: { width: 720, height: 520, minWidth: 360, minHeight: 320 }, // say why if min > 320x240
  dock: true
})
```

Layout (the build and the checks rely on it):

```
src/manifest.ts        plain data
src/server/index.ts    default export defineServerApp(...)    runs in Bun
src/client/index.ts    default export defineClientApp(...)    runs in the browser
src/client/{atoms,molecules,organisms,pages}/   atomic layers (bun run lint:atomic)
src/shared/            pure logic both sides use, with bun tests
src/mock/server.ts     optional: a browser-safe stand-in for web mode and stories
```

### 2. The server module

```ts
import { join } from 'node:path'
import { Database } from 'bun:sqlite'
import { z } from 'zod'
import { defineServerApp } from '@agentos/sdk/app/server'

export default defineServerApp((ctx) => {
  // ctx.dataDir is yours and survives updates. Open what you need here.
  const db = new Database(join(ctx.dataDir!, 'notes.sqlite'), { create: true, strict: true })
  db.exec('CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)')

  // Client → server calls. Payloads are untrusted: parse them.
  ctx.handle('add', (payload) => {
    const body = z.string().min(1).max(10_000).parse(payload)
    db.query('INSERT INTO notes (body) VALUES (?)').run(body)
    ctx.emit('changed') // → every open client of this app
  })

  // Tools: agents call these over MCP, other apps' clients via api.tools.call.
  ctx.registerTool({
    name: 'list_notes',
    description: 'List the most recent notes.',
    input: { limit: z.number().int().min(1).max(100).default(20) },
    readOnly: true, // agents may call read-only tools without asking
    run: ({ limit }) => ({ notes: db.query('SELECT * FROM notes ORDER BY id DESC LIMIT ?').all(limit) })
  })

  // Runs on uninstall, update and server shutdown. Release EVERYTHING you opened.
  return () => db.close()
})
```

**Runtime registration rules.** Apps are registered and unregistered while the server keeps running:

- **On unregister**, the host aborts `ctx.signal`, awaits your dispose function, then drops your handlers and tools. Timers, intervals, watchers, sockets and child processes are your job: clear them in dispose, or listen to `ctx.signal`.
- **Updates** unregister the old version before registering the new one. Leaked resources keep running beside the new code, so release them.
- **`ctx.emit` and `ctx.openWindow` are ignored** once the app is unregistered.
- **A throwing `setup` marks the app "failed"** and leaves everything else running. Clients show the error.
- **Small key/value state** goes in `ctx.storage`; it lives in the server's database, scoped to your app. For anything relational, use your own SQLite file in `ctx.dataDir`.

### 3. The client

```ts
// src/client/index.ts
import { defineClientApp } from '@agentos/sdk/app/client'
import NotesPage from './pages/NotesPage.svelte'

export default defineClientApp({ component: NotesPage })
```

```svelte
<!-- src/client/pages/NotesPage.svelte -->
<script lang="ts">
  import { useAppApi } from '@agentos/sdk/app/client'
  import { Button } from '@agentos/ui/atoms'

  const api = useAppApi() // scoped to this app
  $effect(() => api.on('changed', () => void refresh())) // returns the unsubscribe
  async function add(): Promise<void> {
    await api.invoke('add', 'Hello')
  }
</script>

<Button testid="notes-add" onclick={add}>Add</Button>
```

- **The client API:** `api.invoke`, `api.on`, `api.tools.list` / `api.tools.call` (any app's tools) and `api.host.openApp`.
- **Client code may import only** `@agentos/sdk` (root and `app/client`), `@agentos/ui`, `svelte` and `zod`. They're shared with the host. Anything else is an architecture change.
- **Use the kit** (`@agentos/ui`) and follow `docs/design/DESIGN_LANGUAGE.md`. Every interactive element gets a `testid` (`docs/TESTING.md`).

### 4. Try it

```sh
bun run dev:web            # http://localhost:6200/web.html?open=<id>
                           # the shell on the in-memory host; your app from source
bun run dev                # the real thing: builds dist/bundles, starts the server on :7400 and Vite on :5173
                           # open http://localhost:5173 → Discover → Apps → GET
bun run build:apps <id>    # after changing the app: the dev server hot-reloads it (unregister → register)
bun run release:apps <id>  # to ship it: bump `version` first (see "Ship it")
```

- **Web mode** finds every `apps/*/src/manifest.ts` by itself. If your server module needs Bun (SQLite, files, processes), add `src/mock/server.ts` with the same handlers and tools, backed by memory, so web mode and Storybook can run it.
- **`bun run dev`** stores data in `~/.agentos` (`AGENTOS_DATA` overrides it). `<data>/mcp.json` lets other local MCP clients use the tools too.

### 5. Ship it

A new or changed app reaches `main` only through a PR, with the full gate and the readiness checks in `agentos-code-review`:
- stories for every screen and state;
- `JOURNEYS.md` plus `e2e/journeys.spec.ts`;
- a `verify-change` record;
- a UX research round (`app-design`).

Until all of those exist, the app stays in its MR. Then `bun run release:apps <id>` and commit `bundles/` (and the list entry it adds) in the same PR. To publish an update, bump `version` first: servers show it in Discover as **UPDATE**.

## Known apps

There are no registries and no discovery. A server offers only the apps in its **known-apps list**, `apps/server/known-apps.json`:

```json
{
  "base": "https://raw.githubusercontent.com/flying-dice/AgentOS/main/bundles/",
  "apps": ["calculator/bundle.json"]
}
```

- **Entries** are `bundle.json` URLs, absolute or relative to `base`. Without a `base` they resolve against the list file.
- **The URLs are fixed:** `bundles/` is committed to this repo, so the GitHub URLs above serve the released bundles. Later a CDN takes over by changing the one `base` value.
- **Clients install by id only** (`apps.known()` lists the apps with their `installedVersion`; `apps.install(appId)` installs or updates one). A client can never name a URL, so it can't make the server fetch one.
- **Configuration** (env vars on the server):
  - `AGENTOS_APPS_BASE`: overrides `base` with a URL or a path. `bun run dev` sets it to the local `dist/bundles/`, so development uses the same list against local builds.
  - `AGENTOS_APPS_TOKEN`: for a private repo. Sent as `Authorization: Bearer <token>` only on requests to the base URL's origin, never to other hosts (redirects included).
  - `AGENTOS_APPS`: extra absolute `bundle.json` URLs (comma-separated).
  - `AGENTOS_KNOWN_APPS`: a different list file.
- **Allowed URLs:** `https:` and `file:`; `http:` only on localhost. A hosted `bundle.json` can't point at `file:` URLs. A broken entry is logged and skipped; the rest still load.
- **Trust:** a server module runs with the server's full privileges. The list is the allow-list: add only authors you trust. The sha256 check guards against tampering in transit and a bundle that doesn't match its descriptor, not against a malicious author.

## Rules that are enforced

`bun run lint` (`scripts/check-boundaries.ts`, `check-atomic.ts`, `check-testids.ts`):

- **An app imports** only the SDK, the kit, `svelte` and `zod` on the client. Its server code may use `bun:*`, `node:*` and npm packages declared in its own `package.json`, never host internals, other apps or client packages. `src/client` and `src/server` never import each other; `src/shared` is browser-safe pure logic.
- **The server never imports app packages:** apps arrive as bundles.
- **All client ↔ server traffic is the SDK protocol** (`@agentos/sdk/host/protocol`, zod-validated on the server): no raw WebSockets, no extra endpoints.
- **No raw HTML** (`{@html}`, `innerHTML`): agent and tool output is untrusted.

Missing a capability? Extend the SDK (contract, server implementation, in-memory host, tests), never a side channel.
