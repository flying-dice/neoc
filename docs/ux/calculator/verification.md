# Calculator: manual verification (AGOS-21)

## Intended result (written before looking)

- At the 320x500 minimum every key is a full circle of at least 44 px and nothing is clipped; the longest number the keypad
  takes (`123,456,789`) and the longest result (`9.99999998e17`) are shown in full, never ended with an ellipsis.
- The pending operator is visible on its key (an outlined key) and in words above the display (`27 -`), in light and dark
  shells alike; the body stays the dark instrument canvas.
- A failed sum shows `Error` and its reason under it (`Can't divide by zero`, `Result too large`); AC or a digit gets out.
- A window opened from the Dock takes the keyboard; the keys work after a drag of the resize corner; Enter presses a key
  walked to with Tab and is `=` after keys were clicked or typed (decided by what the person last did: Tab, a click or touch, or typing; not by `:focus-visible`).
- The reason for an error is announced by the window's always-present announcer (`role=status`), not by a live region
  that appears with its text.
- In Split View and in Slide Over the keypad is fully visible and works from the mouse and the keyboard alike.
- The kit changes (pressed data-colour Button, readout that follows its text, Window focus hand-off) change nothing else.

## Checked

Ran the shell on the real host headless (Chromium 1280x800, 1024x768 for the panel), on ports of `AGENTOS_PORT_BASE=9900`:

- [x] Light and dark shell, pending `-` after `9 x 3`: outlined `-` key, caption `27 -`, display `27`; the body is dark in both.
- [x] Error state: `Error` with `Can't divide by zero` above it (dark; the same text is asserted in J4).
- [x] Nine typed digits at a shrunk window: `123,456,789` fits the display (dark, window at about its 320x500 minimum).
- [x] Pressed data-colour Button, light and dark (new stories `Atoms/Button - Pressed data colour`): orange, blue, green and
  gray4 each show the swapped fill and the ring; text stays readable in both themes.
- [x] `Display readout follows its text` story: the readout shrinks as the text grows, in both themes.
- [x] Before and after: `renderer-kit-controls--display-min` changed by the 1% extra shrink of the fit (the long readout is
  about 1% smaller, nothing else moved); looked at both PNGs.
- [x] Keyboard, driven by the panel and by journeys J1, J2, J7: typing right after opening from the Dock, `+ - * / . = Enter
  Backspace Escape`, Tab to a key and Enter presses it, typing after a resize.
- [x] The UX panel (three rounds, `round-1.md`, `round-2.md`, `round-3.md`): three personas, light and dark, 1024x768 and 1280x800.

## Checked after the review (round 3)

Driven with `e2e/tools/session.mjs` on a fresh host (1280x800, light shell), screenshots looked at:

- [x] **Split View**: Calculator on the right, Files on the left (`Split View Right` from the multitasking menu, then Files from
  the Dock). The whole keypad, the `0` key included, is visible and none of it is under the Dock (the Dock hides once both
  halves are filled). Seen while a split waits for its second app ("Choose an app from the Dock"): the Dock floats over the
  bottom row of the keypad until an app is chosen. That is shell behaviour, as on iPadOS, and not changed here.
- [x] **Keyboard in Split View**: tapped `7`, typed `+3`, pressed Enter: `10` with the caption `7 + 3` (Enter is `=` after a
  click and typed keys; the ring stays on the tapped `7`). Tab to `8`, Enter: it pressed `8` (display `8`, the clear key is
  `C`). Before the fix the first of these showed `37`.
- [x] **Slide Over**: Slide Over from the menu: the Calculator is a narrow window over Files, all keys and the display visible,
  the clear key shows `C` while a number is typed. Tapped `0`, typed `+5`, pressed Enter: `85` (the caption `80 + 5`).
