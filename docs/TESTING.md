# Testing AgentOS

Everything UI-facing runs headless in Chromium against the **real Rust host**. There is no in-memory host and no web
mode: the host starts in milliseconds, so the desktop and the journeys talk to the same binary a person runs. Logic is
tested where it lives, in Rust, with fakes for the ports.

| Layer | What | Where | Command |
|---|---|---|---|
| Rust | domain, use cases (against port fakes), adapters, the host end to end | `crates/*/src`, `crates/*/tests` | `cargo nextest run --workspace` |
| Client and kit unit | the renderer core (peer, expressions, ops, drafts, sources, offline), the shell's channel, kit logic | `*.test.ts` next to the code | `bun run test` (runs `bun test` in `client/` and `packages/ui`, and `bun test types`) |
| Luau types and format | every app and fixture type-checks strictly against the generated API; StyLua formatting | `scripts/lint-luau.ts`, `scripts/stylua.ts` | `bun run lint:luau`, `bun run lint:stylua` |
| Stories | every screen and state of the shell, the renderer and the kit, in isolation | `client/` and `packages/ui` Storybooks (`*.stories.svelte`) | `bun run storybook` in the package |
| Story checks | every story: renders, axe WCAG 2.1 AA, visual snapshot light and dark, no overflow | `e2e/stories.spec.ts` (generic: new stories are picked up) | `bun run test:e2e --project=stories` |
| Desktop | the real shell on the real host: every app from the Dock, resize, every layout, axe, snapshots | `e2e/desktop.spec.ts` (generic over built-in pages and `apps/*/app.json`) | `bun run test:e2e --project=desktop` |
| Journeys | key user journeys (`JOURNEYS.md`, `client/journeys/*.md`), driven through test ids | `apps/<app>/e2e/*.spec.ts`, `client/e2e/*.spec.ts`, `e2e/*.journeys.spec.ts` | `bun run test:e2e --project=journeys` |

`bun run test:e2e` runs all of them (it builds every Storybook first; `bun run storybooks list` shows them). Update
snapshots deliberately with `bun run test:e2e:update`, then open the changed PNGs (contact sheets:
`node e2e/tools/contact-sheet.mjs`) and look at them before committing.

## Rust

`cargo nextest run --workspace`, one convention per layer (the hexagon in `docs/architecture/ARCHITECTURE.md` §2):

- **Domain** (`agentos-domain`): plain unit tests, no framework, no I/O.
- **Application** (`agentos-application`): use cases tested against the in-memory port fakes in `agentos_application::testing`
  (`ManualClock`, `RecordingShell`, ...). Tests sit beside the code in `*_tests.rs`. No mocking library: write a fake.
- **Adapters**: integration tests in `crates/<adapter>/tests/`, through the real library (Luau sandbox and limits,
  SQLite migrations and live queries, MCP with an `rmcp` client, ACP against the scripted agent, `/rpc` with a
  WebSocket client, fetch against a local server). The shipped apps are tested here too: `crates/agentos-luau/tests/calculator_app.rs`
  loads `apps/calculator` under the real runtime and calls its tools.
- **Host** (`agentos-host/tests`): the real wiring on an ephemeral loopback port (`tests/common`), driven like a
  browser or an agent would: `/rpc`, `/agui`, `/mcp`, pairing, the Agents app.
- Tests may use `unwrap`, `expect` and `panic!` (`clippy.toml`); an integration-test file with helpers carries
  `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`. Name a test after the behaviour
  (`calculate_evaluates_exactly_and_rejects_what_is_not_arithmetic`), cover the failure path next to the success path,
  and keep each test independent (its own data directory, no fixed ports).
- A protocol or adapter change adds the test in the layer it touches and, if it crosses layers, one in the host.
- `cargo nextest run -p agentos-luau`, or a test name, runs a slice. `bun run lint:rust` is the other half of the Rust
  gate: `cargo fmt --check`, clippy with `-D warnings`, the crate-direction check (`bun run lint:deps`) and `cargo deny`.

## Client, kit and generated types

