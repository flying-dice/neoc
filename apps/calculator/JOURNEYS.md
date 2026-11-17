# Calculator: key user journeys

Each journey is a goal, not a script: research participants must work out *how* themselves. Automated versions
live in `e2e/journeys.spec.ts` with the same ids. Start every journey from web mode unless it says otherwise:
`http://localhost:6200/web.html` (the desktop on the in-memory host).

| Id | Who / goal | Starts at | Done when |
|---|---|---|---|
| J1 | Someone splitting a restaurant bill: work out 12 + 7 with the mouse. | The desktop with Discover open. The Calculator isn't open. | The display shows 19, and they found the Calculator without help. |
| J2 | A keyboard user: calculate 250 × 0.18 without touching the mouse, then clear it. | `?open=calculator` | The display shows 45, then 0, with keyboard only. |
| J3 | Chain operations: (9 × 3) − 7. | `?open=calculator` | The display shows 20, and they could tell which operator was pending. |
| J4 | Make a mistake and recover: divide by zero, then carry on with a new sum. | `?open=calculator` | They saw "Error", understood it, cleared it and finished a new sum. |
| J5 | Ask Claude to work out 18% of 2,450 and put it on the Calculator. | `?open=agents` | The Calculator opens by itself and shows 441, with the expression above it. |
| J6 | Keep the Calculator handy while working in Files: put it in Slide Over and do 6 × 7. | `?open=files,calculator` | The Calculator is in Slide Over and shows 42; Files is still usable. |
| J7 | Use the Calculator in a small window: shrink it as far as it goes and do 5 + 5. | `?open=calculator` | Every key is still fully visible and tappable at the smallest size, and the display shows 10. |
