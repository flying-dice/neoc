# Testing AgentOS

Every app is tested headless, in Chromium, **without Electron or the main process**. The renderer is the system
under test. The backend is replaced by an in-memory host (`@agentos/sdk/testing`) that runs each app's real main
module (handlers, tools, storage) and scripted agents.

| Layer | What | Where | Command |
|---|---|---|---|
| Unit | pure logic (`src/shared`) | `*.test.ts` next to the code | `bun test` in the package |
| Stories | every app screen and state, in isolation | each app's own Storybook (`*.stories.svelte`) | `bun run storybook` in the package |
| Story checks | every story: renders cleanly, axe WCAG 2.1 AA, visual snapshot light + dark, no horizontal overflow at each size | `e2e/stories.spec.ts` (generic, picks up new stories automatically) | `bun run test:e2e --project=stories` |
| Desktop | the real desktop renderer in web mode: open every app from the Dock, resize, every layout, axe, snapshots | `e2e/desktop.spec.ts` (generic over the app registry) | `bun run test:e2e --project=desktop` |
| App journeys | an app's key user journeys (`JOURNEYS.md`), driven through test ids | `apps/<app>/e2e/*.spec.ts` | `bun run test:e2e --project=apps` |

Run everything with `bun run test:e2e` (it builds every Storybook first; `bun scripts/storybooks.ts list` shows
them). Update snapshots deliberately with `bun run test:e2e:update`, then open
the changed PNGs and look at them before committing.

## Test ids

Locators are `getByTestId` only. Every interactive element and every overlay has a stable `data-testid`, from the
first commit of a component. `bun run lint:testids` fails the build otherwise.

### Naming

- kebab-case: `<scope>-<thing>[-<qualifier>]`.
- **Scope** is the app id for anything inside an app (`calculator-key-7`, `settings-pane-appearance`), and the
  shell part for the desktop (`dock`, `status-bar`, `stage-strip`, `window-<appId>`).
- Collections use the item's stable id, never its index or its display text: `dock-calculator`,
  `agents-conversation-<id>`, `discover-listing-research`.
- Parts of a component append to its id: `window-calculator-resize`, `window-calculator-menu-fullscreen`,
  `settings-theme-dark`.

### Kit components

Every `@agentos/ui` component that is interactive, or opens an overlay, takes a **required** `testid: string` prop
and renders it as `data-testid` on its root interactive element. svelte-check therefore fails if a caller forgets
it. Inner parts derive from it (`${testid}-<part>`, `${testid}-<item id>`), documented in the component's header
comment.

### Raw elements

In any `.svelte` file, a `<button>`, `<a>`, `<input>`, `<select>`, `<textarea>`, an element with an `on…` pointer
or key handler, or one with an interactive `role` must carry `data-testid`.

## The in-memory host

`@agentos/sdk/testing` exports:

- `createMemoryHost({ apps, agents, platform })`: a complete `HostBridge` (what `window.agentos` provides in
  Electron). It runs app main modules in memory, routes `emit` to app renderers, validates tool input with zod,
  and plays scripted agent sessions.
- `scriptedAgent(...)`: a fake ACP agent that streams messages, tool calls and permission requests.
- `appApiFor(host, appId)`: the scoped `AppRendererApi` an app component gets, for stories.

An app's main module must be browser-safe (no `fs`, no `electron`), or the app provides a stand-in at
`src/mock/main.ts` and exports it as `./mock-main`.

## Web mode

`apps/desktop` builds a second renderer entry, `web.html`, that installs the memory host as `window.agentos` and
starts the real desktop. URL parameters set up a scenario:

| Param | Example | Effect |
|---|---|---|
| `open` | `open=calculator,settings` | open these apps (instead of Discover) |
| `theme` | `theme=dark` | force light/dark |
| `scenario` | `scenario=agents-busy` | seed agent conversations |

`bun run --filter @agentos/desktop dev:web` serves it for manual poking. Playwright starts it itself.

## Deterministic screenshots

Tests freeze the clock (`page.clock`), emulate `prefers-reduced-motion: reduce`, use a fixed viewport and device
scale factor 1, and wait for fonts. Baselines are per platform (`*-darwin.png`, `*-linux.png`).

## Tools for looking

- `node e2e/tools/contact-sheet.mjs out.png a.png b.png … [--columns 3]`: several screenshots in one labelled
  image, for reviewing baselines or diffs at a glance.
- `node e2e/tools/session.mjs`: a persistent headless browser you drive one command at a time (`look`, `click
  "<label>"`, `type`, `press`, `drag`, `resize`), the way a person would. Used for manual verification
  (`verify-change` skill) and by the UX research panel (`app-design` skill).
- `bun scripts/design-audit.ts <app>`: static check of an app's styles against the design tokens.