`bun test` (via `bun run test`) covers the client renderer core and shell in `client/src`, the kit's logic in
`packages/ui/src` (`keys`, `text`, `sheet`, `stepper`, ...) and `types/tests` (the generated types against samples).
They are plain-object tests: the renderer core is framework-free and runs against a TypeScript fake host on an in-memory
wire (`client/src/renderer/src/view/host/`). `bun run codegen:check` fails when `types/generated/*` or
`docs/reference/components.md` are stale against the Rust schema; `bun run codegen` regenerates them.

## Luau apps

- **Types:** `bun run lint:luau` runs `luau-lsp analyze` over every `.luau` file (apps, codegen fixtures, the generated
  `ui.luau`) against `types/generated/agentos.d.luau`, resolving `@agentos/app` and `@agentos/ui` through the root
  `.luaurc`. Files in `negative/` folders mark lines with `-- expect-error`; the check demands exactly those lines
  are rejected, so a broken definitions file cannot pass silently.
- **Format:** `bun run lint:stylua` (`stylua.toml`, `.styluaignore`); `bun run format` rewrites. Both lints print a
  `SKIPPED` message when `luau-lsp` (PATH or `~/.local/bin`) or `stylua` (PATH or `~/.cargo/bin`) is not installed:
  install them, a skipped lint is not a pass.
- **Logic and tools:** tested from Rust against the shipped package (see "Rust"); views and journeys against the real
  host (below).

## Storybook and the fake hosts

Two Storybooks exist: `client/` (the shell's pages, the pairing screen, and the renderer: "Renderer/Kit - layout",
"Renderer/Kit - controls", "Renderer/Chat transcript", "Renderer/Notes window", all driven by **JSON view trees** on
a fake host) and `packages/ui` (each kit component and its states). Stories are `*.stories.svelte` with Svelte CSF.

- **Shell pages** run against `client/src/renderer/src/shell/fakeHost.ts`, a fake of the host's management methods
  (`app.*`, `host.devices.*`) with the host's own answers and error messages, keeping state so install, disable, remove
  and revoke behave end to end.
- **Renderer states** run against the fake hosts in `client/src/renderer/src/view/host/` (`TreeApp`, `KitApp`,
  `ChatApp`, the fake AG-UI): a tree, ops applied, a two-way binding, a windowed list, a conflict, offline, reconnect.
- A story that shows a deliberately clipped state tags itself `overflow-ok`. Stories tagged `app` (an app screen inside the
  `AppWindow` frame) also run at a minimum and a large window; Luau apps have no Storybook of their own, because the
  `desktop` project already opens every installed app at those sizes in the real shell.
- The real host is exercised only by the Playwright desktop and journey projects.

## The end-to-end host

`playwright.config.ts` starts the host itself (`e2e/host/start.ts`):

1. builds the client (`client/dist`), the host (`cargo build -p agentos-host`), the scripted ACP agent
   (`cargo build -p agentos-acp --bin fake_agent`) and the apps into a **test registry** (`bun scripts/build-apps.ts`:
   every `apps/*/app.json`, plus Notes, the codegen fixture, as a second app the Discover journeys install and remove
   without disturbing the Calculator);
2. writes `agentos.toml` for it: port **7410** (never the default 7400, so a host you have running is untouched; from
   `e2e/host/config.ts`, planned to derive from one env var, `AGENTOS_PORT_BASE`, with the Storybook port), loopback
   only, a fresh data directory under `test-results/host/`, the test registry as `registries`, the Calculator and
   Agents as `install` (Notes is only offered), and one agent on the roster;
3. runs `agentos-host` in the foreground; Playwright stops it with SIGTERM.

**The scripted agent.** The roster's agent is `scripted`, a real ACP process with no model
(`crates/agentos-acp/src/bin/fake_agent.rs`): the prompt text picks what it does (`hello` streams a short reply, `think`
adds a thought, `tool` and `fail` make tool calls, `ask` and `ask-write` request permission, `state` sends commands, usage
and a plan, `slow` streams until cancelled, `exit` crashes; anything else is echoed). Its approval policy is `ask` and it
is granted the Calculator. The Agents journeys (`apps/agents/e2e`) talk to it over the real `/agui`.

