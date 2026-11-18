# AgentOS architecture: Rust host, Luau apps run as Lua 5.1, managed client

Status: **approved** (2026-11-18). LAN binding with device pairing (see `host-stack.md`). Detail lives in two companion documents, both backed by working prototypes:

- [`host-stack.md`](host-stack.md): the Rust host. 11 crates in the hexagonal layout below, 42 tests passing, end to end
  from a Luau app installed over `/rpc` to its tool called over MCP.
- [`ui-system.md`](ui-system.md): the server-driven UI. The component catalogue, view protocol, bindings, the Luau API
  with Calculator and Notes written in full, and a Svelte renderer prototype with 28 tests passing.

This replaces the Bun/TypeScript server, the app client bundles and the shared-module mechanism.

## 1. The shape

```
 Browser: managed client (Vite + Svelte 5 PWA)           Rust host (one process, 127.0.0.1)
 ┌──────────────────────────────────────┐                ┌───────────────────────────────────────────┐
 │ shell: windows, Dock, Stage Manager  │ JSON-RPC / WS  │ application: use cases over ports         │
 │ renderer: view trees → kit (Svelte)  │◀──── /rpc ────▶│   install, dispatch event, tool call,     │
 │ client-managed state, drafts         │                │   agent run, notifications                │
 │ no app code, ever                    │ AG-UI / SSE    │ adapters:                                 │
 │ agent chats ◀────────────────────────┼──── /agui ─────│   luau (one sandboxed Lua 5.1 VM per app) │
 └──────────────────────────────────────┘                │   sqlite (per-app DB, live queries)       │
                                                         │   mcp /mcp ◀── agents (per-agent tokens)  │
  built app packages ── app refs ── registries            │   acp ──▶ agent processes (stdio)         │
  (file:// http:// https://)  ──────────────────────────▶│   fetch, http, codegen                    │
                                                         └───────────────────────────────────────────┘
```

- **Apps are entirely server side, authored in Luau and run as Lua 5.1.** They hold their state, data, tools,
  notifications and UI description. The client runs no app code. Luau is the authoring language (strict types,
  `luau-lsp`); `bun run build:apps` builds an app into one bundled Lua 5.1 file (darklua), and that file is what the
  host loads and runs.
- **The client is a managed renderer.** It renders view trees with the kit, holds client-managed state (drafts,
  selection, scroll, navigation) for instant response, and sends events back.
- **Transports:**
  - **`/rpc` (JSON-RPC 2.0 over WebSocket):** the client and host, carrying view trees, ops, data patches, events and app lifecycle.
  - **`/agui` (AG-UI over HTTP + SSE):** agent chats.
  - **`/mcp` (MCP Streamable HTTP):** the tools agents call.
  - **ACP:** from the host to the agent processes it launches.

## 2. Rust workspace: hexagonal

```
crates/
  agentos-domain        pure model, no I/O: app ids, app refs, manifests, permissions, agent identity, tools,
                        notifications, view tree and component schema, protocol messages
  agentos-application   ports (traits) and use cases; in-memory fakes for tests; no frameworks
  agentos-lua          adapter: runs apps as sandboxed Lua 5.1: the app API modules, budgets, views
  agentos-luavm         support library (not an adapter): the Lua 5.1 VM on mlua, the per-app environment, the
                        budgeted string functions; depends on no other crate, only `luau` may depend on it
  agentos-sqlite        adapter: per-app rusqlite DBs, migrations, live queries via commit hooks
  agentos-mcp           adapter: rmcp server, dynamic tool list, per-agent bearer identity
  agentos-acp           adapter: agent-client-protocol client, agent processes
  agentos-agui          adapter: AG-UI events and the SSE endpoint, mapped from ACP
  agentos-fetch         adapter: app refs and registries over file/http/https, sha256 checks
  agentos-http          adapter: axum, the /rpc peer, Origin/Host guard, static client, event hub
  agentos-codegen       adapter: schema → TypeScript types, Luau .d.luau, docs
  agentos-host          composition root binary (wiring only)
```

- **Direction:** dependencies point inward only. Adapters depend on application and domain, never on each other;
  domain depends on nothing framework-shaped. Enforced by `scripts/check-deps` (an allow-list over `cargo metadata`)
  plus `cargo-deny` bans.
- **Hygiene:**
  - `[workspace.dependencies]`, with exact pins for the fast movers (`rmcp`, `agent-client-protocol`, `mlua`);
  - `[workspace.lints]`: clippy pedantic, `unsafe_code = "forbid"`, and `unwrap`/`expect`/`panic` denied outside tests;
  - `thiserror` per library crate, `anyhow` only in the binary.
- **Tests per layer:**
  - **Domain:** unit tests.
  - **Application:** use-case tests against the port fakes.
  - **Adapters:** integration tests.
  - **Host:** one end-to-end test.
  - `cargo nextest`.
