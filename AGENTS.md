# Working on vos

vos (Virtual OS) is an iPadOS-style virtual desktop for agents. It has three parts:

- **The host** (`crates/`): a Rust program, one Cargo workspace laid out as a hexagon. It runs the apps, owns their
  data, serves the client, and speaks JSON-RPC 2.0 over WebSocket at `/rpc`, AG-UI over HTTP + SSE at `/agui` (agent
  chats), MCP at `/mcp` (the tools agents call) and ACP to the agent processes it launches. It binds the LAN and every
  client must pair as a device first.
- **The apps** (`apps/<id>/`): server-side code **authored in typed Luau** (`app.json` + `*.luau`, `--!strict`, checked
  by luau-lsp against the generated Luau declarations in `types/generated/vos.d.luau`): `bun run build:apps` bundles an
  app into one `app.luau` with darklua (types and comments stripped), and the host runs that in one VM per app (the
  `vos-lua` adapter on the `neoc-vm` library, Luau; see "Language" in the neoc README for how Luau's base library differs),
  loaded from an app ref over `file://`, `http://` or `https://`. An app holds its state, data, tools, notifications
  and UI description. Examples: `apps/calculator`, `apps/agents`.
- **The client** (`client/`): a managed Vite + Svelte 5 PWA. It renders the view trees the host sends with the kit in
  `packages/ui` (atomic layers) through adapters in `client/src/view/`, holds client-managed state, and
  runs **no app code**.

Bun workspaces cover `client` and `packages/*` (an app is Luau, `app.json` + `*.luau`, not a Bun package); Cargo covers
`crates/*`.

The direction is a self-hosted home lab: apps can provide LAN services such as DNS, DHCP and container-backed services,
so vos can stand in for the services people run on a home NAS.

## Sources of truth

| For | Read |
|---|---|
| The platform: shape, crates, decisions | `docs/architecture/ARCHITECTURE.md` |
| The host: ports, adapters, pairing, config, `/agui`, tool permissions | `docs/architecture/host-stack.md` |
| The server-driven UI: view protocol, bindings, state, the Luau authoring API | `docs/architecture/ui-system.md`, `docs/reference/components.md` (generated) |
| The client: shell, renderer, adding a component | `client/README.md` |
| Writing an app: manifest, `app.*` API, security, adding an app | `apps/README.md`, `types/generated/vos.d.luau` (the generated Luau declarations of the API and of the modules an app may require) |
| Tests, test ids, headless runs | `docs/TESTING.md` |
| Design language, WCAG AA, §11 checklist | `docs/design/DESIGN_LANGUAGE.md` |

`types/generated/*` and `docs/reference/components.md` are generated from the Rust schema (`bun run codegen`); never
edit them by hand.

## Work tracking and docs

- Issues and work items live in **Jira project AGOS**:
  <https://flying-dice.atlassian.net/jira/software/projects/AGOS/boards/101>. Create new work there, not as GitHub
  issues (GitHub issues #1-#18 are legacy).
- Long-form wiki and planning pages live in the **Confluence space AGOS**:
  <https://flying-dice.atlassian.net/wiki/spaces/AGOS/overview>. The in-repo `docs/` stay the source of truth for
  architecture, testing and design (the table above).
- Current epic: **AGOS-1**, home-lab network services (DNS, DHCP, containers). vos aims to replace the services
  people run on a home NAS such as Synology.

## Merge policy

- All work happens on a branch and lands through a PR. Never push to `main`.
- Nothing merges without the full gate green locally on the head commit (lint, typecheck, test, e2e; there is no
  hosted CI) and a `vos-code-review` verdict of **READY TO MERGE**.
- An app that isn't ready for the full gate and the UX research panel stays in its MR. Keep pushing to the
  branch; don't merge a partial app behind a flag or with "tests later".

## Definition of done

A change is done when all of these hold:

1. `bun run lint && bun run typecheck && bun run test && bun run test:e2e` pass.
2. New behaviour has new checks: `cargo nextest` tests for Rust logic (domain, use cases against the port fakes,
   adapters, the host end to end), `bun test` for client and kit logic, a story per new screen or state (a kit
   component's own story, or a JSON-tree story on a fake host for a renderer or shell state), a journey spec per new
   or changed journey. Changed snapshots were looked at.
3. Every interactive element has a `data-testid` (kit components take a required `testid` prop; a Luau app's
   interactive nodes have a `key`); it works by keyboard, in light and dark, and at the app's minimum window.
4. UI changes carry a `verify-change` "Manual verification" record in the PR.
5. For a new app or a change people will notice in one: the readiness gate in `vos-code-review` §6 holds,
   including a `docs/ux/<app>/round-<n>.md` that meets the `app-design` exit criteria.

## Boundaries

- **Crate dependency direction** (hexagonal; inward only): `vos-domain` is pure and depends on no other crate;
  `vos-application` (ports and use cases) depends on domain; each adapter (`lua`, `sqlite`, `mcp`, `acp`, `agui`,
  `fetch`, `http`, `codegen`) depends on application and domain and **never on another adapter**; `vos-host` is the
  composition root and the only crate that sees them all. `neoc-vm` is a support library, not an adapter: the
  per-app Lua VM (mlua on Luau, built from source), depending on no other crate here (the `neoc` CLI is its other client, and
  `neoc`/`neoc-vm` never depend on a `vos-*` crate). Among the `vos-*` crates only the `lua` adapter may depend
  on it (it reaches mlua through neoc-vm's re-export, so mlua stays out of every vos crate). Enforced by `bun run lint:deps` (`scripts/check-deps.ts`,
  an allow-list over `cargo metadata`) and `cargo deny check` (`deny.toml`: each framework crate is pinned to its
  adapter). A new crate must be placed in the allow-list on purpose.
- **Rust hygiene** (`[workspace.lints]`): clippy pedantic with `-D warnings`; `unsafe_code` is forbidden;
  `unwrap`, `expect` and `panic!` are denied outside tests; `thiserror` in library crates, `anyhow` only in the host
  binary.
- **Apps are the person's own code on their own host. The VM keeps their Lua state apart; it is not a security
  boundary.** An app runs as Luau in its own VM and its own environment table: its globals, its standard
  libraries (read-only) and its module tables are its own, so apps do not share state by accident, and it has no
  `load*`/`getfenv`/`setfenv`. That is all the separation there is. Through `std:fs`, `vnd:rusqlite` and the other
  modules an app can read and write other apps' data and the host's own database under `~/.vos`. There are **no
  permissions** and **no budgets** (no CPU, memory or time limits): a runaway app (a loop that never yields, a
  `std:thread` worker) can hang the host, not just itself, and can outlive its uninstall, because unloading aborts
  work only at the points where the app yields to the host. That is accepted. It reaches the host through the `app` table (typed by the generated Luau declarations in `types/generated/vos.d.luau`) and
  through **every `std:`, `lib:` and `vnd:` module of `neoc-vm` except `lib:test`, `lib:compile` and `std:workers`** (`require("std:fs")`, `require("lib:json")`,
  `require("vnd:hyper")`, ...: files, network, servers, tasks, threads, SQLite, TLS), next to `vos:app` and
  `vos:ui`. All of them are typed in the declarations; the refused three are not (`vos_domain::NOT_FOR_APPS` is the one
  list: the adapter refuses them, the declarations type each as unusable so naming one is a type error). What an app starts (tasks, servers, workers) belongs to it: unloading or replacing the app aborts
  it and closes its listeners, and what a task or a request handler runs queues behind the app's other calls (one
  call at a time per VM). Still enforced: a tool declared `readOnly` cannot write through the `app.*` API (db, kv, state,
  notify, shell, windows, and registering tools, windows and handlers), and neither can the tasks it starts or the
  requests it triggers through the app's own server (a request is read-only while any read-only call is acting: running,
  queued, or waiting at something in flight such as a request; only one asleep at a timer does not count, so a poller
  that also makes requests still taints webhooks intermittently: put long-lived work that must write in a writable
  call, not a `readOnly` tool; `docs/architecture/host-stack.md` has the details and what stays open); that guarantee does **not** cover the neoc modules (`std:fs`, `vnd:rusqlite`, `std:net`, ...),
  which are open to every tool. Agent auto-approval of `readOnly` app tools relies on this `app.*`-only guarantee
  (owner decision: auto-approval stays on). SQL `ATTACH` is denied through `app.db`/`app.kv` (not through `vnd:rusqlite`), a
  cyclic or absurdly shared table handed back to the host is an error (never a stack overflow), a Lua number can
  not panic a module or size an unbounded allocation (an error instead: a host panic aborts every app), and chunks
  load as text only.
- **Luau, the stack and what an error value can do.** The engine is Luau. An error value whose `__tostring` makes a
  protected call and then fails (which aborted the host on LuaJIT) is answered `error in error handling` once, so it
  reaches the adapter as an ordinary error on every path; `crates/vos-lua/tests/poison.rs` holds one test per path
  (entry chunk, a table given to `app.tool`, `app.notify` or `app.window`, tool, event, request, write-back,
  transaction body, task, render). Recursion through a Rust host function that calls back into Lua stops at Luau's
  C-call limit (200) with `C stack overflow`, provided the thread's stack holds those levels: **every thread that polls
  a VM has `neoc_vm::VM_THREAD_STACK` (8 MiB)**, which `vos_host::runtime` gives the workers and the blocking
  pool (the host re-exports it through `vos_lua`, since the host may not depend on neoc-vm), and the top-level
  future runs on a worker, not on the `block_on` thread; `crates/vos-host/tests/stack.rs` holds it in a debug
  build. A new thread of the host's own that calls into Lua must be built with that stack too. (A Tera panic in
  `lib:template` aborts under `release-host`'s `panic = "abort"`: NEOC-125.)
- **The client runs no app code.** The renderer draws view trees and sends events; nothing executable crosses the
  wire. Kit knowledge of apps, and app knowledge in the client, are both bugs.
- **Agent and tool output is untrusted:** render it as text. No `{@html}` or `innerHTML` for it (the only `{@html}` is
  `packages/ui/src/atoms/Icon.svelte`, over the constant icon map). RPC params, MCP tool inputs, and anything an app
  or agent sends are validated at the edge.
- **Missing a capability?** Extend the host API: the domain type and schema, the application port and use case, the
  adapter, the codegen output and the tests, in that order. Never a side channel (no extra route, raw socket or
  global); client ↔ host traffic is the protocol (JSON-RPC on `/rpc`, AG-UI on `/agui`) and nothing else.
- Release what you open: the host registers and unregisters apps at runtime, so an adapter's resources (VMs, DB
  handles, watchers, agent processes) are dropped on unload.

