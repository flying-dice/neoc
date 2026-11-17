---
name: app-design
description: Use when designing, redesigning or polishing any AgentOS app (a new app module, a built-in app, or the desktop shell), or when asked how good an app's UX or design is. Runs the iterative UX process for that app: key user journeys, a user-research panel of Haiku, Sonnet and Opus participants who use the real app headless, a styleguide conformance score, then design changes, repeated until the exit criteria hold.
---

# Designing an AgentOS app

Every app goes through the same loop, from its first screen to every later redesign:

```
 journeys ──▶ build / change ──▶ automated checks ──▶ research round ──▶ amend the design ─┐
    ▲                              (green first)       (panel + score)                      │
    └──────────────────────────────── until the exit criteria hold ◀───────────────────────┘
```

The styleguide is `docs/design/DESIGN_LANGUAGE.md` (iPadOS virtual desktop, flat design). The testing setup is
`docs/TESTING.md`. Read both before the first round.

## 1. Key user journeys

Each app has 4 to 8 key journeys. They are what people come to the app to do, plus the hard cases: errors, small
windows, keyboard-only use, agents driving the app.

- Module apps: `apps/<app>/JOURNEYS.md`. Built-in apps and the shell: `apps/desktop/journeys/<id>.md`.
- Each row is an id (letter + number), who and what they want, a start state (web-mode URL params:
  `open`, `scenario`, `theme`), and an observable **Done when**. Never write steps: participants must find the
  way themselves.
- Every journey has an automated twin with the same id in `apps/<app>/e2e/*.spec.ts`, driven by test ids against
  web mode.
- A new feature means a new or updated journey in the same PR.

## 2. Automated checks first

The panel's time is expensive, so don't spend it on what a machine can catch. Before a round, all of these pass:

```sh
bun run lint && bun run typecheck && bun test
bun run test:e2e        # stories (axe, overflow, snapshots, resize) + desktop (every layout/size) + journeys
```

When a snapshot changes on purpose, update it (`bun run test:e2e:update`), then **open the changed PNGs and
look at them** (`node e2e/tools/contact-sheet.mjs out.png a.png b.png …` puts several in one image).

## 3. A research round

Start web mode, then run the saved workflow:

```sh
bun run dev:web            # or: bun run --filter @agentos/desktop build:web && … preview:web  (port 6200)
```

```
Workflow({ name: "ux-research", args: { app: "calculator", round: 1 } })
Workflow({ name: "ux-research", args: { app: "calculator", round: 2, previous: "docs/ux/calculator/round-1.md" } })
```

What happens (`.claude/workflows/ux-research.js`):

| Phase | Who | Does |
|---|---|---|
| Panel | **Haiku** as Sam (non-technical, impatient), **Sonnet** as Priya (iPad power user), **Opus** as Alex (low vision, keyboard, dark mode, small windows) | Each tries every journey in a real headless browser via `e2e/tools/session.mjs`, by visible labels only (no test ids, no source). They look at screenshots, think aloud, and give up when their persona would. They report outcome and ease (1 to 7) per journey, plus findings with screenshot evidence. |
| Audit | Opus as design-systems reviewer | Scores conformance to the styleguide with [CONFORMANCE.md](CONFORMANCE.md), from `scripts/design-audit.ts`, the axe/overflow/snapshot tests, the baselines and a live look. |
| Synthesis | research lead | Dedupes the findings, builds the scorecard, and writes `docs/ux/<app>/round-<n>.md` with a prioritised change list. |

The panel and the audit run in parallel. A round is 5 agents.

## 4. Amend the design

Work through the change list in priority order:

- Fix root causes in the kit (`packages/ui`) or the tokens, not one screen at a time. A kit change reaches every
  app, so run the whole e2e suite afterwards.
- If a finding shows the **styleguide** is wrong, change `DESIGN_LANGUAGE.md` deliberately in the same change and
  say so in the round report. Never quietly diverge from it.
- Every fixed finding gets a regression check where one is possible: a journey assertion, a story with a play
  function, or a stricter generic check in `e2e/fixtures.ts`.
- Then go back to step 2 (all green), then run the next round with `previous` set.

## 5. Exit criteria

An app's design is done for now when **one round** shows all of these:

- every journey succeeded for **all three** participants;
- mean ease ≥ **6.0** out of 7;
- **no blocker or major** findings open (minor and polish items may become follow-ups);
- conformance ≥ **90**, with **no category below 70**;
- automated checks green.

If the exit criteria still don't hold after **3 rounds**, stop and bring the latest report to the user: something
needs a product decision, not another iteration.

## 6. Record it

- Round reports stay in `docs/ux/<app>/`. They are the app's design history.
- The PR for a design change links the round report and quotes the scorecard. Also run the `verify-change` skill
  for the change itself.