- **Crates** (versions in `host-stack.md`):
  - runtime: tokio, axum;
  - Lua 5.1: mlua (`lua51`, `vendored`, `async`, `serialize`, `send`), in `agentos-luavm` only;
  - SQLite: rusqlite (bundled);
  - protocols: rmcp, agent-client-protocol;
  - fetch: reqwest (rustls);
  - types: ts-rs and schemars;
  - hot reload: notify.
  - The JSON-RPC peer and the AG-UI types are hand-rolled: there's no bidirectional JSON-RPC crate that fits, and no
    maintained official AG-UI crate.

## 3. Apps

- **Package:** a Luau package `apps/<id>/`: `app.luau` (the entry), modules, `JOURNEYS.md` and e2e. It is built into one
  `app.lua` (the entry and its modules bundled by darklua, types stripped, Luau-only syntax lowered); that file is
  what the app ref lists and the host runs. App logic is tested from Rust integration tests that load the real package (`crates/agentos-lua/tests/calculator_app.rs`, `crates/agentos-host/tests/agents_app.rs`); an in-package `tests/*.luau` harness is planned, not built.
  Built into an app ref: manifest, file URLs and sha256s, and `requires` (host API range).
- **Registries:** a list of app-ref URLs. The default registry is the build's unpacked apps folder over `file://`.
  Users can also add an app by URL; the host shows a preview, then a confirmation that it runs on this host and its
  tools reach agents. A later installer bundles host, client and apps.
