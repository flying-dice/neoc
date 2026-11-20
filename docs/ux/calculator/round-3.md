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
shell's). Mean ease **5.6** / 7 (45 over 8 ratings, with Alex's J2 re-run; 5.4 for the first pass alone; round 2 was 5.6). An
earlier version of this file said 5.7 from "51 over 9 ratings"; that was a miscount (Sam has no J2 rating), corrected here.
The J7 re-run below moves it to **5.75**.
Conformance **96** (audit agent, separate from the moderator; round 2: 92).

### Conformance, per category (audit agent)

Colour and tokens 100, Flat 100, Materials 100, Typography 95, Layout and radii 98, Resize and multitasking 80, Accessibility
100, Controls 100, Content and states 95. No category under 70.

## Findings

| # | Severity | Hit by | Finding | Status |
|---|---|---|---|---|
| 1 | major | Alex | Enter was inconsistent: after Tab to a key and typing `+2`, Enter pressed the focused key again (`88 + 28`), while after a click it meant `=`. | **Fixed in this round**: a key tabbed to and then typed on is not being navigated; Enter is `=` (`view/core/modality.ts`, `typing`). Re-run: Alex J2 ease 7, "the rule feels consistent and easy to predict" (Tab then Enter presses; typed or clicked last, Enter is `=`). J2 asserts `78` then `+2` Enter -> `80`. |
| 2 | minor | 3 of 3 | No visible resize grip on the Calculator and no cue at the minimum, so J7 means guessing the corner (Sam 4, Alex 4). The kit `Window` drew the corner arc only on hover, in `--label-tertiary`, which is invisible on the always-dark body; touch has no hover. | **Grip fixed after this round** (always visible, 3:1 in light, dark and black, a button with arrow-key resize); re-measured in the J7 re-run below. The cue at the minimum stays open. |
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
| Mean ease >= 6.0 | **not met** (5.6; 5.75 after the J7 re-run below). The two ratings of 4 were the resize corner, which the Calculator does not own. |
| No blocker or major open | met, except AGOS-57 (the owner's call); the one new major (Enter) was fixed and re-measured |
| Conformance >= 90, no category under 70 | met (96, lowest category 80) |
| Automated checks green | met (see the verification record) |

Left for the shell and kit, not the Calculator: a cue at the minimum size (2), a way past the window chrome by keyboard (3),
Dock inset in Split View (7) and a stronger focus ring (5). The visible resize arc (2) was done after this round.

## Participant quotes

- Sam: "The calculator did what I asked: the error told me what was wrong, and clearing it let me carry on. The only thing I
  had to guess at was how to make the window smaller."
- Priya: "It feels like Apple's Calculator in a Stage Manager window, the keyboard and the 320-wide minimum just work, I only
  wish I could see where to grab the window corner."
- Alex, first pass: "It's a really solid keyboard calculator until Enter starts pressing whatever key I tabbed to instead of
  giving me the answer."
- Alex, after the fix: "Escape clears, Backspace fixes a slip, and Enter always means equals unless I've just tabbed onto a key.
  That's exactly how I'd want it, if only I could see the focus ring better."

## J7 re-run, after the resize corner was made visible

The kit `Window` now draws its corner grip at all times (it was hover only, in a grey that vanished on a black body):
`--window-grip` `#757579`, 3px, 4.1:1 on the light window, 3.7:1 on the dark one and 4.6:1 on the Calculator's black body,
and the grip is a button (named "Resize Calculator") that the arrow keys resize (Shift for 64 px). Same three personas, fresh
sessions and hosts, only J7; the participants were not told what had changed.

| Journey | Sam | Priya | Alex |
|---|---|---|---|
| J7 minimum window | struggled, **4** (was 4) | success, **6** (was 6) | success, **5** (was 4) |

All three found the grip without help this time (round 3: two of three guessed the corner). Priya: "dragged the corner once, it
stopped where it should, every key was still there". Sam found it by looking closely ("a tiny, faint grey curve") and still
rated 4 because nothing says the window has reached its minimum: the second drag does nothing visible. Alex reached the grip
by Tab (21 presses, last in the tab order), guessed the arrow keys and Shift, and again saw nothing at the minimum.

Mean ease over all ratings of the round, with the re-runs of Alex's J2 (7) and of J7 (4, 6, 5): **5.75** / 7 (46 over 8).
The target of 6.0 is still not met.

New and remaining findings from the re-run:

| # | Severity | Hit by | Finding | Status |
|---|---|---|---|---|
| 13 | minor | Alex | The grip gave no hint of how to use it. | **Fixed after the re-run**: it has the description "Arrow keys resize, Shift for larger steps" (`aria-describedby`; no visual change). |
| 14 | minor | Sam, Alex | No cue at the minimum size (a second drag, or more arrow presses, change nothing visibly). | Open: a cue such as a stopped bounce or a live-region size announcement is a design decision for the shell. |
| 15 | minor | Sam, Alex | The grip is small and faint (Sam: "about 12px"; Alex: low vision). It meets 3:1, as asked, as a subtle flat arc. | Kept: a thicker or larger grip is a styling call for the owner. |
| 16 | minor | Alex | The grip is last in the tab order (21 Tabs); Shift+Tab leaves the window. | Open, shell (window chrome order). |
| 17 | polish | Priya, Sam | A thumb hit area of 44 pt, minimum feedback, an unlabeled Dock and recent-apps thumbnail, the `5 + 5` line moving the keys 4 px. | Deferred. |

### Conformance re-score, Resize and multitasking (separate audit agent, Opus)

**80, unchanged** (total stays 96). The auditor confirmed the grip (always visible, 3:1 or better in all three bodies), the
arrow-key resize and the stop at the manifest minimum (320 x 500) live and in the baselines and the kit story, and found the
automated checks green (31 passed for the Calculator and Window specs). It still deducts 20 for the one cut-off it sees: in
Split View, while the picker waits for the second app, the Dock floats over the keypad's bottom row (`calculator-split-left`
baseline; once the second app is chosen the whole keypad is visible, also live). That is the shell's (Jira), and the rubric
deducts only for cut-offs, so the better grip adds no points.

