---
name: app-design
description: Use when designing, redesigning or polishing any vos app (a new Luau app, a built-in shell page, or the desktop shell), or when asked how good an app's UX or design is. Runs the iterative UX process for that app: key user journeys, a user-research panel of Haiku, Sonnet and Opus participants who use the real app headless on the real host, a styleguide conformance score, then design changes, repeated until the exit criteria hold.
---

# Designing a vos app

Every app goes through the same loop, from its first screen to every later redesign:

```
 discovery ──▶ journeys ──▶ build / change ──▶ automated checks ──▶ usability review ──▶ research round ──▶ amend ─┐
 (new apps)        ▲                              (green first)      (one agent)         (panel + score)          │
                   └──────────────────────────────── until the exit criteria hold ◀──────────────────────────────┘
```

The styleguide is [STYLEGUIDE.md](STYLEGUIDE.md) (iPadOS virtual desktop, flat design). How to run the real shell and the
e2e host is in the `verify-change` skill (sections 3 and 7). Read both before the first round.

**Where the research output goes:** `ux/<app>/` at the repository root (`discovery.md`, `review-<n>.md`, `round-<n>.md`).
The repo keeps no other long-form docs; these files are the app's design history and the evidence the readiness check in
`pr-review` ("vos checks") asks for, and they are committed with the PR that makes the design change.

## 0. Discovery (new apps and re-scopes)

Before building a new app, or changing what an existing one is for, run the **`prebuild-synthetic-ux`** skill on its
issue. It writes `ux/<app>/discovery.md`: constrained personas (including an agent that uses the app's tools),
the riskiest assumptions with the cheapest tests, and key journeys and user stories. Its key journeys seed
`JOURNEYS.md` (section 1, without the steps); its Must stories are the first PR's scope. Skip it for a polish pass on
an app whose purpose is not changing.

## 1. Key user journeys

Each app has 4 to 8 key journeys. They are what people come to the app to do, plus the hard cases: errors, small
windows, keyboard-only use, agents driving the app.

- Luau apps: `apps/<app>/JOURNEYS.md`. Built-in shell pages and the shell: `client/journeys/<id>.md`.
- Each row is an id (letter + number), who and what they want, a start state (what is open and installed; the only URL
  parameter is `theme=light|dark`), and an observable **Done when**. Never write steps: participants must find the way
  themselves.
- Every journey has an automated twin with the same id in `apps/<app>/e2e/*.spec.ts` (or `client/e2e`,
  `e2e/*.journeys.spec.ts`), driven by test ids against the real host.
- A new feature means a new or updated journey in the same PR.

## 2. Automated checks first

The panel's time is expensive, so don't spend it on what a machine can catch. Before a round, all of these pass:

```sh
bun run lint && bun run typecheck && bun run test
bun run test:e2e        # stories (axe, overflow, snapshots) + desktop (every app, every layout/size) + journeys
```

When a snapshot changes on purpose, update it (`bun run test:e2e:update`), then **open the changed PNGs and
look at them** (`node e2e/tools/contact-sheet.mjs out.png a.png b.png …` puts several in one image).

## 3. Usability review, then a research round

First run the **`postbuild-synthetic-usability`** skill against the running app (one agent; it writes
`ux/<app>/review-<n>.md`). Fix its blocker and major defects, back to step 2, then spend the panel's time.


Run the real host with the app installed and the built client, then run the saved workflow
(`verify-change` section 3, "Exercise it like a person"):

```sh
bun run build && bun run build:apps
# the newest built version of an app (dist/apps/<app>/<version>/app-ref.json), whatever its version is
ref() { echo "file://$(ls -d "$PWD/dist/apps/$1"/*/ | sort -V | tail -1)app-ref.json"; }
VOS_DATA=$(mktemp -d) VOS_PORT=7420 VOS_INSTALL="$(ref <app>)" bun run host
```

```
Workflow({ name: "ux-research", args: { app: "calculator", round: 1, baseUrl: "http://127.0.0.1:7420/", dataDir: "<the VOS_DATA above>" } })
Workflow({ name: "ux-research", args: { app: "calculator", round: 2, previous: "ux/calculator/round-1.md", baseUrl: "...", dataDir: "..." } })
```

What happens (`.claude/workflows/ux-research.js`):

| Phase | Who | Does |
|---|---|---|
| Panel | **Haiku** as Sam (non-technical, impatient), **Sonnet** as Priya (iPad power user), **Opus** as Alex (low vision, keyboard, dark mode, small windows) | Each pairs a headless browser with the host (setup, not part of the study) and tries every journey through `e2e/tools/session.mjs`, by visible labels only (no test ids, no source). They look at screenshots, think aloud, and give up when their persona would. They report outcome and ease (1 to 7) per journey, plus findings with screenshot evidence. |
| Audit | Opus as design-systems reviewer | Scores conformance to the styleguide with [CONFORMANCE.md](CONFORMANCE.md), from the axe/overflow/snapshot tests, the baselines, a live look and, for shell pages, `scripts/design-audit.ts`. |
| Synthesis | research lead | Dedupes the findings, builds the scorecard, and writes `ux/<app>/round-<n>.md` with a prioritised change list. |

The panel and the audit run in parallel. A round is 5 agents.

## 4. Amend the design

Work through the change list in priority order:

- Fix root causes in the kit (`packages/ui`), the renderer adapters (`client/src/view`), the component schema
  (Rust, then `bun run codegen`) or the tokens, not one screen at a time. A kit change reaches every app, so run the whole
  e2e suite afterwards. A Luau app changes its tree, keys, labels, copy and window size only: it never styles.
- If a finding shows the **styleguide** is wrong, change `STYLEGUIDE.md` deliberately in the same change and
  say so in the round report. Never quietly diverge from it.
- Every fixed finding gets a regression check where one is possible: a journey assertion, a story with a play
  function, or a stricter generic check in `e2e/fixtures.ts`.
- Then go back to step 2 (all green), then run the next round with `previous` set.

## 5. Exit criteria

An app's design is done for now when **one round** shows all of these:

- every journey succeeded for **all three** participants;
- mean ease ≥ **6.0** out of 7;
- **no blocker or major** findings open (minor and polish items are filed by the author as Bug or Story tickets with the area's label);
- conformance ≥ **90**, with **no category below 70**;
- automated checks green.

If the exit criteria still don't hold after **3 rounds**, stop and bring the latest report to the user: something
needs a product decision, not another iteration.

## 6. Record it

- Round reports stay in `ux/<app>/`. They are the app's design history.
- The PR for a design change links the round report and quotes the scorecard. Also run the `verify-change` skill
  for the change itself.