- **Sandbox** (details in `host-stack.md` §1):
  - one Lua 5.1 VM per app and one environment table per app: an allow-list of base functions, no `load*`,
    `getfenv`/`setfenv`, `io`/`os`/`debug`, read-only standard libraries, text-only chunks, a locked string
    metatable;
  - `require` knows only `@agentos/app` and `@agentos/ui` (an app's own modules are bundled at build time);
  - a memory limit;
  - a CPU budget by the VM's count hook (a runaway loop is killed, also inside coroutines and `pcall`; waiting on the
    host doesn't count), and the pattern-matching string functions run in host code charged to the same budget,
    because the C matcher cannot be interrupted;
  - SQL `ATTACH` denied.
  - Blocking host calls get their own timeouts.
- **App API** (typed with generated `.d.luau`):
  - **Window and UI:** `app.manifest`, `app.window(name, { client = …, render = fn })`, `ui.*` constructors.
  - **Data:** `db:migrate`, `db:query` / `db:row` (live sources), `db:exec`.
  - **Everything else:** `app.state`, `app.tools.register`, `app.notify`, and `app.shell` (Dock badge, menu commands,
    deep links).
- **Lifecycle:** install, enable, disable (data kept, agent access blocked), delete data, uninstall. Hot reload in
  dev rebuilds the VM and keeps the DB.

## 4. UI system (summary of `ui-system.md`)

- **Catalogue at UIKit/Qt parity, in tiers:**
  - **Tier 1 (the first apps):** stacks, grid, scroll; `Screen`, `NavigationStack`, `SplitView` (2 and 3 columns),
    `TabView`; `List` (all styles, sections, swipe, reorder, multi-select, windowing), `Form`; every standard
    control; `Sheet`, `Popover`, `Alert`, `ConfirmationDialog`, `Menu`, `ContextMenu`, `Toast`; text, label, icon,
    image, badge, progress, `Markdown`, `Avatar`, `EmptyState`; drag and drop.
  - **Tier 2:** `Table`, `ChatTranscript` (bound to an AG-UI thread), `CalendarGrid`, `Outline`, `Gauge`.
  - **Tier 3:** `CodeEditor`, `DiffView`, `Canvas`, `WebView` (deferred with the Browser).
- **Protocol:**
  - **`view.tree`:** a full tree, sent on open and on resync.
  - **`view.ops`:** keyed structural ops, versioned; a gap triggers `view.sync`.
  - **`data.patch`:** per-binding value updates.
  - Values change far more often than structure, so typing, agent writes and list refreshes never rebuild the tree.
  - Budget: event to pixels in 30 ms p95, host-local.
- **Bindings:**
  - **Props:** literals, refs, or a closed set of pure functions evaluated on the client.
  - **Four tiers of state:** ephemeral UI and client-managed state (the client is authoritative); app state and data
    (the host is authoritative).
  - **Sources:** live SQLite queries, sent to the client as ids; the SQL never leaves the host.
  - **Lists:** a row template once, then windowed row deltas.
  - **Agent writes:** an agent's write through a tool updates every open window, with no UI code.
- **Two-way edits:**
  - **Text:** an instant draft, then a debounced write with `baseRev`. The host does a 3-way merge; overlapping
    edits go to the human and the losing text is kept as a revision.
  - **Scalar controls:** the human wins.
- **Styling:** apps don't style. They get semantic tokens only (spacing, text styles, palette roles, tints, radii).
  The host owns tokens and theme. Accessibility (labels, roles, focus, keyboard, WCAG AA) is built into the kit.
  Test ids derive from node keys.
- **Type pipeline: Rust first.** The component schema lives in `agentos-domain` as Rust types plus metadata (events,
  slots, accessibility rules, tier). `agentos-codegen` emits the TypeScript types for the renderer, the Luau
  `.d.luau` and constructors for app authors, and docs. Contract tests fail the build on drift. One toolchain, and
  R1 proved the ts-rs → TypeScript and schemars → Luau path.

## 5. Repo layout after the move

```
Cargo.toml, crates/        the host (above)
client/                    the managed client: shell + renderer (was apps/desktop), Vite + Svelte 5
packages/ui/               the kit: Svelte components implementing the schema, tokens, stories (Storybook on Vite)
apps/<id>/                 Luau app packages only
types/                     generated: TypeScript for the client, .d.luau for apps
docs/, .claude/skills/     rewritten for Rust and Luau (coding style, review, testing)
```

- **Removed (done, wave B4):** `apps/server` (TypeScript), `packages/sdk`, app client bundles and `bundles/`, `SHARED_MODULES`, the memory host and web mode.
- **Testing:** client tests run against the real host, which starts in milliseconds; the test harness builds the
  Luau apps to Lua 5.1 and runs them headless in the host.

## 6. Decisions on the open questions

| # | Question | Decision |
|---|---|---|
| 1 | Remote host | **Needs you:** see §8 |
| 2 | Persist client UI state on the host | Yes, as app data in `_ui_state`; clearable per app in Settings |
| 3 | Two humans in one window | Not now: one human plus agents. OT or CRDT later if needed |
| 4 | Heavy client engines (CodeMirror 6, xterm.js) | Yes, lazy-loaded kit code, never app code |
| 5 | WebView | Deferred with the Browser app (#12) |
| 6 | RPC params | By-name objects for the new protocol |
| 7 | Schema authoring | Rust first (§4) |
| 8 | Shell declarations | A separate `app.shell` |
| 9 | i18n | Per-app catalogues resolved on the host; plurals and formatting on the client. Hooks now, catalogues later |
| 10 | Offline | Dim, keep drafts, queue writes, resync on reconnect |
| 11 | Permissions | Enforced at the tool and `app.db` capability layer; an agent can be denied a source the user can see |
| 12 | Conflict revisions | Kept 30 days; "undo agent change" is a shell feature, later |
| 13 | Client/kit version skew | The host serves its own client, so skew is rare: `Unsupported` placeholder plus per-node `fallback` |
| 14 | Compact breakpoint | `DESIGN_LANGUAGE.md` wins (700 px sidebar collapse); one constant in the kit |
| 15 | CRDT (Loro) for document content | Deferred (user, 2026-11-18). Text edits use the 3-way merge; the source design stays open to a later `db:doc` kind backed by Loro |

## 7. Build plan

Sonnet agents in git worktrees implement; the lead coordinates, merges, and runs the gate. Each wave lands green
before the next starts.

| Wave | Work (one worktree each) |
|---|---|
| A | **A1** Import the spike workspace into `crates/`, gate scripts (`fmt`, `clippy`, `nextest`, `check-deps`, `deny`), and a host that serves the client<br>**A2** Schema and codegen pipeline (Rust → TS, `.d.luau`, docs)<br>**A3** Renderer core in `client/`: tree, ops, data patches, events, refs and expressions, client state<br>**A4** Luau app API: windows, render, sources and live queries, two-way bindings and merge, tools, notify |
| B | **B1** Kit tier 1 components implementing the schema, with stories<br>**B2** Calculator in Luau as the acceptance app, plus the e2e harness against the real host<br>**B3** ACP streaming → AG-UI, the Agents app with `ChatTranscript`, and per-agent identity and permissions<br>**B4** Docs and skills rewritten for Rust and Luau; remove the TypeScript server |
| C+ | The issue backlog, re-expressed on this platform:<br>foundations (#2 data, mostly A4; #3 notifications; App Store #4),<br>then app waves (Contacts, Notes, Tasks, Reminders → Calendar, Settings, Skills, Agents, Files, Books, Diagrams → Coder, Reviewer).<br>Browser deferred. |

Every app keeps the existing merge policy: stories, journeys, the full gate, a `verify-change` record, and a UX
research round before merge.

## 8. Needs your decision

1. **Remote host.** Local (127.0.0.1) only for now, with LAN and remote later? A remote host needs real
   authentication and puts a round trip on every server-side event (fine for typing, which is client-side; keypads
   feel it over the internet). **Recommendation:** local first; design auth in when a remote host is needed.
2. **Approve this document**, after which wave A starts.
