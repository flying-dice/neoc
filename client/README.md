# @agentos/client

The AgentOS client: an installable PWA (Vite + Svelte 5) that is the iPadOS-style shell. It talks to the Rust host
(`crates/agentos-host`) over one WebSocket with JSON-RPC 2.0 (`/rpc`), pairs through `/session` and `/pair`, and renders
the windows of the Luau apps installed on the host with the managed renderer below. It runs no app code.

```sh
bun run dev        # from the repo root: builds the apps, runs the host on :7400 and this on :5173 (pair with the code the host prints)
bun run build      # → dist/ (the host serves it: `bun run host`)
```

Entry point: `src/renderer/index.html` → `src/boot.ts`. See `docs/TESTING.md` for tests and `docs/architecture/host-stack.md`
for the host.

## The shell (`src/renderer/src`)

```
boot.ts                 start: GET /session, then the pairing screen or one Session; exposes window.__agentos (tests)
App.svelte              composition root: pairing gate, built-in pages, ViewWindow for installed apps
lib/host.svelte.ts      the host connection: phase (checking, unpaired, connecting, ready), the one Session, the app list,
                        window.open requests, re-pairing after a revoke
lib/apps.svelte.ts      the app registry the window manager reads: built-in pages + running installed apps (icon, tint,
                        window size and minimum from `app.manifest`); the Dock order
lib/windowManager.svelte.ts   Stage Manager windows (floating, full screen, split, Slide Over); a window has an app and a window kind
shell/channel.ts        the host's own methods and notifications (app.*, host.*, window.open) on the session's transport
shell/pairing.ts        GET /session, POST /pair and their messages (wrong code, rate limit, unreachable)
shell/hostApi.ts        the wire types of app.* / host.devices.* (mirror of types/generated/agentos.ts, tested for drift)
shell/fakeHost.ts       a fake of those methods for stories
organisms/ViewWindow    an installed app's window: view.open on mount, view.close on destroy, <ViewHost> in between
pages/                  PairingPage, DiscoverPage (apps: install, enable, disable, remove, add by URL), SettingsPage
                        (Devices), and the built-in Files, Terminal, UI Kit and Agents pages
```

The Agents page (and `lib/agents.svelte.ts`) still reads the old SDK bridge and is out of the Dock until wave B3
replaces it with the AG-UI Agents app.

## The managed renderer (`src/renderer/src/view`)

The client half of the server-driven UI (`docs/architecture/ui-system.md`, sections 2 and 3). The host owns truth and
structure; this renders view trees with the `@agentos/ui` kit, holds client-managed state and drafts, and runs **no app
code**. The shell (windows, Dock, Stage Manager, built-in pages) does not use it yet: wave B2 connects the two.

```
protocol.ts      every wire type (view.tree/ops, data.patch, requests, actions). Hand-written; wave A2 replaces it
                 with types generated from the Rust schema. The only module that declares a wire shape.
rpc/             peer.ts  JSON-RPC 2.0 peer, by-name params          websocket.ts  /rpc transport, backoff + jitter
core/            framework-free, plain objects under bun test; Svelte hands it a $state store
  view.ts          ViewCore: one window. open/sync/resume, ops (base/rev, gap -> view.sync), refs, events, actions
  expr.ts          the closed expression language ($ref / $fn, fmt.* via Intl, a node budget)
  tree.ts          keyed ops and key-path addressing          testid.ts  test ids from keys (spec 5.4)
  bindings.ts      drafts: overlay, debounce/maxWait, shielding, baseRev, ack adoption, rebase, offline queue
  sources.ts       rows windows, row deltas, params as refs (re-query), loadMore, offline refresh
  clientState.ts   declared client state: init, persist (session/app), mirror to the host, client.set
  session.ts       one connection, many views: hello, routing by viewId, resume on reconnect
registry.ts      type -> adapter component (the extension point)       kit.ts  the starter kit registered by default
atoms/ molecules/ organisms/ templates/ pages/    Node, ViewHost and the adapters (Stack, Text, Button, TextField,
                 Toggle, List/ForEach/Row, Screen, Show, Group), one atomic layer each
host/            test support only: a TypeScript fake host on an in-memory wire, a demo "Notes-lite" app, a manual clock
stories/         Storybook "Renderer/Notes window"
```

**Data flow.** `Session` owns the socket and handshakes (`session.hello`). `session.openView` sends `view.open` and gets
a snapshot (tree, state, client decls, source decls, first windows). From then on the host sends three message classes:
`view.tree` (full replace), `view.ops` (keyed structural ops; a `base` that is not the client's `rev`, or an op that
cannot be applied, makes the client call `view.sync`) and `data.patch` (values only: state paths with revisions, source
windows). `ViewHost` renders `store.tree` through the registry; adapters read props with `view.evaluate(expr, scope)`
inside `$derived`, so a patch repaints exactly the props that read it.

**Writes.** A `Binding` prop (`TextField.text`, `Toggle.on`, `List.selection`) is written through `view.bindingOf(...)`:
`client.*` is instant and local; `state.*` and `src.<id>.<field>` / `row.<field>` become a draft overlay the control shows
at once, then a debounced `state.set` / `row.set` carrying `baseRev`. A dirty field shields itself from remote changes;
the host merges on the base it last sent and the ack carries the authoritative value, which the client adopts (caret
mapped through the diff, or the draft rebased if the user kept typing). Overlaps surface a notice; a strict source's
refusal shows the field's error state. Events (`view.event`, addressed by key path and row key) flush dirty drafts first.

**Offline.** On disconnect every window stays mounted, dims with a "Reconnecting" banner, keeps drafts and client-state
writes queued, and refuses events (a tap is not idempotent). On reconnect: `session.hello {resume}`, then
`view.open {resume}` per window; the snapshot goes through the same shielding as live patches and queued drafts replay on
their original `baseRev`. A restarted host gets a fresh instance seeded with the client's persisted values.

**Adding a component** (wave B1): write an adapter under the right atomic layer that takes `AdapterProps`, reads props
through `useView().view`, renders children with `<Children>`, passes `commonOf(...).testid` to the kit component, then
`registry.register('Gauge', { component: Gauge, since: '1.2' })` in `kit.ts`. An unknown type renders its node's
`fallback` slot or `Unsupported`.

Tests: `bun test` covers the peer, transport, expressions, ops, drafts and merges, sources, offline and resume against
the fake host. Storybook ("Renderer/Notes window") shows a tree, ops applied, a two-way binding, a windowed list, a
conflict, a refused write, offline, reconnect and an unsupported component.
