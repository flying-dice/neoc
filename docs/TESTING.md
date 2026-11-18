# Testing AgentOS

Everything runs headless in Chromium against the **real Rust host**. There is no in-memory host and no web mode: the
host starts in milliseconds, so the desktop and the journeys talk to the same binary a person runs.

| Layer | What | Where | Command |
|---|---|---|---|
| Rust | domain, application (against port fakes), adapters, the host end to end | `crates/*/src`, `crates/*/tests` | `cargo nextest run --workspace` |
| Client unit | the renderer core, the shell's channel, pairing | `*.test.ts` next to the code | `bun test` in `client/` |
| Stories | every screen and state of the shell and of the kit, in isolation | each package's own Storybook (`*.stories.svelte`) | `bun run storybook` in the package |
| Story checks | every story: renders, axe WCAG 2.1 AA, visual snapshot light + dark, no horizontal overflow at each size | `e2e/stories.spec.ts` (generic, picks up new stories automatically) | `bun run test:e2e --project=stories` |
| Desktop | the real shell on the real host: open every app from the Dock, resize, every layout, axe, snapshots | `e2e/desktop.spec.ts` (generic over the built-in pages and `apps/*/app.json`) | `bun run test:e2e --project=desktop` |
| Journeys | key user journeys (`JOURNEYS.md`, `client/journeys/*.md`), driven through test ids | `apps/<app>/e2e/*.spec.ts`, `client/e2e/*.spec.ts`, `e2e/*.journeys.spec.ts` | `bun run test:e2e --project=journeys` |

Run everything with `bun run test:e2e` (it builds every Storybook first; `bun scripts/storybooks.ts list` shows
them). Update snapshots deliberately with `bun run test:e2e:update`, then open the changed PNGs (contact sheets:
`node e2e/tools/contact-sheet.mjs`) and look at them before committing.

## The end-to-end host

`playwright.config.ts` starts the host itself (`e2e/host/start.ts`):

1. builds the client (`client/dist`), the host (`cargo build -p agentos-host`) and the apps into a **test registry**
   (`bun scripts/build-apps.ts`: every `apps/*/app.json`, plus Notes, the codegen fixture, as a second app that the
   Discover journeys install and remove without disturbing the Calculator);
2. writes `agentos.toml` for it: port **7410** (never the default 7400, so a host you have running is untouched),
   loopback only, a fresh data directory under `test-results/host/`, the test registry as `registries`, and the
   Calculator as `install` (Notes is only offered);
3. runs `agentos-host` in the foreground; Playwright stops it with SIGTERM.

Then `e2e/host/global-setup.ts` **pairs one browser** the way a person does: `agentos-host pair --config ...` prints a
code (it lands in the shared system database, so the running host accepts it), `POST /pair` trades it for the session
cookie, and the cookie is saved as the storage state every desktop and journey spec shares. Always use
`http://127.0.0.1:7410` (the cookie is host-only). The pairing journeys start unpaired instead.

`e2e/host/pair.ts` also has `pairDevice(name)` (a second device) and `callTool(name, args)`: an agent calling an app's
tool over MCP with the token the host wrote to `<data>/mcp.json`. That is how the Calculator's J5 "an agent puts a result
on the Calculator" is driven, with the open window updating live.

Rules that follow from one shared host:

- **The Calculator is one calculator for the whole host**, so everything that presses its keys runs one test after
  another (`describe.configure({ mode: 'serial' })`), clears it first, and the `journeys` project runs after
  `desktop`. Use Notes (offered, not installed) for anything that installs or removes an app, and a second paired
  device for anything that revokes one: never the shared browser's own device or the Calculator.
- Pairing attempts are rate limited (5 wrong codes a minute): the pairing journeys make at most two wrong guesses, and
  rate limiting itself is covered by the host's pairing tests and the "Too many attempts" story.

## Test ids

Locators are `getByTestId` only. Every interactive element and every overlay has a stable `data-testid`, from the
first commit of a component. `bun run lint:testids` fails the build otherwise.

### Naming

- kebab-case: `<scope>-<thing>[-<qualifier>]`.
- **Scope** is the app id for anything inside an app, and the shell part for the desktop (`dock`, `status-bar`,
  `stage-strip`, `window-<appId>`, `pairing`).
- **A Luau app's window** gets its ids from node keys: `<appId>-<explicit keys on the path>` (`calculator-key-7`,
  `calculator-display`; `client/src/renderer/src/view/core/testid.ts`). Set a `key` on every interactive node.
- Collections use the item's stable id, never its index or its display text: `dock-calculator`,
  `discover-app-notes`, `settings-device-<id>`.
- Parts of a component append to its id: `window-calculator-resize`, `window-calculator-menu-fullscreen`,
  `discover-app-notes-remove`.

### Kit components

Every `@agentos/ui` component that is interactive, or opens an overlay, takes a **required** `testid: string` prop
and renders it as `data-testid` on its root interactive element. svelte-check therefore fails if a caller forgets
it. Inner parts derive from it (`${testid}-<part>`, `${testid}-<item id>`), documented in the component's header
comment.

### Raw elements

In any `.svelte` file, a `<button>`, `<a>`, `<input>`, `<select>`, `<textarea>`, an element with an `on…` pointer
or key handler, or one with an interactive `role` must carry `data-testid`.

## Stories and the fake host

The shell's pages (Discover, Settings, the pairing screen, the desktop) are tested in Storybook against
`client/src/renderer/src/shell/fakeHost.ts`: a fake of the host's management methods (`app.*`, `host.devices.*`) with
the host's own answers and error messages, keeping its own state so install, disable, remove and revoke behave end to
end. The real host is only exercised by the Playwright journeys. The Agents page still uses the old SDK in-memory
host in its stories until wave B3 replaces it (`client/.storybook/host.ts`).

## Deterministic screenshots

Tests freeze the clock (`page.clock`), emulate `prefers-reduced-motion: reduce`, use a fixed viewport and device
scale factor 1, and wait for fonts. Baselines are per platform (`*-darwin.png`, `*-linux.png`). The Calculator's
display is "0" in every desktop shot: nothing in `desktop.spec.ts` presses a key. `?theme=light|dark` forces the
appearance (the specs use it).

## Tools for looking

- `node e2e/tools/contact-sheet.mjs out.png a.png b.png … [--columns 3]`: several screenshots in one labelled
  image, for reviewing baselines or diffs at a glance.
- `node e2e/tools/session.mjs`: a persistent headless browser you drive one command at a time (`look`, `click
  "<label>"`, `type`, `press`, `drag`, `resize`), the way a person would. Used for manual verification
  (`verify-change` skill) and by the UX research panel (`app-design` skill). Point it at a running host
  (`bun run host`, or the e2e host) and pair first.
- `bun scripts/design-audit.ts <app>`: static check of an app's styles against the design tokens.
