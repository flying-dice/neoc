---
name: verify-change
description: Use before finishing, opening or merging ANY vos change that alters what a person sees or does (kit components in packages/ui, renderer adapters and the shell in client/, Luau app windows, window management, menus, dialogs, layout, theming, agent tools that move the UI, host changes that change a view or protocol message). Run the real shell and the real Rust host headless, exercise the change, look at the screenshots against the intended result and the design language, and write down what was checked and how. Passing unit tests is not enough.
---

# Verify a change in the real UI before it lands

Tests check what someone thought to assert. They don't see that a menu covers its own label, that a sheet is cut
off in a small window, or that dark mode turned a title black on a dark bar. So **the agent that makes a UI change
runs the real shell, uses the changed behaviour, looks at the screenshots and says what it checked.** There are
no exceptions for "small" kit changes: a kit change touches every app.

Everything runs headless against the **real Rust host**: the shell in Chromium, the Luau apps on the host, one paired
browser. The e2e run starts its own host on port 7410 (`VOS_PORT_BASE` + 10; set the base to run beside another worktree), so it never collides with
a host you have running on 7400. Section 7 says how the e2e host works.

## 1. Write down the intended result first

Before you look, write 3 to 8 lines in a person's terms, for example:

- "The multitasking menu opens centred under the ··· handle, fully inside the window, in both themes."
- "At the Calculator's minimum window (320x500) every key is a full circle of at least 44 px; nothing is clipped."

List **every place the change shows up**. For a kit component, find its renderer adapter and the apps that use it:
`grep -rn "<ComponentName" client/src packages/ui/src` and `grep -rn "ui.ComponentName" apps`. Pick at least one real app
(or the shell page) per variant. For a host change, find the views, tools or protocol messages it alters.

## 2. Run the checks that cover it

```sh
bun run lint            # atomic layers, test ids, clippy, crate direction, luau-lsp, StyLua, codegen:check
bun run typecheck       # includes the required `testid` props on kit components
bun run test            # cargo nextest over the vos crates, then the client, kit and types unit tests
bun run test:e2e        # builds every Storybook, starts the real host, then:
                        #   stories  every story: axe light+dark, overflow, snapshots
                        #   desktop  every app on the real host: Dock, floating light/dark, resize, full screen/split/slide over, iPad & large screens
                        #   journeys each app's and the shell's key journeys (apps/<app>/e2e, client/e2e, e2e/*.journeys.spec.ts)
```

Run a slice with `bunx playwright test --project=desktop --grep calculator` (it starts the host itself). `test:e2e` needs a
Mac for the baselines and must not run beside another `test:e2e` (shared ports).

When a change is intended to alter visuals, update the baselines (`bun run test:e2e:update`) and then go to step 4.
**A snapshot update with no look at the images is not verification.**

## 3. Exercise it like a person

Cover what the automated suite can't see yet. Run the host and drive the shell yourself (this is "driving the real shell";
`app-design`, `postbuild-synthetic-usability` and the `ux-research` workflow use it too):

```sh
bun run build && bun run build:apps                       # client/dist and the app refs in dist/apps
# the newest built version of an app (dist/apps/<app>/<version>/app-ref.json), whatever its version is
ref() { echo "file://$(ls -d "$PWD/dist/apps/$1"/*/ | sort -V | tail -1)app-ref.json"; }
VOS_DATA=$(mktemp -d) VOS_PORT=7420 \
  VOS_INSTALL="$(ref calculator),$(ref agents)" \
  bun run host                                            # prints "Pairing code: K7QM-3XHR"; leave it running (run_in_background)
node e2e/tools/session.mjs start me --url "http://127.0.0.1:7420/?theme=dark"
node e2e/tools/session.mjs pair me K7QM-3XHR      # every device pairs first, even on loopback (fills "Pairing code", clicks "Pair")
node e2e/tools/session.mjs look me                # screenshot path (open it with Read) + accessibility tree
node e2e/tools/session.mjs click me "Calculator"  # the Dock; labels match accessible names first
node e2e/tools/session.mjs press me Tab
node e2e/tools/session.mjs drag me 900,640 500,400    # e.g. a window's resize corner
node e2e/tools/session.mjs resize me 1024 768
node e2e/tools/session.mjs stop me                # then stop the host
```

