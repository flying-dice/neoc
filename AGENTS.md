# Working on AgentOS

AgentOS is an iPadOS-style virtual desktop for agents: an Electron host (`apps/desktop`), app modules
(`apps/<app>`), the app SDK (`packages/sdk`) and the UI kit (`packages/ui`). Bun workspaces.

## Rules

1. **Design language.** UI follows `docs/design/DESIGN_LANGUAGE.md` (iPadOS, flat design, tokens only, WCAG AA).
2. **Atomic design.** Components live in `atoms/ molecules/ organisms/ templates/ pages/` and import only from
   lower layers (`bun run lint:atomic`).
3. **Test ids from the first commit.** Every interactive element and overlay has a `data-testid`; interactive kit
   components take a required `testid` prop (`bun run lint:testids`, `docs/TESTING.md`).
4. **Every app is tested headless with no backend.** Each app has its own Storybook with a story per screen and
   state, key journeys in `JOURNEYS.md` with matching specs in `e2e/`, and is covered by the generic story and
   desktop specs (axe, resize, layouts, snapshots). Run `bun run test:e2e`.
5. **Verify UI changes in the real renderer** before they land: the `verify-change` skill.
6. **Design work is iterative and researched**: the `app-design` skill (journeys, then the Haiku/Sonnet/Opus
   research panel via the `ux-research` workflow, then the conformance score, then design changes, until the exit
   criteria hold).

## Commands

```sh
bun run dev             # the Electron app
bun run dev:web         # the desktop in a browser on the in-memory host (http://localhost:6200/web.html)
bun run lint            # atomic layers + test ids + eslint
bun run typecheck
bun test                # unit tests
bun run test:e2e        # all Storybooks + desktop + journeys, headless (test:e2e:update to rebaseline)
bun run storybooks list # every app's Storybook (each package: bun run storybook)
bun scripts/design-audit.ts <app>   # static styleguide audit
```

## Adding an app

Follow `apps/README.md`, then also give the app, from its first commit: a `manifest.window` min size, a
`.storybook/` (two lines; copy `apps/calculator/.storybook`), stories for every screen (`tags: ['app']`, wrapped in
`AppWindow`), `JOURNEYS.md` plus `e2e/journeys.spec.ts`, and a browser-safe main module (or `src/mock/main.ts`). The
generic desktop and story specs pick the app up automatically.