## Second re-run: the minimum-size cue, a bolder grip, and J2 for Sam

What changed in the kit and shell (the Calculator itself changed only in two small ways, see the commit):

- **Cue at the limit** (kit `Window`): when a resize, by drag or by arrow keys, reaches the minimum size or the screen's edge, the
  grip nudges back once (no nudge under `prefers-reduced-motion`; checked live with the media query on and off) and takes the
  `--tint`, a flat capsule beside the grip says which ("Smallest size", "Narrowest", "Shortest", "Widest", "Tallest", "Largest
  size") for about two seconds, and a polite live region says the full sentence ("Calculator is at its smallest size", "can't
  get any narrower", ...) once, not on every key press after it. At its smallest size the grip stays tinted, whenever you look.
- **Grip**: 24 px with a 5 px stroke (was 16 and 3), a 44 pt button that sits mostly outside the window so it does not cover the
  `=` key's circle, the tooltip "Resize — drag or use arrow keys", the same `--window-grip` grey (3:1 or more on light, dark and
  black). It stays **last in the tab order** on purpose (the reasoning is in the `Window` header: it is window chrome in the
  bottom-right corner; first would cost every keyboard user a stop in every window). Alex's "21 Tabs" is therefore still true.
- **Window manager** (found by the audit, see below): the split divider never leaves a pane narrower than its app's minimum; Slide
  Over and a split that waits for its second app stop above the Dock; the empty side of a waiting split is sized from the split
  ratio and ends at the same edge as the first pane.

Same three personas (Sam/Haiku, Priya/Sonnet, Alex/Opus), fresh sessions on fresh hosts, labels and the accessibility tree
only, not told what changed. Sam also ran J2, which he had no rating for.

### J7, by build (every run, none left out)

