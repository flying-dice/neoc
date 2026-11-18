---
name: postbuild-synthetic-usability
description: Simulated usability review of something that exists (code, screenshot, prototype, wireframe). Task walkthroughs, heuristic and WCAG audit, edge cases and prioritised findings.
---

# Post-build synthetic usability review

A usability review of a real artifact, using simulated users plus expert inspection. It finds defects you can verify now and flags behaviour worth testing with real people. It does not replace a usability test or an accessibility audit with assistive technology.

## Before starting

You need the artifact and the tasks users are meant to complete with it. If no tasks are given, infer the one or two obvious ones and state them.

## Three rules that always hold

Everything else in this skill is judgment. These are not.

**1. Review only what is there.** If a label, state, error message or piece of help text is not in the artifact, the user does not have it. Do not assume backend behaviour, onboarding, or anything that happens off-screen. Say what the artifact type cannot show: a screenshot says nothing about keyboard order, focus or screen-reader semantics; code without rendered styles says little about visual hierarchy or contrast.

**2. Simulated users behave like busy people.** They skim, misread jargon, pick the wrong control when two look alike, hesitate when the next step is unclear, and abandon when effort outweighs value. They do not read everything, forgive friction or praise the design. If every walkthrough succeeds cleanly, look again.

**3. Label each finding by how sure you can be.**
- *Defect*: verifiable in the artifact. Point to the element or line.
- *Likely issue*: breaks a recognised heuristic or pattern. Name it.
- *Hypothesis*: a prediction about behaviour that needs real users.

Cite a WCAG success criterion only when you can check it from the artifact; otherwise write "could not verify". Never claim conformance.

## Workflow

Six stages. Scale them to the request: a single component may need only 1, 3 and 5. Skip what does not apply and say what you skipped.

1. **Inventory.** What is actually present: screens, controls, states, copy. What is missing or cannot be assessed.
2. **Task walkthrough.** Step through each primary task: what the user sees, what they would most likely do, where they stall. Record each as completed, struggled or abandoned, with the reason.
3. **Heuristic and accessibility audit.** Use Nielsen's heuristics and WCAG 2.2 AA as lenses, not checklists to fill in. Report only what applies.
4. **Edge cases and errors.** Empty, invalid, slow, failed, interrupted, repeated. Does the artifact handle each one, or go silent?
5. **Prioritise.** Blocker, major or minor, judged by effect on task completion and how many users are likely to hit it.
6. **Validation.** What to confirm with real users or assistive technology, and the cheapest way to do it.

## Output

Lead with blockers and their fixes. For each finding: what, where, why it matters, its label, and a suggested fix. Format to suit the request. Do not pad with praise; mention what works only when it is a deliberate choice worth protecting from later changes.

## In AgentOS

The artifact is the **running app on the real host**, not a description of it, so rule 1 can be applied to more than
a screenshot: drive it.

- **Setup** (`docs/TESTING.md`, "Driving the real shell"): `bun run build && bun run build:apps`, start a host with the
  app installed, pair, then use `node e2e/tools/session.mjs` (`start`, `pair`, `look`, `click`, `type`, `press`,
  `resize`). `look` gives a screenshot and the accessibility tree, so keyboard order, focus, roles and names *can* be
  checked here: check them rather than writing "could not verify". Read the app's tree in `apps/<app>/app.lua` for
  labels, keys and states the screens do not reach.
- **Tasks** are the app's journeys (`apps/<app>/JOURNEYS.md` or `client/journeys/<id>.md`); each **Done when** is the
  pass condition for the walkthrough. Walk every one, plus the matrix `verify-change` lists (light and dark, the
  minimum window, large, Split View and Slide Over, keyboard only).
- **Agents are users too.** For each tool the app registers, read its description and schema as an agent would: is it
  clear which tool does what, what the arguments mean, and what a failure says? Call one through the Agents app with
  the scripted agent where the journey involves it.
- **Lenses:** Nielsen and WCAG 2.2 AA as written above, plus `docs/design/DESIGN_LANGUAGE.md` (§11 checklist) and
  [CONFORMANCE.md](../app-design/CONFORMANCE.md) when a score is wanted. Cite evidence by screenshot path or by
  `file:line` in `app.lua` / the kit.
- **Write it to** `docs/ux/<app>/review-<n>.md`. Blocker and major *defects* are fixed before the next `ux-research`
  round (that panel is expensive; this review is one agent). A *defect* found here gets a regression check where one is
  possible: a journey assertion, a story with a play function, or a stricter generic check in `e2e/fixtures.ts`.
- It does not replace the `ux-research` round the readiness gate asks for: it is the cheap pass that makes that round
  worth running, and the review to use for a UI change too small for a round (`verify-change` points here).
