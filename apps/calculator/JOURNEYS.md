# Calculator: key user journeys

The Calculator is a Vite project (NEOC-298): its client, a Svelte keypad on the kit's `ui-button`, runs in a frame of its window
or alone on its own page at `/apps/calculator`, and its arithmetic and its agent tool are Luau. The client holds the keypad's
state, so each window and the standalone page have a calculator of their own and a sum is lost when the page closes or
reloads; the backend keeps
nothing. `crates/vos-lua/tests/calculator.rs`
holds the keypad to the tables below and tests the tool.

Each journey is a goal, not a script. Automated versions live in `e2e/journeys.spec.ts` with the same ids, against the real
host (the e2e host installs the app at start), through the app's frame (`calculator-frame`) or its own page.

| Id | Who / goal | Starts at | Done when |
|---|---|---|---|
| CW1 | Someone splitting a bill: work out 12 + 7 with the mouse. | The desktop. The Calculator isn't open. | They opened it from the home screen, and the display shows 19 with the caption `12 + 7`. |
| CW2 | A keyboard user: 250 × 0.18 without the mouse, clear it, then 12 + 7 with a slipped digit fixed. | The Calculator open. | The display shows 45, then 0, then 19, by keyboard only (digits, operators, `=` or Enter, Backspace, Escape). |
| CW3 | Chain and precedence: 2 + 3 × 4, the way a phone does it. | The Calculator open. | After `2 + 3 ×` the display still shows 3, the caption reads `2 + 3 ×` and the × key is drawn pressed; `=` shows 14. |
| CW4 | Make a mistake and recover: divide by zero, then a new sum. | The Calculator open. | They saw "Error" with "Can't divide by zero" under it (announced to a screen reader), cleared it and finished a new sum. |
| CW5 | Use it in a small window: shrink it as far as it goes and do 5 + 5. | The Calculator open. | The window stopped at its minimum (320x500); every key is fully visible, and the display shows 10. |
| CW6 | Open it on its own: go to `/apps/calculator`, type 6 × 7 straight away, then tap 8 ÷ 2. | A paired browser. | The page is the Calculator full screen (no shell), titled with its name; the keyboard works without a click and shows 42, the keys then show 4; in dark and in light it is the same dark instrument. |
| CW8 | A mixed sum the way a phone does it: 2 + 3 × 4, then 2 × 3 + 4, by mouse and by keyboard, and the same sums from an agent. | The Calculator open. | The display shows 14 (not 20), then 10, and the `calculate` tool gives the same answers. |

## How a sum is worked out

The keypad (`keypad.luau`) and the `calculate` tool run one evaluator (`calc.luau`), so a sum gives the same answer from both: `×` and
`÷` bind tighter than `+` and `−`, and operators of equal precedence go left to right (`8 − 2 − 1` is 5, `8 ÷ 4 ÷ 2` is
1). The keypad shows the sum the way the iPadOS Calculator does:

- **The display** shows what the operator just pressed has completed, not the last number typed. `2 + 3 ×` shows 3
  (nothing is complete), `2 × 3 +` shows 6, `2 + 3 × 4 +` shows 14 and `2 + 3 × 4 ×` shows 12. While a number is being
  typed, the display shows it.
- **The caption** is the sum so far, in words: `2 + 3 ×` while an operator waits, the whole sum (`2 + 3 × 4`) once `=` is
  pressed. Typing the next number leaves it alone. It is one line: a long chain keeps its last terms behind an ellipsis
  (`… 3 + 4 + 5`).
- **The operator key** of the waiting operator is `selected` (and `aria-pressed`) until a number is started, or `±` or
  `%` turns the display into one.
- **`=`** evaluates the whole sum. With an operator waiting, the displayed number is the operand: `5 + =` is 10,
  `2 + 3 × =` is `2 + 3 × 3` = 11. A second `=` does nothing (the iPadOS Calculator repeats the last operation; this one
  never did, and this change is only about how a sum is evaluated).
- **Another operator** before the next number replaces the waiting one and re-reads the sum (`2 + 3 ×` then `+` shows 5,
  the caption `2 + 3 +`); nothing is calculated for the replaced one.
- **`%` and `±`** act on the displayed number, which becomes the operand (`2 + 50 %` then `=` is 2.5). `%` is still
  "divide by 100", not a percentage of the first operand.
- **Divide by zero** is raised as soon as the operator that completes it is pressed (`1 + 8 ÷ 0 +` is an error) or on `=`,
  with the same sentence as the tool ("Can't divide by zero"); a divisor still being typed does not fail yet.
- **`C`** clears the number being typed and leaves the sum going (the display is 0 and the operator stays selected: a
  number or `=` carries on, another operator replaces the waiting one); **`AC`** (nothing typed) drops the whole sum.
- **A result** is the first number of the next sum (`2 + 3 × 4 =` then `+ 1 × 2 =` is 16).