Enforced by `bun run lint:boundaries` (`scripts/check-boundaries.ts`) on the client and the kit: no relative import
leaves its package (the client never reaches into `crates/`), the client and kit import only `@vos/ui` (stories
also `@vos/storybook`), no Node or Bun builtins in browser code, `new WebSocket` only in
`client/src/view/rpc/websocket.ts`, and no `{@html}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML` or
`document.write`.

Also: atomic layers (`lint:atomic`, in `packages/ui` and `client/src`) and test ids from the first commit
(`lint:testids`).

## Skills: when to use which

| When | Skill |
|---|---|
| Writing or changing code (Rust, Luau, Svelte/TypeScript) | `coding-styleguide` |
| Before pushing | `vos-clean-code-review`, then `vos-pre-commit` (loops until clean; `vos-refactor` clears markers) |
| Every MR, and before calling a branch ready | `vos-code-review` (verdict: READY TO MERGE or STAYS IN MR) |
| Any change to what a person sees or does | `verify-change` |
| Before building a new app (or re-scoping one) | `prebuild-synthetic-ux` (personas, riskiest assumptions, key journeys and stories → `docs/ux/<app>/discovery.md`) |
| Designing or redesigning an app | `app-design` (discovery, journeys, usability review, the `ux-research` panel, conformance, iterate) |
| Reviewing the usability of something built | `postbuild-synthetic-usability` (task walkthroughs on the real host, heuristics and WCAG → `docs/ux/<app>/review-<n>.md`) |

