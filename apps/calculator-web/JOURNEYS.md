# Calculator (web): key user journeys

The Calculator built the new way (NEOC-298): a Vite project whose client (Svelte, on the kit's `ui-button`) runs in a frame of
its window, or alone on its own page at `/apps/calculator-web`, and whose arithmetic is Luau behind one method, `press`. It
lives beside the old Calculator (`apps/calculator`, a view-tree app) until it does everything that one does; then it takes the
id `calculator`. The behaviour is the old one's: `apps/calculator/JOURNEYS.md`, "How a sum is worked out", holds for this
keypad too, and `crates/vos-lua/tests/calculator_web.rs` runs the old keypad tests' tables against it.

What differs from the old one: each window has a calculator of its own (the client holds the keypad's state; the backend keeps
nothing), and there are no agent tools yet (`calculate`, `history`, `show_on_calculator` come with the next part of NEOC-298).

Automated versions live in `e2e/journeys.spec.ts` with the same ids, against the real host (the e2e host installs the app at
start), through the app's frame (`calculator-web-frame`).

| Id | Who / goal | Starts at | Done when |
|---|---|---|---|
| CW1 | Someone splitting a bill: work out 12 + 7 with the mouse. | The desktop. The Calculator isn't open. | They opened it from the home screen, and the display shows 19 with the caption `12 + 7`. |
| CW2 | A keyboard user: 250 × 0.18 without the mouse, clear it, then 12 + 7 with a slipped digit fixed. | The Calculator open. | The display shows 45, then 0, then 19, by keyboard only (digits, operators, `=` or Enter, Backspace, Escape). |
| CW3 | Chain and precedence: 2 + 3 × 4, the way a phone does it. | The Calculator open. | After `2 + 3 ×` the display still shows 3, the caption reads `2 + 3 ×` and the × key is drawn pressed; `=` shows 14. |
| CW4 | Make a mistake and recover: divide by zero, then a new sum. | The Calculator open. | They saw "Error" with "Can't divide by zero" under it (announced to a screen reader), cleared it and finished a new sum. |
| CW5 | Use it in a small window: shrink it as far as it goes and do 5 + 5. | The Calculator open. | The window stopped at its minimum (320x500); every key is fully visible, and the display shows 10. |
| CW6 | Open it on its own: go to `/apps/calculator-web`, type 6 × 7 straight away, then tap 8 ÷ 2. | A paired browser. | The page is the Calculator full screen (no shell), titled with its name; the keyboard works without a click and shows 42, the keys then show 4; in dark and in light it is the same dark instrument. |