With the Vite dev server instead (`bun run dev`: host on :7400, client on :5173 with hot reload) point the session at
`http://localhost:5173/` and pair with the code the host prints; rebuild an app with `bun run build:apps` and the host reloads its
VM. A fresh `VOS_DATA` keeps your own `~/.vos` untouched; another code any time: `bun run host pair` with the same
`VOS_DATA`. URL param: `theme=light|dark`.

Cover the matrix for each changed behaviour:

| State | How |
|---|---|
| Light and dark | `?theme=dark`, or Settings |
| Floating at its preferred size | open the app from the Dock |
| Minimum window | drag `window-<app>-resize` toward the top-left; the window clamps at the app's `minW` / `minH` from `app.manifest` |
| Large window / full screen | drag the corner out, or ··· → Full Screen |
| Split View, Slide Over | ··· → Split View / Slide Over |
| Screen sizes | `resize me 1024 768` (iPad), 1600x1000 |
| Keyboard | Tab / Shift+Tab / Enter / Space / Escape / arrows where the control supports them |
| Text size | Settings → Text Size at 125% |
| Agent-driven | a tool changes the window with nobody pressing anything: in a spec `callTool` (`e2e/host/pair.ts`, MCP with the token in `<data>/mcp.json`); by hand, chat with an agent in the Agents app, or in the e2e host the scripted agent (prompt `tool`) |
| Offline / reconnect | stop and restart the host: windows dim with "Reconnecting" and recover |
| Before and after | for a fix: the same screenshot before your change and after it |

If you add behaviour, add to the checks so they keep it covered: a story (with a `play` function for an
interaction), a journey test, or a stricter generic check in `e2e/fixtures.ts`. Locators are `getByTestId` only.
If an element you need has no test id, that's a bug: add one (`coding-styleguide`, `references/typescript-svelte.md`,
"Test ids"; a Luau node needs a `key`).

## 4. Look at every screenshot

Open each relevant PNG with the Read tool. Put several in one image with
`node e2e/tools/contact-sheet.mjs sheet.png a.png b.png … --columns 3` (write it to a scratch directory, not the repo).
Compare each one with step 1 and with the review checklist in `.claude/skills/app-design/STYLEGUIDE.md` §11:

- placement: overlays centred on their anchor, nothing under the Dock or off screen, nothing peeking from behind
  a window;
- clipping and overlap: cut-off text, sideways scrolling, controls outside the window;
- flat iPadOS look: token colours in both themes, no gloss or bevels, materials only on floating chrome, the type
  scale, the 4 pt grid;
- the rest of the window: a fix in one place hasn't broken the layout around it.

If anything is off, fix it and look again. Don't explain a wrong screenshot away.

## 5. Record it

