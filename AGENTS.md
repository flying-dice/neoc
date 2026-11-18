# Working on AgentOS

AgentOS is an iPadOS-style virtual desktop for agents: a Bun server (`apps/server`), a PWA client (`client`),
app modules released as bundles and installed from the server's known-apps list
(`apps/<app>`), the app SDK (`packages/sdk`) and the UI kit (`packages/ui`). Bun workspaces.

Sources of truth: `apps/README.md` (app model, SDK, security), `docs/TESTING.md` (test ids, headless testing),
`docs/design/DESIGN_LANGUAGE.md` (design language, WCAG AA, §11 checklist).

## Merge policy

- All work happens on a branch and lands through a PR. Never push to `main`.
- Nothing merges without the full gate green locally on the head commit (lint, typecheck, test, e2e; there is no
  hosted CI) and an `agentos-code-review` verdict of **READY TO MERGE**.
- An app that isn't ready for the full gate and the UX research panel stays in its MR. Keep pushing to the
  branch; don't merge a partial app behind a flag or with "tests later".

## Definition of done

A change is done when all of these hold:

1. `bun run lint && bun run typecheck && bun run test && bun run test:e2e` pass.
2. New behaviour has new checks: bun tests for logic, a story per new screen or state, a journey spec per new or
   changed journey. Changed snapshots were looked at.
3. Every interactive element has a `data-testid`; it works by keyboard, in light and dark, and at the app's
   minimum window.
4. UI changes carry a `verify-change` "Manual verification" record in the PR.
5. For a new app or a change people will notice in one: the readiness gate in `agentos-code-review` §6 holds,
   including a `docs/ux/<app>/round-<n>.md` that meets the `app-design` exit criteria.

## Boundaries

Enforced by `bun run lint:boundaries` (`scripts/check-boundaries.ts`):

- An app's client imports only `@agentos/sdk` (root, `app/client`), `@agentos/ui`, `svelte`, `zod` and its own files;
  its server may also use `bun:*`, `node:*` and its declared npm deps. Never host internals or another app.
- Inside an app, `src/client` and `src/server` talk only through `api.invoke` / `ctx.handle` / `ctx.emit`;
  `src/shared` is pure logic.
- Client ↔ server traffic goes only through the SDK protocol (JSON-RPC over `/rpc`): no raw `WebSocket`, no extra
  `Bun.serve` routes. The server never imports app packages: apps arrive as bundles (installed by id from the known-apps list).
- Server modules are registered/unregistered at runtime: release what they open in their dispose function.
- Agent and tool output is untrusted: no `{@html}` or `innerHTML`.

Missing a capability? Extend the SDK (contract, zod schema, server and memory host), never a side channel.
Also: atomic layers (`lint:atomic`) and test ids from the first commit (`lint:testids`).

## Skills: when to use which

| When | Skill |
|---|---|
| Writing or changing code | `coding-styleguide` |
| Before pushing | `clean-code-review`, then `pre-commit` (loops until clean; `refactor` clears markers) |
| Every MR, and before calling a branch ready | `agentos-code-review` (verdict: READY TO MERGE or STAYS IN MR) |
| Any change to what a person sees or does | `verify-change` |
| Designing or redesigning an app | `app-design` (journeys, the `ux-research` panel, conformance, iterate) |

## Commands

```sh
bun run dev             # server on :7400 + client on :5173 (builds dist/bundles first)
bun run build:apps      # dev build of app bundles → dist/bundles (mutable; the dev server hot-reloads them)
bun run release:apps    # release build → bundles/ (immutable, committed); bump the app's version to change a released app
bun run build && bun start   # production: the server serves the built PWA on :7400
bun run host            # the Rust host on :7400 (pairing code on stdout); needs `bun run build` and `bun run build:apps` first
bun run host            # the Rust host (cargo run -p agentos-host): serves the client, /rpc, /agui, /mcp on :7400
bun run host pair       # print a fresh device pairing code (every client must pair; see docs/architecture/host-stack.md)
bun run lint            # atomic layers + test ids + boundaries + eslint + cargo fmt/clippy/deny + crate deps + luau-lsp + stylua
bun run typecheck       # + cargo check --workspace --all-targets
bun run test            # cargo nextest run --workspace + unit tests in every package
bun run format          # prettier + cargo fmt + stylua
bun run test:e2e        # all Storybooks + desktop + journeys, headless (test:e2e:update to rebaseline)
bun run storybooks list # every app's Storybook (each package: bun run storybook)
bun scripts/design-audit.ts <app>   # static styleguide audit
bun run codegen         # regenerate types/generated/* and docs/reference/components.md from the Rust schema
bun run codegen:check   # fails when they are stale (part of bun run lint)
```

Visual baselines (`e2e/__screenshots__`) are macOS-only, so run `test:e2e` on a Mac. There is no hosted CI: the
`pre-commit` skill runs the gate and `agentos-code-review` checks it.

## Adding an app

Follow `apps/README.md` → "Adding an app". From its first commit the app has a `manifest.window` min size, its own
`.storybook/` (copy `apps/calculator/.storybook`), a story per screen and state (`tags: ['app']`, inside
`AppWindow`), `JOURNEYS.md` plus `e2e/journeys.spec.ts`, and a browser-safe server module (or `src/mock/server.ts`).
Then design it with `app-design` until the exit criteria hold; until then it stays in its MR.
