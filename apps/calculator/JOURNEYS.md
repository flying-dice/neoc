# Calculator: key user journeys

Each journey is a goal, not a script: research participants must work out *how* themselves. Automated versions
live in `e2e/journeys.spec.ts` with the same ids (each test checks its row's "Done when"). They run against the real Rust host (`bun run host`, with the
registry from `bun run build:apps`, so the Calculator is offered in Discover; the e2e host installs it at start),
in a paired browser. The Calculator is a Luau app: it runs on the host, its window is a view the client renders, and
there is one calculator however many windows are open.

| Id | Who / goal | Starts at | Done when |
|---|---|---|---|
| J1 | Someone splitting a restaurant bill: work out 12 + 7 with the mouse. | The desktop with Discover open. The Calculator isn't open. | The display shows 19, and they found the Calculator without help. |
| J2 | A keyboard user: calculate 250 × 0.18 without touching the mouse, clear it, then do 12 + 7 and fix a slipped digit. | The Calculator open. | The display shows 45, then 0, then 19, with keyboard only (digits, operators, `=` or Enter, Backspace, Escape). Enter is `=` after keys were clicked or typed, and presses the focused key only when it was reached with Tab and nothing was typed since. |
| J3 | Chain operations: (9 × 3) − 7. | The Calculator open. | The display shows 20, and they could tell which operator was pending and what had been entered so far (the caption `9 × 3 −`). |
| J4 | Make a mistake and recover: divide by zero, then carry on with a new sum. | The Calculator open. | They saw "Error" and its reason ("Can't divide by zero") under it, understood it (a screen reader announces it), cleared it and finished a new sum. |
| J5 | An agent (Claude, over MCP) works out 18% of 2,450 and puts it on the Calculator; later it puts another result there. | The desktop with Discover open. The Calculator isn't open. | The Calculator opens by itself and shows 441 with the expression above it; the second result appears in the open window without anything being pressed, and the person can carry on from it. |
| J6 | Keep the Calculator handy while working in Files: put it in Slide Over and do 6 × 7. | Files and the Calculator open. | The Calculator is in Slide Over and shows 42, and still takes typed keys after the multitasking menu; Files, waiting in the recent-apps strip, comes back beside it and still works (its search narrows the list) while the Calculator keeps its 42. |
| J7 | Use the Calculator in a small window: shrink it as far as it goes and do 5 + 5. | The Calculator open. | The window stopped at its minimum (320x500); every key is still fully visible and at least 44 pt, and the display shows 10. |
| J8 | Someone working out a mixed sum, the way they would on a phone: 2 + 3 × 4, then 2 × 3 + 4, then a longer chain, mouse and keyboard. | The Calculator open. | The display shows 14 (not 20), then 10; after `2 + 3 ×` it still shows 3 and the caption reads `2 + 3 ×`; after `2 × 3 +` it shows 6. A result carries on into the next sum, and a divide by zero inside a chain is the same error. An agent that calls `calculate` with the same sums gets the same answers. |

## How a sum is worked out (AGOS-57)

The keypad and the `calculate` tool run one evaluator (`calc.luau`), so a sum gives the same answer from both: `×` and
`÷` bind tighter than `+` and `−`, and operators of equal precedence go left to right (`8 − 2 − 1` is 5, `8 ÷ 4 ÷ 2` is
1). The keypad shows the sum the way the iPadOS Calculator does:

- **The display** shows what the operator just pressed has completed, not the last number typed. `2 + 3 ×` shows 3
  (nothing is complete), `2 × 3 +` shows 6, `2 + 3 × 4 +` shows 14 and `2 + 3 × 4 ×` shows 12. While a number is being
  typed, the display shows it.
- **The caption** is the sum so far, in words: `2 + 3 ×` while an operator waits, the whole sum (`2 + 3 × 4`) once `=` is
  pressed. Typing the next number leaves it alone. It is one line: a long chain keeps its last terms behind an ellipsis
  (`… 3 + 4 + 5`). After an agent's `show_on_calculator` it is the agent's expression.
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
- **A result** is the first number of the next sum (`2 + 3 × 4 =` then `+ 1 × 2 =` is 16), and so is one an agent put
  there.