**Pairing in tests.** Every client route needs a paired device, including from loopback. `e2e/host/global-setup.ts`
**pairs one browser** the way a person does: `agentos-host pair --config ...` prints a code (it lands in the shared
system database, so the running host accepts it), `POST /pair` trades it for the session cookie, and the cookie is saved
as the storage state every desktop and journey spec shares. Always use `http://127.0.0.1:<port>` (the cookie is
host-only, so `localhost` would be a different device). The pairing journeys (`e2e/pairing.journeys.spec.ts`) start unpaired
instead. `e2e/host/pair.ts` has `issuePairingCode()`, `pairDevice(name)` (a second device) and `callTool(name, args)`:
an agent calling an app's tool over MCP with the token the host wrote to `<data>/mcp.json`. That is how the Calculator's
J5 "an agent puts a result on the Calculator" is driven, with the open window updating live.

Rules that follow from one shared host:

- **The Calculator is one calculator for the whole host**, so everything that presses its keys runs one test after
  another (`describe.configure({ mode: 'serial' })`), clears it first, and the `journeys` project runs after
  `desktop`. Use Notes (offered, not installed) for anything that installs or removes an app, and a second paired
  device for anything that revokes one: never the shared browser's own device or the Calculator.
- Pairing attempts are rate limited (5 wrong codes a minute): the pairing journeys make at most two wrong guesses, and
  rate limiting itself is covered by the host's pairing tests and the "Too many attempts" story.
- Do not run two `test:e2e` at once on one machine: they would fight over the host and Storybook ports. Run the
  suite in one worktree at a time.

## Test ids

Locators are `getByTestId` only. Every interactive element and every overlay has a stable `data-testid`, from the
first commit. `bun run lint:testids` fails the build for raw Svelte elements and kit components without one.

### Naming

- kebab-case: `<scope>-<thing>[-<qualifier>]`.
- **Scope** is the app id for anything inside an app, and the shell part for the desktop (`dock`, `status-bar`,
  `stage-strip`, `window-<appId>`, `pairing`).
- **A Luau app's window** gets its ids from node keys: `<appId>-<explicit keys on the path>` (`calculator-key-7`,
  `calculator-display`; `client/src/renderer/src/view/core/testid.ts`). Set a `key` on every interactive node; the host
  rejects an interactive node without one. Auto keys, slot names and the window root are skipped, so wrapping a node in a
  stack never renames it; a row template contributes `row-<rowKey>`.
- Collections use the item's stable id, never its index or its display text: `dock-calculator`,
  `discover-app-notes`, `settings-device-<id>`.
- Parts of a component append to its id: `window-calculator-resize`, `window-calculator-menu-fullscreen`,
  `discover-app-notes-remove`.

### Kit components

Every `@agentos/ui` component that is interactive, or opens an overlay, takes a **required** `testid: string` prop
and renders it as `data-testid` on its root interactive element, so svelte-check fails if a caller forgets it. Inner
parts derive from it (`${testid}-<part>`, `${testid}-<item id>`), documented in the component's header comment. The
renderer adapters pass the id derived from the node's key path.

### Raw elements

In any `.svelte` file, a `<button>`, `<a>`, `<input>`, `<select>`, `<textarea>`, an element with an `on...` pointer or
key handler, or one with an interactive `role` must carry `data-testid`.

## Deterministic screenshots, baselines and contact sheets

Tests freeze the clock (`page.clock`, 09:41), emulate `prefers-reduced-motion: reduce`, use a fixed 1280x800 viewport and
device scale factor 1, and wait for fonts and two animation frames. `?theme=light|dark` forces the appearance (the specs
use it). The Calculator's display is "0" in every desktop shot: nothing in `desktop.spec.ts` presses a key.

**Baselines are macOS-only** (`e2e/__screenshots__/**/*-darwin.png`): run `test:e2e` on a Mac. A change that should alter
pixels is rebaselined with `bun run test:e2e:update`, and the changed PNGs are looked at, never just committed. The
threshold is `maxDiffPixelRatio: 0.002`; do not raise it to get green.