## Commands

```sh
bun run dev             # build:apps, then the host on :7400 (dev_reload) + Vite on :5173; pair with the code the host prints
bun run build:apps      # bundle every apps/*/app.json into one Luau file (darklua) + app refs + registry.json under dist/apps
bun run build           # build the client (client/dist)
bun run host            # cargo run -p vos-host: serves client/dist, /rpc, /agui, /mcp on :7400
bun run host pair       # print a fresh device pairing code (every client must pair; see host-stack.md)
bun start               # the same host, built with the release-host profile (opt-level z, panic abort)
bun run lint            # conflict markers + atomic layers + test ids + boundaries + rust + luau (luau-lsp) + stylua + codegen:check + package eslint
bun run lint:conflicts  # leftover merge-conflict markers in tracked files (runs first)
bun run lint:rust       # cargo fmt --check, clippy -D warnings, lint:deps (crate direction), cargo deny
bun run lint:luau       # luau-lsp analyze, strict, of every app against types/generated/vos.d.luau (plus the usage fixture and the negative fixtures, their marker counts pinned)
bun run lint:stylua     # StyLua check (skipped with a message when stylua is not installed)
bun run typecheck       # cargo check --workspace --all-targets + tsc on types/ + svelte-check in each package
bun run test            # cargo nextest over the vos crates (neoc's crates have their own gate) + bun test (types, scripts, client, kit)
bun run format          # prettier + cargo fmt + StyLua
bun run test:e2e        # all Storybooks + desktop + journeys, headless, against the real host
                        # VOS_PORT_BASE=9100 for a second worktree (Storybook = base, e2e host = base + 10)
bun run test:e2e:update # rebaseline snapshots (then look at them)
bun run storybooks list # every Storybook (client, packages/ui); each package: bun run storybook
bun run codegen         # regenerate types/generated/* (incl. vos.d.luau, ui.lua, prelude.lua) and docs/reference/components.md from the Rust schema
bun run codegen:check   # fails when they are stale (part of bun run lint)
bun scripts/design-audit.ts <page>  # static styleguide audit of a built-in shell page's styles
```

