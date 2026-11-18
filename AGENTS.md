# Working on AgentOS

AgentOS is an iPadOS-style virtual desktop for agents. It has three parts:

- **The host** (`crates/`): a Rust program, one Cargo workspace laid out as a hexagon. It runs the apps, owns their
  data, serves the client, and speaks JSON-RPC 2.0 over WebSocket at `/rpc`, AG-UI over HTTP + SSE at `/agui` (agent
  chats), MCP at `/mcp` (the tools agents call) and ACP to the agent processes it launches. It binds the LAN and every
  client must pair as a device first.
- **The apps** (`apps/<id>/`): server-side **Luau** (`app.json` + `*.luau`), one sandboxed VM per app, loaded from an
  app ref over `file://`, `http://` or `https://`. An app holds its state, data, tools, notifications and UI
  description. Examples: `apps/calculator`, `apps/agents`.
- **The client** (`client/`): a managed Vite + Svelte 5 PWA. It renders the view trees the host sends with the kit in
  `packages/ui` (atomic layers) through adapters in `client/src/renderer/src/view/`, holds client-managed state, and
  runs **no app code**.

Bun workspaces cover `client` and `packages/*` (the `apps/*` glob matches no package: an app is Luau, `app.json` + `*.luau`, not a Bun package); Cargo covers `crates/*`.

## Sources of truth

| For | Read |
|---|---|
| The platform: shape, crates, decisions | `docs/architecture/ARCHITECTURE.md` |
| The host: ports, adapters, pairing, config, `/agui`, tool permissions | `docs/architecture/host-stack.md` |
| The server-driven UI: view protocol, bindings, state, the Luau authoring API | `docs/architecture/ui-system.md`, `docs/reference/components.md` (generated) |
| The client: shell, renderer, adding a component | `client/README.md` |
| Writing an app: manifest, `app.*` API, security, adding an app | `apps/README.md`, `types/generated/agentos.d.luau` (the typed API) |
| Tests, test ids, headless runs | `docs/TESTING.md` |
| Design language, WCAG AA, §11 checklist | `docs/design/DESIGN_LANGUAGE.md` |

`types/generated/*` and `docs/reference/components.md` are generated from the Rust schema (`bun run codegen`); never
edit them by hand.

## Merge policy

- All work happens on a branch and lands through a PR. Never push to `main`.
- Nothing merges without the full gate green locally on the head commit (lint, typecheck, test, e2e; there is no
  hosted CI) and an `agentos-code-review` verdict of **READY TO MERGE**.
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
5. For a new app or a change people will notice in one: the readiness gate in `agentos-code-review` §6 holds,
   including a `docs/ux/<app>/round-<n>.md` that meets the `app-design` exit criteria.

## Boundaries

- **Crate dependency direction** (hexagonal; inward only): `agentos-domain` is pure and depends on no other crate;
  `agentos-application` (ports and use cases) depends on domain; each adapter (`luau`, `sqlite`, `mcp`, `acp`, `agui`,
  `fetch`, `http`, `codegen`) depends on application and domain and **never on another adapter**; `agentos-host` is the
  composition root and the only crate that sees them all. Enforced by `bun run lint:deps` (`scripts/check-deps.ts`,
  an allow-list over `cargo metadata`) and `cargo deny check` (`deny.toml`: each framework crate is pinned to its
  adapter). A new crate must be placed in the allow-list on purpose.
- **Rust hygiene** (`[workspace.lints]`): clippy pedantic with `-D warnings`; `unsafe_code` is forbidden;
  `unwrap`, `expect` and `panic!` are denied outside tests; `thiserror` in library crates, `anyhow` only in the host
  binary.
- **The Luau sandbox is the app boundary.** An app reaches the host only through the `app` table
  (`types/generated/agentos.d.luau`); it has no filesystem, no network, no `io`/`os`/`debug`, and `require` loads only
  files of its own package. Its capabilities come from the `permissions` in its `app.json` (`data`, `notify`, `shell`,
  `agents`) and are enforced in the host, not trusted from the app. Memory, CPU and host-call time are budgeted; SQL
  `ATTACH` is denied. A tool declared `readOnly` really cannot write.
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
leaves its package (the client never reaches into `crates/`), the client and kit import only `@agentos/ui` (stories
also `@agentos/storybook`), no Node or Bun builtins in browser code, `new WebSocket` only in
`client/src/renderer/src/view/rpc/websocket.ts`, and no `{@html}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML` or
`document.write`.

