---
name: verify-change
description: Use before finishing, opening or merging ANY AgentOS change that alters what a person sees or does (kit components in packages/ui, renderer adapters and the shell in client/, Luau app windows, window management, menus, dialogs, layout, theming, agent tools that move the UI, host changes that change a view or protocol message). Run the real shell and the real Rust host headless, exercise the change, look at the screenshots against the intended result and the design language, and write down what was checked and how. Passing unit tests is not enough.
---

# Verify a change in the real UI before it lands

Tests check what someone thought to assert. They don't see that a menu covers its own label, that a sheet is cut
off in a small window, or that dark mode turned a title black on a dark bar. So **the agent that makes a UI change
runs the real shell, uses the changed behaviour, looks at the screenshots and says what it checked.** There are
no exceptions for "small" kit changes: a kit change touches every app.

Everything runs headless against the **real Rust host**: the shell in Chromium, the Luau apps on the host, one paired
browser. The e2e run starts its own host on port 7410 (planned: derived from `AGENTOS_PORT_BASE`), so it never collides with
a host you have running on 7400. See `docs/TESTING.md`.

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
bun run test            # cargo nextest, then the client, kit and types unit tests
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

Cover what the automated suite can't see yet. Run the host and drive the shell yourself (`docs/TESTING.md`,
"Driving the real shell"):

```sh
bun run build && bun run build:apps                       # client/dist and the app refs in dist/apps
AGENTOS_DATA=$(mktemp -d) AGENTOS_PORT=7420 \
  AGENTOS_INSTALL="file://$PWD/dist/apps/calculator/0.1.0/app-ref.json,file://$PWD/dist/apps/agents/0.1.0/app-ref.json" \
  bun run host                                            # prints "Pairing code: K7QM-3XHR"; leave it running (run_in_background)
node e2e/tools/session.mjs start me --url "http://127.0.0.1:7420/?theme=dark"
node e2e/tools/session.mjs fill me "Pairing code" K7QM-3XHR      # every device pairs first, even on loopback
node e2e/tools/session.mjs click me "Pair"
node e2e/tools/session.mjs look me                # screenshot path (open it with Read) + accessibility tree
node e2e/tools/session.mjs click me "Calculator"  # the Dock; labels match accessible names first
node e2e/tools/session.mjs press me Tab
node e2e/tools/session.mjs drag me 900,640 500,400    # e.g. a window's resize corner
node e2e/tools/session.mjs resize me 1024 768
node e2e/tools/session.mjs stop me                # then stop the host
```

With the Vite dev server instead (`bun run dev`: host on :7400, client on :5173 with hot reload) point the session at
`http://127.0.0.1:5173/` and pair with the code the host prints; rebuild an app with `bun run build:apps` and the host reloads its
VM. A fresh `AGENTOS_DATA` keeps your own `~/.agentos` untouched; another code any time: `bun run host pair` with the same
`AGENTOS_DATA`. URL param: `theme=light|dark`.

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
If an element you need has no test id, that's a bug: add one following `docs/TESTING.md` (a Luau node needs a `key`).

## 4. Look at every screenshot

Open each relevant PNG with the Read tool. Put several in one image with
`node e2e/tools/contact-sheet.mjs sheet.png a.png b.png … --columns 3` (write it to a scratch directory, not the repo).
Compare each one with step 1 and with the review checklist in `docs/design/DESIGN_LANGUAGE.md` §11:

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
applies as well: update the app's journeys and run a `ux-research` round before calling it done.
