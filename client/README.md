# @agentos/client

The AgentOS client: an installable PWA (Vite + Svelte 5) that is the iPadOS-style shell. It talks to the Bun server
(`apps/server`) over one WebSocket with JSON-RPC 2.0 (`@agentos/sdk/host/client`), and loads installed apps' client
bundles from the server.

```sh
bun run dev        # with the server: from the repo root, `bun run dev` (server :7400 + this on :5173)
bun run dev:web    # web mode on :6200: the shell on the in-memory host, repo apps from source, no server
bun run build      # → dist/ (served by the server in production)
```

Entry points: `src/renderer/index.html` → `src/boot.ts` (real server) and `src/renderer/web.html` → `src/web/main.ts`
(in-memory host). See `apps/README.md` for building apps and `docs/TESTING.md` for tests.

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