Also: atomic layers (`lint:atomic`, in `packages/ui` and `client/src`) and test ids from the first commit
(`lint:testids`).

## Skills: when to use which

| When | Skill |
|---|---|
| Writing or changing code (Rust, Luau, Svelte/TypeScript) | `coding-styleguide` |
| Before pushing | `clean-code-review`, then `pre-commit` (loops until clean; `refactor` clears markers) |
| Every MR, and before calling a branch ready | `agentos-code-review` (verdict: READY TO MERGE or STAYS IN MR) |
| Any change to what a person sees or does | `verify-change` |
| Before building a new app (or re-scoping one) | `prebuild-synthetic-ux` (personas, riskiest assumptions, key journeys and stories → `docs/ux/<app>/discovery.md`) |
| Designing or redesigning an app | `app-design` (discovery, journeys, usability review, the `ux-research` panel, conformance, iterate) |
| Reviewing the usability of something built | `postbuild-synthetic-usability` (task walkthroughs on the real host, heuristics and WCAG → `docs/ux/<app>/review-<n>.md`) |

## Commands

```sh
bun run dev             # build:apps, then the host on :7400 (dev_reload) + Vite on :5173; pair with the code the host prints
bun run build:apps      # package every apps/*/app.json into app refs + registry.json under dist/apps
bun run build           # build the client (client/dist)
bun run host            # cargo run -p agentos-host: serves client/dist, /rpc, /agui, /mcp on :7400
bun run host pair       # print a fresh device pairing code (every client must pair; see host-stack.md)
bun start               # the same host, release build
bun run lint            # conflict markers + atomic layers + test ids + boundaries + rust + luau + stylua + codegen:check + package eslint
bun run lint:conflicts  # leftover merge-conflict markers in tracked files (runs first)
bun run lint:rust       # cargo fmt --check, clippy -D warnings, lint:deps (crate direction), cargo deny
bun run lint:luau       # luau-lsp analyze of every app against types/generated (strict, plus a negative fixture)
bun run lint:stylua     # StyLua check (skipped with a message when luau-lsp or stylua is not installed)
bun run typecheck       # cargo check --workspace --all-targets + tsc on types/ + svelte-check in each package
bun run test            # cargo nextest run --workspace + bun test (types, scripts, client, kit)
bun run format          # prettier + cargo fmt + StyLua
bun run test:e2e        # all Storybooks + desktop + journeys, headless, against the real host
                        # AGENTOS_PORT_BASE=9100 for a second worktree (Storybook = base, e2e host = base + 10)
bun run test:e2e:update # rebaseline snapshots (then look at them)
bun run storybooks list # every Storybook (client, packages/ui); each package: bun run storybook
bun run codegen         # regenerate types/generated/* and docs/reference/components.md from the Rust schema
bun run codegen:check   # fails when they are stale (part of bun run lint)
bun scripts/design-audit.ts <page>  # static styleguide audit of a built-in shell page's styles
```

Visual baselines (`e2e/__screenshots__`) are macOS-only, so run `test:e2e` on a Mac. There is no hosted CI: the
`pre-commit` skill runs the gate and `agentos-code-review` checks it. Needed tools: Rust (the pinned toolchain),
`cargo-nextest`, `cargo-deny`, Bun, and, for the Luau checks, `luau-lsp` and `stylua`.

## Adding an app

Follow `apps/README.md` → "Adding an app". From its first commit the app has an `app.json`, an `app.manifest`
with a `window` min size (and a comment when the layout needs more than 320x240), a `JOURNEYS.md` plus
`e2e/journeys.spec.ts`, and a Rust test of its logic and tools next to the others in `crates/agentos-luau/tests`.
The generic `desktop` project opens every `apps/*/app.json` in the real shell (sizes, layouts, light and dark, axe,
snapshots) with no extra code; each screen and state beyond the landing view is reached by a journey, or, when a
journey can't reach it cheaply, shown by a JSON-tree story in the client Storybook. The app is `luau-lsp`-strict and
StyLua clean from the first commit. Then design it with `app-design` until the exit criteria hold; until then it
stays in its MR.
