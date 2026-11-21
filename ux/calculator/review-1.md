# Calculator: usability review 1 (AGOS-57, operator precedence)

A `postbuild-synthetic-usability` pass (one agent, the author of the change) on the running app, version 0.3.0, on the real
host headless. It is **not** a `ux-research` round: no panel, no conformance score. The PR says why and asks the reviewer to
rule on that (see "Position on the research round").

Earlier Calculator research reports predate NEOC-115 and were dropped with `docs/` (git `51f47f4`). This is the first file
under `ux/calculator/`.

## Scope

What changed for a person: how a sum is worked out (`×` and `÷` before `+` and `−`), what the display and the caption show
while a sum is in progress. What did not: layout, keys, labels, sizes, colours, the window, the tools' shapes. The review
covers J3 and the new J8, and re-walks J2 (keyboard), J4 (error) and J5 (agent) for regressions.

Method: `e2e/tools/session.mjs` against `vos` on `:9520` with a temporary data directory; the accessibility tree and a
screenshot after each step; dark and light, 320x500 minimum window, 1024x768, 1600x1000 full screen, Slide Over; mouse and
keyboard. Evidence is the screenshots listed in the PR's "Manual verification" and the assertions of J3 and J8.

## Inventory

| Present | Notes |
|---|---|
| Display (`live="polite"`, label "Result") | In progress it shows what the pressed operator completed: `2 + 3 ×` shows 3, `2 × 3 +` shows 6. |
| Caption (one line, not a live region) | The sum so far: `2 + 3 ×`; the whole sum after `=`; an agent's expression after `show_on_calculator`; the reason under `Error`. |
| Operator keys | The waiting operator is outlined and `aria-pressed`. |
| Error state | Unchanged sentence, same in the tool ("Can't divide by zero"). |

Cannot be assessed here: what a screen reader says (no assistive technology in this environment), and what real people
expect after `2 + 3 ×`.

## Task walkthroughs

| Task | Result | Notes |
|---|---|---|
| J3: 9 × 3 − 7 | completed | Caption `9 ×` while typing 3, `9 × 3 −` with 27 on the display, `9 × 3 − 7` and 20 after `=`. |
| J8 mouse: 2 + 3 × 4 | completed | 3 after `×`, caption `2 + 3 ×`, `×` outlined; 4 while typing; 14 and `2 + 3 × 4` after `=`. |
| J8 keyboard: `2*3+` `4=` | completed | 6 after `+`; 10. `2+3*4-5/5` then Enter gives 13. |
| J8 chain from a result, change of mind | completed | `+1*` after 13 shows 1; `+2=` gives 16 with caption `13 + 1 + 2` (the `×` was replaced). |
| J8 divide by zero in a chain: `5+3/0=` | completed | Error with the reason; `2+3*4=` after it gives 14. |
| J2, J4, J5, J6, J7 | completed | Unchanged by this change; their specs pass. |

A busy person who expects a running total (a pocket calculator that works left to right) sees 14 and the caption
`2 + 3 × 4`: the caption is what lets them see why it is not 20.

## Heuristics and edge cases

Findings are labelled *defect* (verifiable), *likely issue* (a named heuristic) or *hypothesis* (needs real people).

1. **Hypothesis, minor.** Visibility of system status: after `2 + 3 ×` the display shows 3, not 5. This is what the iPadOS
   Calculator does and what the ticket asks for; the caption and the outlined `×` say why. Someone used to a left-to-right
   machine may pause. Cheapest check: five people, one sum, ask what they expect to see.
2. **Hypothesis, minor.** Screen readers: the display is a polite live region and says "3" after `×`; the pending operator
   is only on the key (`aria-pressed`) and the caption is deliberately not live. Whether that is enough could not be
   verified without assistive technology. Not a regression: the same split existed for `27 −`.
3. **Likely issue, minor, by design.** The caption is one line of at most 24 characters of terms, and a long chain keeps its
   last terms behind an ellipsis (`… 3 + 4 + 5 + 6 + 7 + 8`; `… 999,999,999 ×`). The leading operands are then not visible.
   The display and the key still tell the state; a wrapping caption would need a layout change, which is out of scope.
4. **Kept as it was, minor.** A second `=` does nothing and `%` is "divide by 100" (the iPadOS Calculator repeats the last
   operation and takes a percentage of the first operand). Both predate this change; `JOURNEYS.md` records the choice.
   Hypothesis: people who rely on `5 + 3 = =` meet it.
5. **Edge cases checked, no defect.** Operator changed before the next number (re-reads the sum, nothing calculated);
   `C` in the middle of a sum (display 0, the sum goes on, an operator replaces the waiting one, `AC` drops it); `±` and `%`
   end the wait (`2 + 3 × ±` then `=` is -7); `=` with an operator waiting uses the displayed number as the operand
   (`2 + 3 × =` is 11, as `5 + =` is 10); a result too large to show fails at the press that cannot be shown.
6. **Agents.** `calculate` already described its precedence ("2 + 3 * 4 is 14") and now agrees with the keypad: a table of
   24 sums and every four-number, three-operator sum (128) give the same display from both. `show_on_calculator` puts the
   result on the keypad and carries on from it with the same rules.

No blocker and no major defect.

## Validation

- Five people, `2 + 3 ×` then `4 =`: what do they expect on the display after `×`, and do they find the caption? Cheapest
  way: a moderated session on an iPad with the host on the LAN.
- A VoiceOver pass over J8 (does hearing "3" after `×` mislead, is `aria-pressed` on `×` announced).
- If the `ux-research` panel is wanted for this app, run it as its own round (`ux/calculator/round-1.md`) with this review's
  items 1, 2 and 4 as the questions.

## Position on the research round

This review stands in for a round only if the reviewer agrees that a semantics-only change does not need the panel. The
readiness row "Researched" of `pr-review` V5 is therefore **not met by this PR** and is called out in the PR for a ruling.
