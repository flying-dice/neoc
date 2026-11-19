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

## How

`bun run lint`, `bun run typecheck`, `bun run test`, `bun run test:e2e` (counts in the commit message and the report),
`AGENTOS_PORT_BASE=9900`; changed and new baselines reviewed as a contact sheet; driven with `e2e/tools/session.mjs`
and a scripted probe against the real host (`cargo run -p agentos-host`, ports 9921 to 9924, stopped afterwards); round 3 and the Split View and Slide Over pass ran on ports 9921 to 9926, all stopped afterwards.

## Not checked

Text size at 125% (the session tool has no control for it), a screen reader (only the accessibility tree and the
status region were inspected), and Split View and Slide Over in the dark shell beyond the desktop baselines.