- [x] **Error announcement**: after `8 / 0 =` the page's `status` region holds `Can't divide by zero` (the region exists
  before the error, so a screen reader hears the change); after AC it is empty. Journey J4 asserts both, and
  `calculator_app.rs` asserts the `w:announce` commands.
- [x] **Keyboard coverage**: J2 now covers typed digits and operators with `=`, Enter, Backspace and Escape, a key reached with
  Tab pressed by Enter and Space, Enter after typing on a key reached with Tab (78, `+2` -> `80`), Enter after clicked keys (7 + 2 -> `9`), Enter after typing on a clicked key
  (7, `+3` -> `10`), and a click taking Enter back after a Tab. With the earlier `:focus-visible` check the journey fails
  (`22` for `9`); with the input-modality check it passes.
- [x] Light and dark in Split View and Slide Over: the desktop spec's baselines (`layout: split-left`, `layout: slideover`,
  light and dark, with axe) are green; the interactive pass above was in the light shell, the panel's dark pass (Alex,
  round 3) covered the dark shell.

## Checked: the resize corner (kit `Window`)

Intended result, written before looking: a floating window shows a short arc in its bottom-right corner at all times (no hover
needed), flat, in one grey (`--window-grip`, `#757579`) that holds at least 3:1 on the light window (`#f2f2f7`, 4.1:1), the
dark window (`#1c1c1e`, 3.7:1) and the Calculator's black body (4.6:1); a drag still resizes and stops at the manifest
minimum; Tab reaches the grip, which has the name "Resize Calculator" and a visible focus ring, and the arrow keys resize
it by 16 px (Shift: 64 px) down to the minimum; a drag leaves the keys with the keypad.

- [x] Light shell, Calculator at its preferred size (340 x 560): the arc sits inside the corner, concentric with the window's
  radius, calm next to the "..." dots; it does not touch the `=` key.
- [x] Dark shell, same: visible on the black body and on the dark window of Files.
- [x] Minimum window: dragging the corner far past the limit stops at 320 x 500 and the whole keypad is visible with the arc in
  the corner (`calculator-min` baseline and the live look).
- [x] Keyboard: 21 Tabs from a clicked key reach the grip (named "Resize Calculator"); a blue focus ring shows around it;
  three Right arrows grew the window from 320 to 368 wide, six Left arrows brought it back to 320.
- [x] The kit story `Organisms/Window`: "Resize corner" (light and dark themes), "on a black body" and "with the keyboard"
  assert the grip's computed colour against the window and against `rgb(0, 0, 0)` (at least 3:1), the accessible name, the
  arrow steps, Shift's bigger step and the stop at the minimum.
- [x] Baselines: `bun run test:e2e:update` left no baseline changed on its own, because the screenshot tolerance
  (`maxDiffPixelRatio` 0.002) hides a 16 px arc. The desktop project was therefore rewritten with
  `--update-snapshots=all`, which touched 49 files. A pixel diff against the previous baselines (in Chromium, per file)
  showed 43 with the new arc (36 of them nothing else, 7 on Discover or Settings with a few hundred pixels of unrelated
  moving text such as "2 min ago" next to it), and 6 with no arc (the fullscreen, Split View and Slide Over layouts have no grip; they differed by nothing
  or by moving text only); those 6 were put back. Six new kit baselines
  for the Window story were added. A contact sheet of the corner of all 43 and the 6 new ones was looked at, light and dark,
  white, grey and black bodies: the arc is the same crisp shape and place in every one.

## Checked: the limit cue, the bolder grip and the window manager (second re-run)

Intended result, written before looking: when a resize, by drag or arrow keys, reaches the minimum size or the screen's edge,
the window says so without anything loud. The grip nudges back once (not under `prefers-reduced-motion`) and takes the
`--tint`; a small flat capsule over the window's bottom edge, beside the grip, names the limit for about two seconds and covers
none of the keys' circles; a polite live region says the full sentence once; at the smallest size the grip stays tinted. The
grip is 24 px with a 5 px stroke, still `--window-grip` grey at rest (3:1 or more on light, dark and black), with the tooltip
"Resize — drag or use arrow keys", and its 44 pt button is mostly outside the window so a click on the lower-right edge of the
`=` key still presses `=`. Split View never lets a pane under its app's minimum; Slide Over and a split that waits for its second
app stop above the Dock.

Driven on fresh hosts with `e2e/tools/session.mjs` and a scripted probe (Chromium 1280x800 and 1024x768), screenshots looked at:

