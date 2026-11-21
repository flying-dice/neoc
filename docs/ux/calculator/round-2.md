# Calculator, UX research round 2

Same three personas as [round 1](round-1.md) (fresh sessions, hosts and data), on the build with the round 1 changes.
Journeys J1, J2 (Priya, Alex), J3, J4, J7. The participants were not told what had changed.

## Scorecard

| Journey | Sam | Priya | Alex |
|---|---|---|---|
| J1 find it, 12 + 7 | success, 6 | success, 6 | success, 6 (opened from the Dock by keyboard, typed at once) |
| J2 keyboard only | not run | success, 6 | success, 5 |
| J3 chain, (9 x 3) - 7 | success, 5 | success, 6 | success, 6 |
| J4 divide by zero, long numbers | success, 5 | success, 6 | success, 6 |
| J7 minimum window | success, 4 | success, 6 | success, 5 |

Success: **14 of 14** (was 9 of 14). Mean ease **5.6** / 7 (was 5.0). Conformance 92.

## Findings

| # | Severity | Hit by | Finding | Status |
|---|---|---|---|---|
| 1 | major | Alex | Enter on a key reached with Tab acted as `=`, not as a press of that key. | **Fixed** after the round, and **re-fixed after review**: the first fix decided by `:focus-visible`, but in Chromium a keydown on a clicked key turns that on, so after clicking keys Enter pressed the clicked key again (7, +, 2, Enter showed 22, not 9). Now the client decides from what the person last did (Tab, a click or touch, or typing: `view/core/modality.ts`): Enter and Space press a key walked to with Tab, and after a click or after typing Enter is `=`. Journey J2 checks both ([round 3](round-3.md)). |
| 2 | major | Alex | After dragging the resize corner focus sat on the window frame and typed keys were ignored. | **Fixed** after the round: the kit `Window` hands focus to its content's `data-window-focus` element. Journey J7 types after the resize. |
| 3 | major | Priya | No operator precedence: `2 + 3 x 4` shows 20 where she expects 14 (and the agent tool `calculate`, which parses precedence, says 14). | **Open, product decision.** The keypad has always chained left to right, immediate execution (documented in the README and tested). The default is kept; the owner decides. |
| 4 | minor | 3 of 3 | Digits past the ninth dropped silently. | Deferred polish. |
| 5 | minor | 2 of 3 | The pending ring is thin (a white key as on iPadOS reads faster), and it leaves the key once the second number starts; the caption keeps saying it. | Accepted: the ring is the kit-wide contrast-safe pressed look. |
| 6 | minor | 2 of 3 | The focus ring is faint on black (kit-wide token). | Deferred, kit-wide. |
| 7 | minor | 3 of 3 | Dock tile is an orange `+` with no name; no resize grip, no cue at the minimum; Disable has no confirmation. | Deferred, shell. |
| 8 | minor | 1 of 3 | The display has no accessible name; the shell's status region is empty (the display is itself an `aria-live` region). | Deferred. |
| 9 | polish | 2 of 3 | Stale caption after `=` when a new number starts; caption close to the grab dots at the minimum window. | Deferred. |

Resolved since round 1 and confirmed by the panel: the pending operator is visible on the key and in the caption (all
three), the display shows `123,456,789` and `9.99999998e17` in full (all three), keyboard works as soon as the window
opens (Priya, Alex), a digit recovers from an error (Priya, Alex), the error reasons are clear (all three).

## Exit criteria

| Criterion | Result |
|---|---|
| Every journey succeeded for every participant | met (14 of 14) |
| Mean ease >= 6.0 | **not met** (5.6; the lowest were J7 for Sam, 4, who had to guess the resize corner (shell), and J2 and J7 for Alex before the two fixes above) |
| No blocker or major open | **not met**: finding 3 (precedence) awaits a product decision; 1 and 2 are fixed |
| Conformance >= 90, no category under 70 | met (92) |
| Automated checks green | met (see the verification record) |

Per the owner's brief (a focused evaluation, a second round only if the first misses the criteria) this stops here; a third
round would measure the two fixes above, not discover new issues. Findings 3, 4 and 7 need decisions or shell work outside
the Calculator.