`node e2e/tools/contact-sheet.mjs out.png a.png b.png ... [--columns 3] [--width 1800]` lays several screenshots in one
labelled image, for reviewing a set of baselines or before/after pairs at a glance. Open the result with the Read tool.

## Driving the real shell: `e2e/tools/session.mjs`

A persistent headless browser you drive one command at a time, the way a person uses the app. It is what `verify-change`
and the UX research panel (`app-design`, `ux-research`) use. It needs a running host that serves the built client:

```sh
bun run build && bun run build:apps                       # the client and the app refs (dist/apps)
AGENTOS_DATA=$(mktemp -d) AGENTOS_PORT=7420 \
  AGENTOS_INSTALL="file://$PWD/dist/apps/calculator/0.1.0/app-ref.json,file://$PWD/dist/apps/agents/0.1.0/app-ref.json" \
  bun run host                                            # prints the URL and "Pairing code: K7QM-3XHR"
node e2e/tools/session.mjs start me --url http://127.0.0.1:7420/ [--theme dark] [--width 1280 --height 800]
node e2e/tools/session.mjs fill me "Pairing code" K7QM-3XHR      # the first screen
node e2e/tools/session.mjs click me "Pair"
node e2e/tools/session.mjs look me                        # screenshot path (open it with Read) + accessibility tree
node e2e/tools/session.mjs click me "<label>" | --at x,y   # by accessible name, then visible text
node e2e/tools/session.mjs type me "text"; press me Tab; drag me x1,y1 x2,y2; scroll me 0,400; resize me 1024 768
node e2e/tools/session.mjs stop me
```

Commands match labels by accessible name first, so a missing label is a finding. `look` also prints console errors.
Sessions live in `test-results/sessions/<name>/`. `window.__agentos` exposes the shell's host state, window manager
(`wm.open('calculator')`) and app registry. Another pairing code any time: `bun run host pair` with the same
`AGENTOS_DATA`. To drive the e2e host instead, keep `bun run test:e2e` running and point `--url` at its port; pair with
`issuePairingCode()` from `e2e/host/pair.ts`.

## Known gotchas

- **Storybook 10's final render phase is `finished`**, not `completed`; the stories spec waits for either.
- **Svelte transitions ignore `prefers-reduced-motion`.** Wrap durations in `motion()` (`packages/ui/src/motion.ts`) or
  snapshots flake.
- **Play functions click programmatically**, which leaves a focus ring a pointer would not: the stories spec blurs the
  active element before the snapshot. A story that depends on focus waits for it (the List reorder story flaked under
  parallel load until it did).
- **Notifications can overtake responses on `/rpc`.** Never assume a notification arrives after the response of the request
  that caused it; the client and the specs wait for the state, not the order.
- **Contrast.** Stock iOS blue, red and secondary label fail WCAG AA; the tokens use the higher-contrast variants on
  purpose. When axe flags contrast, check the token before the component.
- **One host, one cookie.** The session cookie is host-only: mixing `localhost` and `127.0.0.1` pairs a second device and
  breaks everything shared. `reuseExistingServer` is off for the host, so a stale process on the port fails the run.
- **Shared state.** Specs that touch the Calculator, install or remove apps, or revoke devices must follow the rules under
  "The end-to-end host", or they poison the specs after them.
- **First run is slow.** The webServer builds the client, the host and every Storybook (a 600 s timeout). Later runs are
  incremental.
- **The `.claude/` worktrees** carry copies of every spec; `playwright.config.ts` ignores them so only this checkout's specs
  run.
- A flaky test is a bug in the test or the app. Do not retry it into green or skip it; find the missing wait or the race.

## Static audits

`bun scripts/design-audit.ts <page>` checks a built-in shell page's styles (and the components it imports) against the
design tokens. Luau apps carry no CSS: apps pick semantic tokens only, so their conformance is checked by the generic
axe and overflow specs, the baselines and a live look (see the `app-design` skill).