The cue went through three builds before the shipped one, because each run found something. All runs, in order:

| Build | Sam | Priya | Alex | What the run found |
|---|---|---|---|---|
| Visible grip (first re-run, above) | 4 | 6 | 5 | no cue at the minimum, grip small and faint |
| A: tint at the smallest size, live-region sentence, tooltip | 5 | 6 | 6 | nothing visible in words; the cue was colour only |
| B: A + a label below the corner, 24 px grip | 6 | 6 | 6 | the label can sit under the Dock for a tall window |
| C: B with the label inside the window, beside the grip | 6 | 6 | 5 | the label covered the `.` and `=` keys for two seconds (all three said so) |
| **D, shipped**: the label over the window's bottom edge, 44 pt button outside the window | 6 | 6 | 6 | the label no longer covers keys; the tab order and focus ring remarks stay |

The shipped build is D. B and D score alike; C scored 17 and is not the build that shipped. The spread between builds is one
point on one participant, which is the noise of three agents; I report D's ratings as the measure and list the others so it
can be seen that nothing was dropped.

### Scorecard, round 3, with every re-run (shipped build)

| Journey | Sam | Priya | Alex | Sum |
|---|---|---|---|---|
| J2 keyboard only | **5** (first rating) | 6 | 7 (re-run after the Enter fix) | 18 |
| J4 divide by zero | 6 | 6 | 6 | 18 |
| J7 minimum window (build D) | 6 | 6 | 6 | 18 |
| Sum | 17 | 18 | 19 | **54** |

Mean ease: 54 / 9 ratings = **6.00** / 7 (round 3 first pass: 5.4; after Alex's J2 re-run: 5.6; after the first J7 re-run:
5.75). Every journey succeeded for every participant. This is the first measurement with a rating for every persona and
journey of the round. It meets 6.0 exactly, not with room to spare: build C of the same run would have given 53 / 9 = 5.89.

Sam's J2 (5): he typed `250*0.18` and Enter, found Escape to clear by guessing, and Backspace for the slip; "I did not know a
keyboard route existed", "nothing on screen says the keyboard works", the focus ring sits on `=` after every key and looks
like a pressed key, and the Dock icon is an unlabelled orange "+" that he found through its accessibility label. These are
findings 9 (no keyboard hint) and the Dock icon, both left as they were (below).

### What the shipped build's participants still said

| # | Severity | Hit by | Finding | Status |
|---|---|---|---|---|
| 14 | minor | Sam, Alex | No cue at the minimum. | **Fixed**: nudge, tint, label, live region (above). Sam, Priya and Alex all saw it ("better than iPadOS, which gives no cue"). |
| 15 | minor | Sam, Alex | The grip is small and faint. | **Done as far as the 3:1 rule allows**: 24 px, 5 px, 44 pt target. Sam still calls it "easy to miss" (polish): the grey cannot go darker without losing 3:1 on one of light, dark or black. |
| 16 | minor | Alex | Last in the tab order (21 to 22 Tabs), and typed digits do nothing while the grip has focus. | **Kept, with the reason in the code**. Open, shell: a window-level resize shortcut or a way past the window chrome (finding 3). |
| 18 | polish | Priya, Sam | The tint stays after the drag (it is the smallest-size state), and the grip sits at the `=` key's corner. | By design (a cue that is true whenever you look; the button is outside the key's circle: clicking the lower-right of `=` reaches `=`, checked by the audit). |
| 19 | polish | Alex | The focus ring is faint on black; the result is not announced after `=`; the display has no label. | Deferred (findings 5 and 6; the readout is a polite live region named "Result"). |
| 20 | polish | Sam | The Dock icon is an orange "+" with no visible name. | Deferred: a calculator glyph needs an icon in the kit and the manifest enum. |
| 21 | polish | Priya | The keys move 4 px when the `5 + 5` line appears. | Deferred. |

### Conformance, three independent audit agents (Opus), and what they found

