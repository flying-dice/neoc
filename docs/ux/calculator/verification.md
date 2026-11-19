# Calculator: manual verification (AGOS-21)

## Intended result (written before looking)

- At the 320x500 minimum every key is a full circle of at least 44 px and nothing is clipped; the longest number the keypad
  takes (`123,456,789`) and the longest result (`9.99999998e17`) are shown in full, never ended with an ellipsis.
- The pending operator is visible on its key (an outlined key) and in words above the display (`27 -`), in light and dark
  shells alike; the body stays the dark instrument canvas.
- A failed sum shows `Error` and its reason under it (`Can't divide by zero`, `Result too large`); AC or a digit gets out.
- A window opened from the Dock takes the keyboard; the keys work after a drag of the resize corner; Enter presses a key
  reached with Tab and is `=` otherwise.
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
- [x] The UX panel (two rounds, `round-1.md`, `round-2.md`): three personas, light and dark, 1024x768 and 1280x800.

## How

`bun run lint`, `bun run typecheck`, `bun run test`, `bun run test:e2e` (counts in the commit message and the report),
`AGENTOS_PORT_BASE=9900`; changed and new baselines reviewed as a contact sheet; driven with `e2e/tools/session.mjs`
and a scripted probe against the real host (`cargo run -p agentos-host`, ports 9921 to 9924, stopped afterwards).

## Not checked

Text size at 125% (the session tool has no control for it), a screen reader (only the accessibility tree and the
`aria-live` attribute were inspected), and the Calculator in Split View beyond the generic desktop baselines.
