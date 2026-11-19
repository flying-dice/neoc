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
