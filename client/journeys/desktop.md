# Desktop shell: key user journeys

The iPadOS-style desktop itself, served by the Rust host: Dock, windows, the Stage Manager strip, Split View, Slide Over
and full screen. Every journey starts in a paired browser at the host's address (`docs/TESTING.md`: pairing is
a precondition, see `pairing.md` for the unpaired side). The first window the desktop opens is Discover.

Each journey is a goal, not a script: research participants find the way themselves. The automated twin of each is
the test with the same id in `e2e/desktop.journeys.spec.ts`, driven by test ids against the real host.

| Id  | Who / goal                                                                   | Starts at                                              | Done when                                                                                                                                                 |
| --- | ---------------------------------------------------------------------------- | ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | First launch: find out what AgentOS is and open one app from the Dock.       | The desktop with Discover open (the default).          | They can say in a sentence what AgentOS is, another app's window is open, and Discover has stepped back into the recent-apps strip.                      |
| D2  | Work with two apps side by side: Files on the left, Calculator on the right. | Files open.                                            | Both are in Split View, Files left and the Calculator right, with a divider between them; the empty half asked for an app and the Dock answered.        |
| D3  | Make a window bigger, then smaller, then move it.                            | Settings open.                                         | They resized it both ways and moved it; it never reached past the right edge or down behind the Dock.                                                    |
| D4  | Switch back to an app they used a moment ago, without the Dock.              | Files and Terminal opened in that order (Files is in the strip). | They used the recent-apps strip to bring Files back; Terminal took its place in the strip.                                                  |
| D5  | Tidy up: minimise one window and close another.                              | Files and Terminal open (one on stage, one in the strip). | The minimised app waits in the strip, the closed one is gone from the stage and the strip; the Dock still opens it again.                              |
| D6  | Make one app take over the whole screen, then go back to a normal window.    | Discover open.                                         | They used full screen (the window grew to fill the stage) and returned to a floating window of its original size.                                        |

Notes for the people running these:

- The strip hides itself while a window covers it: rest the pointer on the left edge to bring it back.
- The multitasking menu is the "···" at the top of each window (Full Screen, Split View left or right, Slide Over, Floating,
  Minimise, Close). Dragging the title bar moves a floating window.
- Nothing here depends on which Lua apps are installed beyond the Calculator and the Agents app the e2e host installs;
  the desktop hides an app's Dock icon when it is not installed.
