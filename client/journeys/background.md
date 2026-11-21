# Background work: key user journeys

An app can work while nobody is looking: a counter, a poller, a server it runs for the LAN. Its window shows what
it is doing as it happens, closing the window does not stop it, and removing the app does. The journeys use the
Ticker, a test app whose background task raises a counter every fifth of a second (it is built for the tests and
is not in Discover; the specs install it and remove it again).

Automated twins have the same ids: `e2e/background.journeys.spec.ts` (they share one Ticker, so they run one after
another).

| Id  | Who / goal                                            | Starts at                     | Done when                                                                                                                                   |
| --- | ----------------------------------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| BG1 | Open an app that works on its own and see it working. | The Ticker in the Dock.       | The count rose in the open window without any tap or key, again and again.                                                                  |
| BG2 | Pause the work from the window, then let it go on.    | The Ticker open and counting. | Paused, the count stayed where it was and the button said Resume; resumed, it rose again.                                                   |
| BG3 | Close the window, come back later.                    | The Ticker open and counting. | The window closed; opened again from the Dock it showed a count above the one at close: the app kept working without a window.              |
| BG4 | Lose the connection to the host and get it back.      | The Ticker open and counting. | The window dimmed with a "Reconnecting" banner; when the host was reachable again the banner went, the window was current and moving again. |
| BG5 | Press a control whose action fails.                   | The Ticker open and counting. | A message said what went wrong, the window stayed up and answered the next tap, and the count never stopped.                                |

## Manual verification

Ran the real shell on the real host headless (Chromium, 1280x800, light and dark) with the Ticker installed at start
(`vos-host` on port 7420, a fresh data directory, driven with `e2e/tools/session.mjs`), and checked:

- [x] Opening the Ticker from the Dock shows the current count at once, and it rises with no input (49, then 126 and
      127 a few seconds apart); the accessibility tree and the screenshot agree (light, 360x280 floating).
- [x] Fail: a toast inside the window says "ticker/app.lua:65: the Fail button always fails", the window stays up and
      keeps counting (screenshot: the toast sits over the bottom of the window, under the buttons; the counter is
      still readable).
- [x] Pause with the keyboard (click the window, Tab to Pause, Enter): the button becomes Resume and the count holds
      (310 and 310 on two looks).
- [x] Dark theme at the window's 320x240 minimum: the count, status and three buttons fit, nothing is clipped, the
      "Smallest size" cue shows (screenshot).
- [x] Stopping the host: the window shows "This window is no longer available / shutting down" and the shell shows
      "Reconnecting to vos…"; restarting the host: the shell reconnected and the Ticker window came back counting
      (the count restarts from the new host's state). This is the existing shutdown behaviour, not changed here.
- [x] `bun run test:e2e`: the five BG journeys pass (BG4 drops the `/rpc` socket and checks the dim and the
      "Reconnecting" banner, then that the window is current and moving again).

Not checked: Split View, Slide Over, 1024x768 and 1600x1000 screens, 125% text size (the Ticker is a test app; its
layout is a centred stack and was checked at its minimum and preferred sizes). The error toast overlaps the corner of
the window slightly at the preferred size; that is how every window's toast is placed and was left alone.