Visual baselines (`e2e/__screenshots__`) are macOS-only, so run `test:e2e` on a Mac. There is no hosted CI: the
`vos-pre-commit` skill runs the gate and `vos-code-review` checks it. Needed tools: Rust (the pinned toolchain),
`cargo-nextest`, `cargo-deny`, Bun, `darklua` 0.18.0 (`cargo install darklua --version 0.18.0 --locked`; building the
apps fail without it, they never skip) and, for the Luau checks, `luau-lsp` 1.70.1 (the release binary from the luau-lsp GitHub releases, on `PATH` or in
`~/.local/bin`; there is no Homebrew formula) and `stylua` (with Luau syntax). Without them `lint:luau` and
`lint:stylua` skip on a developer machine and **fail** when `VOS_GATE=1` or `CI` is set (the `vos-pre-commit` skill
runs the gate that way; the luau-lsp editor test in `crates/vos-codegen` follows the same rule). No system Lua is
needed: the Rust tests load and run the bundled apps on neoc-vm's Luau. A native Lua 5.3+ is optional: with it
`bun test scripts` also compares the prelude's `utf8` with the native one, and the codegen contract test runs the
example apps' bundles with it.

## Adding an app

Follow `apps/README.md` → "Adding an app". From its first commit the app has an `app.json`, an `app.manifest`
with a `window` min size (and a comment when the layout needs more than 320x240), a `JOURNEYS.md` plus
`e2e/journeys.spec.ts`, and a Rust test of its logic and tools next to the others in `crates/vos-lua/tests`.
The generic `desktop` project opens every `apps/*/app.json` in the real shell (sizes, layouts, light and dark, axe,
snapshots) with no extra code; each screen and state beyond the landing view is reached by a journey, or, when a
journey can't reach it cheaply, shown by a JSON-tree story in the client Storybook. The app is `lint:luau` (strict) and
StyLua clean from the first commit. Then design it with `app-design` until the exit criteria hold; until then it
stays in its MR.
