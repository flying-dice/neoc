---
name: design-review
description: Review a vos surface (an app, the shell, or a package's components) against iPadOS (Apple's Human Interface Guidelines) and the vos design language, then fix what is wrong, with special attention to fonts and text. Use for a visual audit, design QA, "does this look right / match the styleguide", before shipping UI, or after a UI change.
---

# Design review

The rules are the [ui-styleguide](../ui-styleguide/SKILL.md) rulebook: `references/index.md` lists every rule by ID,
and each area has its own file. This skill is how to check a surface against them. Text comes first: it is most of
an iPadOS screen, where most deviations hide, and the hardest to see.

Apple's HIG pages now describe Liquid Glass; vos is iPadOS 16–17. Where they and the rulebook differ (Liquid Glass,
scroll edge effects, sentence-case alert buttons, a tab bar at the top, glyph-only Back and Close, search at the bar's
trailing end in Mail and Notes), the rulebook wins.

## 1. Capture every state
```
bun .agents/skills/design-review/scripts/capture.ts <book> --build
```
`<book>` is the Storybook's folder name (`notes`, `ui`, `kit`, `client`). It renders every story at iPad sizes (an
`app` story at full, half and Slide Over width, and at its smallest window), light and dark, at text size ×1.25 and
with Reduce Motion and Transparency, and writes to `test-results/design-review/<book>/`:
- `shots/*.png`: the pictures;
- `styles.md`: every text style drawn (the type inventory);
- `summary.md`: measured findings, named by rule ID (`references/checks.md` says what each test covers).

Each picture is a story's last frame: an alert, menu, banner or tooltip that a play function opened and closed is in
none of them. Before looking, list those from the source ([source.md](references/source.md), first section). A state
with no story is read in its source and reported as a missing story.

## 2. The type inventory
Read `typography.md` and `writing.md` in full, then go through `styles.md` line by line. Each line must be one row of
the role table, whole: size, leading, weight, tracking and colour.
- A size marked off-ramp (TYPE-01), a weight outside 400/500/600/700 (TYPE-20) or a colour in bold (off the palette)
  is a finding.
- Tracking that is not its size's token fails TYPE-25. `-0.41` on any size but 17 is almost always
  `font: var(--type-…)` set without its `letter-spacing`.
- Two lines for one role (two bar-title styles, two bar-button sizes) fail TYPE-49 and the role's own rule.
- The samples say where a style is drawn: search the source for them.

## 3. Look
- Read the area files the surface uses: always `color.md`, `layout.md` and `accessibility.md`; then navigation,
  controls, forms, lists, modals, menus, feedback, help and input as they apply.
- Walk [references/look.md](references/look.md): sections 1 to 6 once per story, section 7 (choices) once per app,
  then the comparisons.

## 4. Triage the machine's findings
For each finding in `summary.md`, open its picture and decide whether it is real; calling it allowed needs a rule that
allows it. A finding caused by a kit element, a token or the shell (it appears wherever that element does) is
reported once, under Shared. A real problem filed under the wrong rule moves to the rule that fits (red error text
outside the kit's footer is FORM-19, not COLOR-08).

## 5. Source and behaviour
- Read each component of the surface with [references/source.md](references/source.md): kit `ui-*` elements, not
  hand-rolled controls; tokens, not literals; ARIA names.
- Run `bun .agents/skills/design-review/scripts/lint-source.ts <folder>` for leads.
- Do the journeys of [references/interact.md](references/interact.md) once per app: the keyboard alone, pointer then
  touch, a narrowing window, each form with wrong values, each delete and alert, slow and failing data. Operate the
  built Storybook or read the handlers, and say which.

## 6. Fix and verify
- Fix every finding, blockers first, in the design language: `--type-*` with its `--tracking-*`, or `.ios-*` classes;
  colour and radius tokens; kit elements; the writing rules.
- Never restyle around a kit element in one app.
- When reviewing an app, don't edit `packages/*` or `client/`. List those problems under Shared.
- Run the app's `typecheck` and `build-storybook`, then capture again. Look at the pictures of what you changed.

## Report
1. **Findings**, most severe first: `rule ID | severity | where (story, size, theme, or file:line) | what is wrong | fix`.
2. **Shared**: kit, `@vos/ui`, token and shell problems, not fixed.
3. **Not checked**: areas or states you could not see, and why.

Severity:
- `blocker`: unreadable or unusable (contrast, clipped text, an unlabeled control, a target too small to hit); a task
  the keyboard or touch cannot finish (a keyboard trap, something only on hover or only in a tooltip, no way back in
  a narrow window); typed input lost (Escape, a resize or a click outside discards it).
- `major`: what stops it looking or behaving like iPadOS: a text style that is not its role's, or is applied in pieces
  (size, weight, leading, tracking); edges or baselines that should line up and don't; the wrong control, modal or
  message for the job; misleading copy.
- `minor`: polish: a radius, a gap off the rhythm, copy that is right but wordy.
