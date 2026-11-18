# Desktop shell: key user journeys

The iPadOS-style desktop itself: Dock, windows, Stage Manager strip, Split View, Slide Over.
Start at `http://localhost:6200/web.html`.

| Id  | Who / goal                                                                   | Starts at                                           | Done when                                                                                           |
| --- | ---------------------------------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| D1  | First launch: find out what AgentOS is and open one app from the Dock.       | The desktop with Discover open.                     | They can say in a sentence what AgentOS is, and another app's window is open.                       |
| D2  | Work with two apps side by side: Files on the left, Calculator on the right. | `?open=files`                                       | Both are in Split View, one per side.                                                               |
| D3  | Make a window bigger, then smaller, then move it.                            | `?open=settings`                                    | They resized it both ways and moved it, without anything getting cut off or hidden behind the Dock. |
| D4  | Switch back to an app they used a moment ago, without the Dock.              | `?open=files,terminal` (Files ends up in the strip) | They used the recent-apps strip to bring Files back.                                                |
| D5  | Tidy up: minimise one window and close another.                              | `?open=files,calculator`                            | One window is in the strip, the other is gone.                                                      |
| D6  | Make one app take over the whole screen, then go back to a normal window.    | `?open=discover`                                    | They used full screen and returned to a floating window.                                            |
