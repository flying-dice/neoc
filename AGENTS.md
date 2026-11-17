# Working on AgentOS

AgentOS is an iPadOS-style virtual desktop for agents: an Electron host (`apps/desktop`), app modules
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

- An app imports only `@agentos/sdk` (root, `app/main`, `app/renderer`), `@agentos/ui`, `svelte`, `zod` and its own
  files. Never `electron`, Node builtins, `apps/desktop`, another app or `@agentos/sdk/host/*`.
- Inside an app, `src/renderer` and `src/main` talk only through `api.invoke` / `ctx.handle` / `ctx.emit`;
  `src/shared` is pure logic.
- Renderer ↔ main traffic goes only through the SDK protocol: no raw `ipcMain` / `ipcRenderer` /
  `webContents.send` outside `packages/sdk/src/host` and the desktop composition roots.
- Agent and tool output is untrusted: no `{@html}` or `innerHTML`. The renderer stays sandboxed.

Missing a capability? Extend the SDK (contract, zod schema, desktop host and memory host), never a side channel.
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
bun run dev             # the Electron app
bun run dev:web         # the desktop in a browser on the in-memory host (http://localhost:6200/web.html)
bun run lint            # atomic layers + test ids + boundaries + eslint
bun run typecheck
bun run test            # unit tests in every package
bun run test:e2e        # all Storybooks + desktop + journeys, headless (test:e2e:update to rebaseline)
bun run storybooks list # every app's Storybook (each package: bun run storybook)
bun scripts/design-audit.ts <app>   # static styleguide audit
```

Visual baselines (`e2e/__screenshots__`) are macOS-only, so run `test:e2e` on a Mac. There is no hosted CI: the
`pre-commit` skill runs the gate and `agentos-code-review` checks it.

## Adding an app

Follow `apps/README.md` → "Adding an app". From its first commit the app has a `manifest.window` min size, its own
`.storybook/` (copy `apps/calculator/.storybook`), a story per screen and state (`tags: ['app']`, inside
`AppWindow`), `JOURNEYS.md` plus `e2e/journeys.spec.ts`, and a browser-safe main module (or `src/mock/main.ts`).
Then design it with `app-design` until the exit criteria hold; until then it stays in its MR.