- [x] **Light shell, minimum window (320 x 500)**: dragging the corner far past the limit stops at 320 x 500, the arc is blue,
  the label "Smallest size" sits across the bottom edge under the `.` key and covers no key; the status region reads "Calculator
  is at its smallest size" at once. About two seconds later the label and the sentence are gone and the arc stays blue.
- [x] **Dark shell, same**: the same, with the dark tint; the label reads (white on the tinted fill) and the arc is clear on the
  black body.
- [x] **At its preferred size** (340 x 560): the grip is grey, bolder than before, inside the corner, not touching `=`.
- [x] **Keyboard**: Tab reaches "Resize Calculator" (21 Tabs from the first key; last in the tab order on purpose, see the `Window`
  header), the focus ring goes round the arc, Shift+Arrow resizes by 64 px, the status says "can't get any narrower" when only
  the width is at its minimum and "is at its smallest size" when both are; the label says "Narrowest" and then "Smallest size".
  The story "Resize limit cue" asserts the same, the tint's contrast against the window and against black (3:1 or more), and that
  the cue clears.
- [x] **Reduced motion**: with `prefers-reduced-motion: reduce` the computed animation of the arc is `none` and the colour is
  still `rgb(0, 102, 204)`; without it the animation is `grip-push-out`. The tint and the label carry the cue.
- [x] **The `=` key**: a click at the lower-right edge of the key, inside its circle, at the minimum size presses `=` (the audit
  agent clicked four points there). With the earlier 40 px grip it did not.
- [x] **Split View**: the divider dragged as far as it goes leaves the Calculator at 320 px or more (the audit looked at 1024x768,
  where it used to reach about 250 px and cut the keypad off); while a split waits for its second app the first pane and the
  empty side end at the same edge above the Dock and the empty side follows the divider (the empty side was a fixed 50 % that ran
  behind the Dock; the audit that found it saw it on the build before the fix, the tests and the baselines cover the fix).
- [x] **Slide Over at 1024x768**: the window ends above the Dock (it ran 26 px behind it, over the `0` key's corner).
- [x] **Baselines**: the grip and the layouts changed the baselines, so the whole suite was rewritten with
  `--update-snapshots=all` (148 files byte-different, 63 changed in pixels because of this change, 85 only by moving text or
  anti-aliasing and put back, 2 new for the new story, 6 Window story baselines among the 63). Pixel diff of every changed
  file against HEAD: the 7 `*-min` files and 7 `*-large` differ only by the 23 x 23 px corner (208 px of tinted arc at the minimum,
  137 px of grey at the large size); the 14 floating, 7 `screen-ipad` and 7 `screen-large` files by the grey arc alone (Discover
  adds the moving "2 min ago" text beside it); the 7 `split-left` and 7 `slideover` files by the layout change (the pane and the
  empty side now stop above the Dock; Slide Over ends 26 px higher), looked at for the Calculator and in a contact sheet for
  Discover and Files; the 6 Window story baselines by the larger arc, and the 2 new ones show the grip tinted at 320 x 240 with
  no label. A contact sheet of the lot was looked at, light and dark. The first rewrite caught the label half faded in `*-min`,
  `*-large` and the keyboard story: `resizeWindow` (`e2e/fixtures.ts`) and the story now wait the cue out before the snapshot,
  and those were rewritten again.

## How

`bun run lint`, `bun run typecheck`, `bun run test`, `bun run test:e2e` (counts in the commit message and the report),
`AGENTOS_PORT_BASE=9900`; changed and new baselines reviewed as a contact sheet; driven with `e2e/tools/session.mjs`
and a scripted probe against the real host (`cargo run -p agentos-host`, ports 9921 to 9924, stopped afterwards); round 3 and the Split View and Slide Over pass ran on ports 9921 to 9926, all stopped afterwards.

## Not checked

Text size at 125% (the session tool has no control for it; the third audit agent set the root font size by hand and found nothing clipped, and Text Size does not scale the px type tokens at all), a screen reader (only the accessibility tree and the
status region were inspected), and Split View and Slide Over in the dark shell beyond the desktop baselines.
