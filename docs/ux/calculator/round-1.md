# Calculator, UX research round 1

Focused round for AGOS-21 (the Calculator as the reference app). Panel of three, each on its own real host (the built
client, the Calculator installed), by visible labels and the accessibility tree only: **Sam** (Haiku, 54, not
technical, mouse), **Priya** (Sonnet, iPad power user, expects iPadOS behaviour), **Alex** (Opus, low vision,
keyboard, dark mode, 1024x768). Journeys J1, J2 (Priya, Alex), J3, J4, J7 from `apps/calculator/JOURNEYS.md`; J5 (an
agent over MCP) and J6 (Slide Over) are covered by their journey specs only, because a participant cannot call a tool and
Slide Over is shell behaviour. The conformance audit was scored by the moderator from the baselines and a live look, not
by a separate agent (see "Conformance").

## Scorecard

| Journey | Sam | Priya | Alex |
|---|---|---|---|
| J1 find it, 12 + 7 | success, 5 | success, 6 | struggled, 5 (`Plus` matched `Plus minus`, so 12 + 7 became -127) |
| J2 keyboard only | not run | success, 6 | success, 5 |
| J3 chain, (9 x 3) - 7 | struggled, 3 | struggled, 4 | struggled, 4 |
| J4 divide by zero, long numbers | success, 6 | success, 5 | success, 6 |
| J7 minimum window | success, 5 | success, 6 | struggled, 4 |

Success without struggling: 9 of 14 (64%). Mean ease **5.0** / 7. Exit criteria: not met (J3 struggled for all three, mean
ease under 6, majors open).

## Findings (merged, most severe first)

| # | Severity | Hit by | Finding |
|---|---|---|---|
| 1 | major | 3 of 3 | **The pending operator is not visible.** The operator key had no pressed look (only `aria-pressed`); the line above the display said `9 x 3` after `-` was pressed and `9 x 3` over `7` while typing, which reads like 9 x 3 = 7. |
| 2 | major | 2 of 3 | **The display was cut off**: `123,456,78...` for a typed nine-digit number and `9.9999999...` for e26, hiding the exponent, so 10^125 read as 1.2. The text "fit" shrank to a size that was half a pixel too wide, so it still ended in an ellipsis. |
| 3 | major | 1 of 3 | **Typing went nowhere after opening from the Dock by keyboard, or after a drag of the window** (focus was on the Dock icon or the window frame, and the window's shortcuts listen inside the window). |
| 4 | minor | 2 of 3 | After an error every key was dead with no feedback; a digit should start a new sum, as on an iPad. |
| 5 | minor | 2 of 3 | `Plus minus` and `Plus` share a prefix, so a name search or voice command for "plus" hit the wrong key. |
| 6 | minor | 3 of 3 | Digits past the ninth are dropped silently. |
| 7 | minor | 1 of 3 | Enter on a key focused with Tab acts as `=` instead of pressing that key. |
| 8 | minor | 1 of 3 | The focus ring is faint on the dark-grey keys against the black body. |
| 9 | minor | 2 of 3 | The Dock tile is an orange `+` with no name (shell); no visible resize grip and no hint the window reached its minimum (shell). |
| 10 | polish | 2 of 3 | The caption crowds the window grab dots at the minimum size; a stale caption stays above a fresh entry after `=`; no accessible name on the display. |

Positive, repeated: the error says why (`Can't divide by zero`, `Result too large`), better than iPadOS; the keyboard set
(digits, `+ - * / . = Enter Backspace Escape`) works; the keys stay round and about 68 px at 320 x 500; light and dark
shells around the always-dark body look right.

## Changes made after round 1

1. **Kit, `Button`**: a pressed (`aria-pressed`) data-colour button swaps fill and ink and gains a ring in the fill
   colour (`packages/ui/src/atoms/Button.svelte`, story "Pressed data colour", light and dark). The ink is the one chosen for
   4.5:1 on the fill, so the swap keeps the contrast. Fixes 1.
2. **App, caption**: pressing an operator shows what is waiting (`27 -`), then the whole sum after `=` (`27 - 7`). Fixes 1.
3. **Kit, `Text fit`**: re-measure when the text changes (a `MutationObserver`), and fit 1% under the box so a sub-pixel
   overshoot never becomes an ellipsis; a story "Display readout follows its text" and the J7 spec check the glyphs fit.
   Fixes 2.
4. **Client, `ViewWindow` and `ViewHost`**: a window opened from the Dock (or with nothing focused) takes the keys; the
   view root is a focus target (`tabindex="-1"`, `data-window-focus`). Fixes 3 (opening).
5. **App**: a digit after an error starts a new sum; `Plus minus` is now `Change sign`. Fixes 4 and 5.
6. **Client, shortcuts**: `"+"` parsed to nothing, so the `+` key never worked from the keyboard; a trailing `+` is now the
   key. Found while writing J2. An invisible `=` shortcut was added next to Enter.
7. Failures say why in plain sentences (no `calc.lua:56:` prefix), also for agents; `%` by zero and `10 ^ 400` have their own
   reasons instead of "Malformed expression".

Not changed: 6 (silent digit cap, as on iPadOS; a deferred polish item), 8 (the focus ring is a kit-wide token), 9 (shell:
Dock tile and resize grip).

## Conformance (moderator score, against `.claude/skills/app-design/CONFORMANCE.md`)

Colour and tokens 95 (semantic tokens only; the always-dark `Scope` is the documented instrument palette), Flat 100,
Materials 100 (none used), Typography 90 (display style only, fit down to 0.3), Layout and radii 95, Resize 95 (min 320 x 500
declared with its reason; desktop baselines at every layout and size), Accessibility 80 (axe clean light and dark, 68 px
keys, but the faint focus ring on dark keys), Controls 90, Content and states 95. Weighted **92**, no category under 70.
