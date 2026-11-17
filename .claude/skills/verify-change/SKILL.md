---
name: verify-change
description: Use before finishing, opening or merging ANY AgentOS change that alters what a person sees or does (kit components in packages/ui, app pages, the desktop shell, window management, menus, dialogs, layout, theming, agent tools that move the UI). Run the real renderer headless in web mode, exercise the change, look at the screenshots against the intended result and the design language, and write down what was checked and how. Passing unit tests is not enough.
---

# Verify a change in the real UI before it lands

Tests check what someone thought to assert. They don't see that a menu covers its own label, that a sheet is cut
off in a small window, or that dark mode turned a title black on a dark bar. So **the agent that makes a UI change
runs the real renderer, uses the changed behaviour, looks at the screenshots and says what it checked.** There are
no exceptions for "small" kit changes: a kit change touches every app.

Everything runs headless with **no Electron and no main process**. Web mode (`apps/desktop`, `web.html`) runs the
real desktop renderer on the in-memory host from `@agentos/sdk/testing`, which runs the real app main modules and
a scripted Claude. See `docs/TESTING.md`.

## 1. Write down the intended result first

Before you look, write 3 to 8 lines in a person's terms, for example:

- "The multitasking menu opens centred under the ··· handle, fully inside the window, in both themes."
- "At the Calculator's minimum window (320x480) every key is a full circle of at least 44 px; nothing is clipped."

List **every place the change shows up**. For a kit component, find its users:
`grep -rn "<ComponentName" apps packages/ui/src --include=*.svelte`. Pick at least one real app per variant.

## 2. Run the checks that cover it

```sh
bun run lint            # atomic layers + test ids (every interactive element has data-testid)
bun run typecheck       # includes the required `testid` props on kit components
bun test                # unit tests (src/shared logic)
bun run test:e2e        # builds every Storybook, then:
                        #   stories  every story: axe light+dark, overflow, snapshots, min/large window
                        #   desktop  every app: Dock, floating light/dark, resize, full screen/split/slide over, iPad & large screens
                        #   apps     each app's key journeys (apps/<app>/e2e)
```

Run a slice with `npx playwright test --project=desktop --grep calculator`.

When a change is intended to alter visuals, update the baselines (`bun run test:e2e:update`) and then go to step 4.
**A snapshot update with no look at the images is not verification.**

## 3. Exercise it like a person

Cover what the automated suite can't see yet. Drive web mode yourself:

```sh
bun run dev:web                                   # http://localhost:6200/web.html
node e2e/tools/session.mjs start me --url "http://localhost:6200/web.html?open=settings&theme=dark"
node e2e/tools/session.mjs look me                # screenshot path + accessibility tree
node e2e/tools/session.mjs click me "Display & Appearance"
node e2e/tools/session.mjs press me Tab
node e2e/tools/session.mjs drag me 900,640 500,400    # e.g. a window's resize corner
node e2e/tools/session.mjs resize me 1024 768
node e2e/tools/session.mjs stop me
```

URL params: `open=<app,app>`, `theme=light|dark`, `scenario=<name>` (`apps/desktop/src/renderer/src/web/scenarios.ts`),
`delay=<ms>` for streamed agent replies. `window.__agentos` exposes the host and the stores.

Cover the matrix for each changed behaviour:

| State | How |
|---|---|
| Light and dark | `theme=dark`, or Settings → Display & Appearance |
| Floating at its preferred size | `open=<app>` |
| Minimum window | drag `window-<app>-resize` toward the top-left; the window clamps at the app's `minWidth` / `minHeight` |
| Large window / full screen | drag the corner out, or ··· → Full Screen |
| Split View, Slide Over | ··· → Split View / Slide Over |
| Screen sizes | `resize me 1024 768` (iPad), 1600x1000 |
| Keyboard | Tab / Shift+Tab / Enter / Space / Escape / arrows where the control supports them |
| Text size | Settings → Text Size at 125% |
| Agent-driven | for app behaviour: a scripted agent (`demoAgent` in `@agentos/sdk/testing`) uses the app's tools and the window follows |
| Before and after | for a fix: the same screenshot before your change and after it |

If you add behaviour, add to the checks so they keep it covered: a story (with a `play` function for an
interaction), a journey test, or a stricter generic check in `e2e/fixtures.ts`. Locators are `getByTestId` only.
If an element you need has no test id, that's a bug: add one following `docs/TESTING.md`.

## 4. Look at every screenshot

Open each relevant PNG with the Read tool. Put several in one image with
`node e2e/tools/contact-sheet.mjs /tmp/sheet.png a.png b.png … --columns 3`. Compare each one with step 1 and with
the review checklist in `docs/design/DESIGN_LANGUAGE.md` §11:

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

Ran the renderer headless in web mode (Chromium, 1280x800, plus 1024x768 and 1600x1000) and checked:

- [x] Settings, dark: the nav bar title is white on the dark bar (screenshot 1)
- [x] Calculator at its 320x480 minimum: every key ≥ 44 px, display fits (screenshot 2)
- [x] Before/after: before, the title was black on dark; after, fixed

How: `bun run test:e2e` (N passed), updated baselines reviewed as contact sheets; driven with
`e2e/tools/session.mjs` for keyboard and resize.
```

Attach or link the screenshots that show the checked states. Say plainly what you couldn't check and why.

## 6. Design changes go through the panel

If the change is a design change to an app (new screens, a redesign, a new interaction), the `app-design` skill
applies as well: update the app's journeys and run a `ux-research` round before calling it done.