Put this in the PR description (or your final message when there's no PR):

```md
## Manual verification

Ran the shell on the real host headless (Chromium, 1280x800, plus 1024x768 and 1600x1000) and checked:

- [x] Settings, dark: the nav bar title is white on the dark bar (screenshot 1)
- [x] Calculator at its 320x500 minimum: every key ≥ 44 px, display fits (screenshot 2)
- [x] Before/after: before, the title was black on dark; after, fixed

How: `bun run test:e2e` (N passed), updated baselines reviewed as contact sheets; driven with
`e2e/tools/session.mjs` against `bun run host` for keyboard and resize.
```

Attach or link the screenshots that show the checked states. Say plainly what you couldn't check and why.

## 6. Design changes go through the panel

If the change is a design change to an app (new screens, a redesign, a new interaction), the `app-design` skill
applies as well: update the app's journeys and run a `ux-research` round before calling it done. For a UI change too
small for a round, run the `postbuild-synthetic-usability` skill on the changed screens and fix what it marks as a
blocker or major defect.

## 7. How the e2e host works (what the specs assume)

`playwright.config.ts` starts the host itself (`e2e/host/start.ts`): it builds the client, `vos-host`, the scripted ACP agent
(`cargo build -p vos-acp --bin fake_agent`) and the apps into a test registry (`bun scripts/build-apps.ts`, plus Notes as a
second app the Discover journeys install and remove, and the Ticker, `crates/vos-host/tests/fixtures/ticker`, an app whose
background task counts up, which no registry lists). It writes a `vos.toml` for a loopback host with a fresh data directory
under `test-results/host/`, the Calculator and Agents installed and one agent on the roster. Ports come from
`VOS_PORT_BASE` (`scripts/ports.ts`): unset, Storybooks are on 6100 and the host on 7410 (never 7400); set, Storybooks are
on the base and the host on base + 10 (`VOS_PORT_BASE=9100` gives 9110, for a second worktree). Never run two `test:e2e` on
one machine with the same base: they fight over the ports.

- **The scripted agent** is a real ACP process with no model (`crates/vos-acp/src/bin/fake_agent.rs`): the prompt text picks
  what it does (`hello`, `think`, `tool`, `fail`, `ask`, `ask-write`, `state`, `slow`, `exit`; anything else is echoed). Its
  approval policy is `ask` and it is granted the Calculator.
- **Pairing.** Every route needs a paired device, even from loopback. `e2e/host/global-setup.ts` pairs one browser the way
  a person does and saves the cookie as the storage state every desktop and journey spec shares. Use
  `http://127.0.0.1:<port>`, never `localhost` (the cookie is host-only; the other name is another device).
  `e2e/host/pair.ts` has `issuePairingCode()`, `pairDevice(name)` and `callTool(name, args)` (an agent calling an app tool
  over MCP with the token in `<data>/mcp.json`). The pairing journeys start unpaired and make at most two wrong guesses
  (five wrong codes a minute are rate limited).
- **One shared host.** The Calculator is one calculator for the whole host: specs that press its keys run serially
  (`describe.configure({ mode: 'serial' })`) and clear it first; the `journeys` project runs after `desktop`. Use Notes
  (offered, not installed) for anything that installs or removes an app, a second paired device for anything that revokes
  one, the Ticker for background work. An install or removal is broadcast to every open page, so those journeys are their
  own project, `background`, which runs after `journeys` (`--no-deps` runs only it).
- **Deterministic screenshots.** The clock is frozen at 09:41, `prefers-reduced-motion` is on, the viewport is 1280x800 at
  scale 1, fonts and two animation frames are awaited, `?theme=light|dark` forces the appearance. Baselines
  (`e2e/__screenshots__/**/*-darwin.png`) are macOS-only; the threshold is `maxDiffPixelRatio: 0.002`: never raise it to get
  green. `node e2e/tools/contact-sheet.mjs out.png a.png b.png ... [--columns 3] [--width 1800]` lays screenshots in one
  labelled image.
- **Stories.** `client/` (shell pages on `client/src/shell/fakeHost.ts`; renderer states on the fake hosts in
  `client/src/view/host/`) and `packages/ui` (each kit component and state) are Storybooks of `*.stories.svelte`. A story that
  deliberately clips tags itself `overflow-ok`; stories tagged `app` also run at a minimum and a large window.
- **Gotchas.** Storybook 10 ends its render at `finished`, not `completed`. Svelte transitions ignore reduced motion: wrap
  durations in `motion()` (`packages/ui/src/motion.ts`) or snapshots flake. Play functions click programmatically, which
  leaves a focus ring a pointer would not (the stories spec blurs first; a story that depends on focus waits for it).
  Notifications can overtake responses on `/rpc`: wait for state, not order. Stock iOS blue, red and secondary label fail AA,
  so the tokens use the high-contrast variants: when axe flags contrast, check the token first. A flaky test is a bug in the
  test or the app: find the missing wait or the race; never retry it into green or skip it.
- `bun scripts/design-audit.ts <page>` checks a built-in shell page's styles against the tokens; a Luau app has no CSS, so its
  conformance is the generic axe and overflow specs, the baselines and a live look.
