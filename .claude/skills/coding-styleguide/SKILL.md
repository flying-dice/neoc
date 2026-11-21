---
name: coding-styleguide
description: Use while writing or changing any code in this repo - the neoc runtime (crates/neoc, crates/neoc-vm: modules, the Luau engine), the vos host in crates/vos-* (hexagon), typed Luau apps in apps/ (the app API, table-call constructors, StyLua, luau-lsp), or the client and kit in TypeScript and Svelte 5 (client/, packages/ui, scripts). The house rules for where code belongs (neoc-vm takes no vos concepts; domain, ports and use cases, adapters, the composition root; per-app VM isolation (not a security boundary); the managed renderer), types and expected errors, validating at trust boundaries, and Svelte 5 runes. Read references/rust.md, references/luau.md or references/typescript-svelte.md for the language details.
---

# Coding styleguide

Read the neighbouring code and its tests first and match them. Apply these rules to the code you are changing;
don't migrate unrelated APIs, add dependencies or change configuration on the way.

## Where code belongs

**neoc** (`crates/neoc-vm`, `crates/neoc`) is a Luau runtime: modules (`std:`, `lib:`, `vnd:`), the sandbox, the embedding hooks. Its
rules (module conventions, errors as `(value, err)`, the facade that records each module's types, Rust style) are in
`CLAUDE.md` ("Modules", "Embedding the VM", "Rust style"); `runtime-design` and `requesting-module` cover design and new modules. `neoc-vm` depends on no
other workspace crate and takes no vos concept (apps, `readOnly`, views, `app.*`): vos policy stays in `vos-lua`.

**vos** is hexagonal. A Rust host runs Luau apps, each in its own VM, and serves a managed client; nothing crosses a process or
crate seam except through a port or the protocol.

| Role | vos |
|---|---|
| Domain | `crates/vos-domain`: pure model (app ids, app refs and manifests, tool grants, agent identity, tools, notifications, the view tree and component schema, protocol messages). No I/O, no framework. |
| Ports and use cases | `crates/vos-application`: the port traits (`AppRuntime`, `DataStore` / `AppData`, `ToolRegistry`, `Notifier`, `AgentConnector`, `Fetcher`, `Clock`, ...) and the services that orchestrate domain objects through them. In-memory fakes in `testing`. |
| Driven adapters | `vos-lua` (per-app VMs), `vos-sqlite`, `vos-fetch`, `vos-acp` (agent processes) |
| Driving adapters | `vos-http` (`/rpc`, pairing, guard, static client), `vos-mcp` (`/mcp`), `vos-agui` (`/agui`) |
| Dev-time adapter | `vos-codegen`: the Rust schema to `types/generated/*` (`vos.ts`, `ui.lua`, `vos.d.luau`) |
| Composition root | `vos-host`: wiring only (build adapters, hand each to its port, mount routers) |
| Apps | `apps/<id>`: Luau packages; their only contract is the `app` table (`types/generated/vos.d.luau`) |
| Client | `client/`: the shell and the renderer (`client/src/view`); it renders view trees and runs no app code |
| Kit | `packages/ui`: Svelte components in atomic layers, tokens, stories; knows nothing about apps |

The boundary rules:

- **Dependencies point inward.** An adapter depends on application and domain, never on another adapter; domain depends
  on nothing framework-shaped; application on domain only. Enforced by `bun run lint:deps` and `cargo deny check`.
  Frameworks (tokio, axum, mlua, rusqlite, reqwest, rmcp, ACP) stay in their adapters.
- **The VM keeps an app's Lua state apart; it is not a security boundary.** Apps have their own VM, environment and
  module tables, and use `app.*` and every `std:`/`lib:`/`vnd:` module with no permissions and no budgets (the modules
  reach other apps' data and the host database). What they start (tasks, servers, workers) is released when they are
  unloaded. Through `app.*`, apps reach other apps only through tools. A Lua number that reaches Rust is bounded or an
  error, never a panic.
- **The client runs no app code.** Client and host talk only through the protocol (JSON-RPC on `/rpc`, AG-UI on `/agui`);
  no raw sockets or side routes. A new wire shape is declared once, in the Rust schema, and generated.
- **Release what you open.** The host loads and unloads apps at runtime: an adapter's VMs, database handles, watchers and
  child processes are dropped on unload and shutdown.

**Missing a capability?** Extend the host API, never open a side channel: the domain type (and schema), the port and use
case in application, the adapter, the codegen output (`bun run codegen`), and tests at each layer. That is a platform
change: call it out in the MR.

UI layering is atomic (`bun run lint:atomic`; the styleguide is `.claude/skills/app-design/STYLEGUIDE.md`): atoms < molecules < organisms <
templates < pages, each importing only strictly lower layers. Reach for `@vos/ui` first; a generic need goes into the
kit (with a story and a required `testid` prop), never into one renderer adapter or app.

## Types and errors, in short

- **Rust:** typed errors per layer (`DomainError`, `PortError`, `AppError`), `?` to propagate, no `unwrap`, `expect` or
  `panic!` outside tests, no `unsafe`. Details in `references/rust.md`.
- **Luau:** `--!strict` typed Luau that `lint:luau` accepts, annotated signatures, plain functions for logic,
  `render` pure. Details in `references/luau.md`.
- **TypeScript and Svelte:** `strict` on, `unknown` until parsed, no `any`, no `as` on data you didn't construct, no
  non-null `!`; Svelte 5 runes only. Details in `references/typescript-svelte.md`.

## Trust boundaries (validate here)

| Boundary | Where |
|---|---|
| Client to host | JSON-RPC params on `/rpc`: parsed into typed domain messages with by-name params; an invalid shape is a `-32602`, never a panic. View events carry a key path and values; the host looks the handler up in the VM. |
| App to host | everything an app passes to `app.*` (SQL params, tool schemas, window specs, trees) is checked by the Lua adapter: the tree is validated before it leaves the VM, tool input is validated against the declared schema before `run`. A tool declared `readOnly` is enforced as read-only. |
| Agent to host | MCP tool calls: bearer token to agent identity, grants checked before the VM runs, input validated. ACP messages from an agent process are untrusted data mapped to domain events. |
| Host to client (agent output) | agent messages, tool results and permission prompts are untrusted text: render them as text nodes or kit `Markdown`, never `{@html}` or `innerHTML`. `/agui` events are validated in the client before use. |
| App refs | fetched files are sha256-verified against the app ref before install; a size cap applies; the add-by-URL preview runs nothing. |
| Disk | per-app SQLite and `system.db` rows written by an older version: migrations are numbered and checksummed; parse what you read. |

## Before you finish

Match the test layers in `CLAUDE.md` ("Tests"): Rust logic gets a test in its layer, client and kit logic bun tests next to the code, every
screen and state a story or a journey, every journey a spec, every interactive element a `data-testid`. UI changes go
through the `verify-change` skill.
