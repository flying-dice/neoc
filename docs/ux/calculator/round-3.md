# Calculator, UX research round 3

A focused round on the three journeys that the review of round 2 reopened: **J2** (keyboard only), **J4** (an error and its
announcement) and **J7** (the smallest window). Same three personas as [round 1](round-1.md) and [round 2](round-2.md): Sam
(Haiku, mouse only, so no keyboard journey), Priya (Sonnet) and Alex (Opus, dark, 1024x768, keyboard). Fresh sessions on fresh
hosts, the built client and the Calculator 0.2.1, by labels and the accessibility tree only; the participants were not told what
had changed. The conformance score is from a **separate audit agent** (Opus), not from the moderator.

What changed before this round: Enter after clicked keys (the `:focus-visible` check pressed the clicked key again: `7 + 2 Enter`
showed 22), the error reason announced through the window's `w:announce` instead of a live caption, a generic message for
unexpected errors, and the other review fixes (see the commit message).

## Scorecard

| Journey | Sam | Priya | Alex |
|---|---|---|---|
| J2 keyboard only | not run (mouse only) | success, 6 | first pass: struggled, 5; **re-run after the fix: success, 7** |
| J4 divide by zero | success, 6 | success, 6 | success, 6 |
| J7 minimum window | struggled, 4 | success, 6 | struggled, 4 |

Success: 9 of 9 finished; 7 of 9 without struggling (both J7 struggles are about finding the resize corner, which is the
shell's). Mean ease **5.7** / 7 (51 over 9 ratings, with Alex's J2 re-run; 5.4 for the first pass alone; round 2 was 5.6).
Conformance **96** (audit agent, separate from the moderator; round 2: 92).

### Conformance, per category (audit agent)

Colour and tokens 100, Flat 100, Materials 100, Typography 95, Layout and radii 98, Resize and multitasking 80, Accessibility
100, Controls 100, Content and states 95. No category under 70.

## Findings

| # | Severity | Hit by | Finding | Status |
|---|---|---|---|---|
| 1 | major | Alex | Enter was inconsistent: after Tab to a key and typing `+2`, Enter pressed the focused key again (`88 + 28`), while after a click it meant `=`. | **Fixed in this round**: a key tabbed to and then typed on is not being navigated; Enter is `=` (`view/core/modality.ts`, `typing`). Re-run: Alex J2 ease 7, "the rule feels consistent and easy to predict" (Tab then Enter presses; typed or clicked last, Enter is `=`). J2 asserts `78` then `+2` Enter -> `80`. |
| 2 | minor | 3 of 3 | No visible resize grip on the Calculator and no cue at the minimum, so J7 means guessing the corner (Sam 4, Alex 4). The kit `Window` draws the corner arc only on hover, in `--label-tertiary`, which is invisible on the always-dark body; touch has no hover. | **Open, shell** (Window chrome for every app: its baselines all change). Recommended as its own work item. |
| 3 | minor | Alex | Focus lands on the window's "..." button after the last key; typing there is ignored and Enter opens the menu. | Open, shell (the shortcuts listen inside the app view; the window chrome is outside it). |
| 4 | minor | 2 of 3 | The reason for an error is small mid-grey text above the large `Error`. | Deferred polish (a token step on `caption`). |
| 5 | minor | Alex, 3 of 3 over three rounds | The focus ring is faint on black and on the dark Dock. | Deferred, kit-wide token (audit: "muted blue but clearly visible"). |
| 6 | minor | Alex | Results are not announced after `=` (only errors are, through the status region). | Not changed: the readout is itself a polite live region (`live`), which announces each result; the status region is for what the display does not say. |
| 7 | minor | audit | In Split View, while the picker waits for its second app, the Dock floats over the bottom keypad row (`calculator-split-left` baseline). Once both halves are filled the Dock hides (checked in `verification.md`). | Open, shell (Dock inset in a split). |
| 8 | minor | audit | The view region is named with the app id (`calculator`). | **Fixed in this round**: `ViewHost` takes a `label`; the shell passes the app's display name. |
| 9 | polish | Priya | No hint that the keyboard works; a second Enter after `=` does nothing (Apple repeats the last operation); Enter and the focus ring on a clicked key. | Deferred; Enter after a click is `=` by design, Space presses the focused key. |
| 10 | polish | Alex | `5 + 5` sits close to the window handle at the minimum height; the Dock is cut off at a 480 px screen. | Deferred, shell. |
| 11 | polish | audit | Operator glyphs (17 px) look light beside the digits; `padding.left = 7` (28 px) on the wide `0` is not in the §5 list. | Deferred; the second is the iPadOS label inset. |
| 12 | open | Priya (rounds 1 and 2) | Operator precedence: the keypad chains left to right (`2 + 3 x 4` is 20), the `calculate` tool says 14. | **Open owner decision, AGOS-57.** Not changed. |

Resolved and confirmed by the panel: after clicks, Enter is `=` (Priya, Alex); the reason of an error is announced by the
status region and the region is emptied by AC or a digit (Alex's accessibility tree: `status: Can't divide by zero`, empty
before and after); keys fit and stay at least 44 pt at 320 x 500 (all three); Space and Enter press a key reached with Tab.

## Exit criteria

| Criterion | Result |
|---|---|
| Every journey succeeded for every participant | met (9 of 9 finished; 2 struggled on J7, the shell's resize grip) |
| Mean ease >= 6.0 | **not met** (5.7). The two ratings of 4 are the resize corner, which the Calculator does not own; without them the mean is 6.1. |
| No blocker or major open | met, except AGOS-57 (the owner's call); the one new major (Enter) was fixed and re-measured |
| Conformance >= 90, no category under 70 | met (96, lowest category 80) |
| Automated checks green | met (see the verification record) |

Left for the shell and kit, not the Calculator: a visible resize arc on every floating window (2), a way past the window chrome by
keyboard (3), Dock inset in Split View (7) and a stronger focus ring (5).

## Participant quotes

- Sam: "The calculator did what I asked: the error told me what was wrong, and clearing it let me carry on. The only thing I
  had to guess at was how to make the window smaller."
- Priya: "It feels like Apple's Calculator in a Stage Manager window, the keyboard and the 320-wide minimum just work, I only
  wish I could see where to grab the window corner."
- Alex, first pass: "It's a really solid keyboard calculator until Enter starts pressing whatever key I tabbed to instead of
  giving me the answer."
- Alex, after the fix: "Escape clears, Backspace fixes a slip, and Enter always means equals unless I've just tabbed onto a key.
  That's exactly how I'd want it, if only I could see the focus ring better."
