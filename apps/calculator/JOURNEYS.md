# Calculator: key user journeys

Each journey is a goal, not a script: research participants must work out *how* themselves. Automated versions
live in `e2e/journeys.spec.ts` with the same ids (each test checks its row's "Done when"). They run against the real Rust host (`bun run host`, with the
registry from `bun run build:apps`, so the Calculator is offered in Discover; the e2e host installs it at start),
in a paired browser. The Calculator is a Lua app: it runs on the host, its window is a view the client renders, and
there is one calculator however many windows are open.

| Id | Who / goal | Starts at | Done when |
|---|---|---|---|
| J1 | Someone splitting a restaurant bill: work out 12 + 7 with the mouse. | The desktop with Discover open. The Calculator isn't open. | The display shows 19, and they found the Calculator without help. |
| J2 | A keyboard user: calculate 250 × 0.18 without touching the mouse, then clear it. | The Calculator open. | The display shows 45, then 0, with keyboard only. |
| J3 | Chain operations: (9 × 3) − 7. | The Calculator open. | The display shows 20, and they could tell which operator was pending. |
| J4 | Make a mistake and recover: divide by zero, then carry on with a new sum. | The Calculator open. | They saw "Error", understood it, cleared it and finished a new sum. |
| J5 | An agent (Claude, over MCP) works out 18% of 2,450 and puts it on the Calculator; later it puts another result there. | The desktop with Discover open. The Calculator isn't open. | The Calculator opens by itself and shows 441 with the expression above it; the second result appears in the open window without anything being pressed, and the person can carry on from it. |
| J6 | Keep the Calculator handy while working in Files: put it in Slide Over and do 6 × 7. | Files and the Calculator open. | The Calculator is in Slide Over and shows 42; Files, waiting in the recent-apps strip, comes back beside it and still works (its search narrows the list) while the Calculator keeps its 42. |
| J7 | Use the Calculator in a small window: shrink it as far as it goes and do 5 + 5. | The Calculator open. | The window stopped at its minimum (320x500); every key is still fully visible and at least 44 pt, and the display shows 10. |