The same rubric ([CONFORMANCE.md](../../../.claude/skills/app-design/CONFORMANCE.md)), the same instructions to be strict, a fresh
agent each time; none of them is the moderator. They did not agree, so all three are listed:

| Audit | Build it saw | Total | Resize | What it found |
|---|---|---|---|---|
| Round 3 | before the grip work | 96 | 80 | Dock over the keypad while a split waits for its second app |
| First of this round | A (grip 36 px, tint cue) | **89.3** | 40 | the split divider let the Calculator pane go to 250 px (below its 320 minimum) at 1024x768, Slide Over under the Dock at 1024x768, the grip's box over the `=` key |
| Second | B (grip 40 px, label below) | **86.7** | 40 | the same, plus raw `#fff`, `999px`, `2px 10px` in the label's CSS, a raw Slide Over shadow, "Largest size" for a single axis, the 40 px button |
| Third (window manager fixes, 44 pt grip, tokens; label inside the window) | C | 98.0 | 100 | the empty side of a waiting split ended 94 px lower than the first pane and did not follow the divider (Layout, -20); the label covered the `.` and `=` keys for two seconds (not deducted, "if you count it, 95") |
| **Fourth, on the shipped source** | D | **96.6** | **100** | the "..." handle's dots are `--label-secondary` on the black body: about 1.5:1 in the light shell (Colour, -15; the same failure `--window-grip` fixed for the corner); the handle button is 43 x 15, under 44 pt (Accessibility, -5); two off-grid paddings on the handle (Layout, -4). Not deducted: leaving Slide Over for Full Screen or Split View sends the only window to the recent-apps strip (shell, predates this change); the capsule's lower half sits under the Dock for a window that ends at the Dock |

**The conformance score to quote is 96.6, from the fourth audit agent (Opus, separate from the moderator), on the build that ships.**
Resize and multitasking is 100 (was 80), the other categories 85 to 100, no category under 70.

What I did about the first three audits' findings: the window manager clamps the divider to each pane's app minimum and stops
Slide Over and a waiting split above the Dock (`windowManager.svelte.ts`, tests in `windowManager.test.ts`); the grip's button
is 44 pt and outside the window; new tokens `--on-tint`, `--radius-pill` and `--shadow-slideover`, documented in
`DESIGN_LANGUAGE.md` (as is the 28 px inset of the wide `0`, which the audits had counted against the Calculator); "Widest",
"Tallest" and "Largest size" separate the axes; the label moved from inside the window to over its bottom edge (all three
participants of build C asked for it); the empty side of a waiting split is sized from the split ratio and ends at the first
pane's bottom edge (`splitPlaceholderFrame`, with a test). **Not fixed, reported by the audits**: the "..." handle's dots on a
black body in the light shell (about 1.5:1, a kit change that touches every window's baseline) and its 43 x 15 px target;
Text Size does nothing to px type tokens (kit-wide; the second audit counted it, the others did not); `--on-tint` is white
and holds 5:1 only on the default blue; a frame left by a device with a bigger screen is not clamped on a 390 px screen
(shell); the Slide Over exit above. The scores moved with what each audit tested (86.7 to 98.0); the spread is the rubric's
cut-off deductions, which depend on which layouts and sizes an auditor opens, and on how it reads the categories the rubric
does not spell out, not noise in the Calculator.

### Exit criteria, after the second re-run

| Criterion | Result |
|---|---|
| Every journey succeeded for every participant | met (9 of 9 ratings, all finished) |
| Mean ease >= 6.0 | **met: 54 / 9 = 6.00** (build D; 5.89 with build C, see above) |
| No blocker or major open | met except AGOS-57 (the owner's call); Sam's two "major" labels were the grip and the missing cue, both addressed |
| Conformance >= 90, no category under 70 | met: **96.6** by the fourth audit, on the shipped build (lowest category 85); the earlier audits, of earlier builds, scored 89.3, 86.7 and 98.0 and are recorded above |
| Automated checks green | met (see the verification record) |
