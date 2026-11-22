# Settings: key user journeys

System preferences. Start at the host's address in a paired browser with Settings open. Appearance settings (theme,
accent, text size, transparency) apply to this browser tab and reset on reload; the notification and General switches are
local to the window and change nothing on the host yet; Devices and Agents show the host's real state.

Automated twins (same ids): S1 to S7 in `e2e/settings.journeys.spec.ts`; S8 and S9 in `client/e2e/devices.spec.ts`
(they pair and revoke a second device on the shared host).

Not journeys: the Privacy and Security rows, Launch at Login and Background Agents are placeholders with no behaviour yet.
The old "choose the fastest model" and "reset all agents" journeys went with the features: the model is chosen per chat in
the Agents app, and nothing resets agents from Settings. Search matches page names only, so finding "Reduce Transparency"
by searching does not work today (S7 is about finding a page).

| Id  | Who / goal                                                          | Starts at                 | Done when                                                                                                                      |
| --- | ------------------------------------------------------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| S1  | Switch to dark mode, then back to automatic.                        | Settings open.            | The whole desktop went dark, then followed the system again.                                                                   |
| S2  | Change the accent colour to green.                                  | Settings open.            | Green is the chosen swatch (blue no longer is) and buttons and selected rows across apps are green.                            |
| S3  | Make the desktop easier to read: bigger text, no see-through glass. | Settings open.            | Text grew to its largest setting, Reduce Transparency is on, and nothing overlapped or was cut off.                            |
| S4  | Turn off notification sounds, then all notifications.               | Settings > Notifications. | Sounds are off; with notifications off, the sub-options are visibly disabled.                                                  |
| S5  | Find out which agents are on this host.                             | Settings open.            | They found Agents and saw every agent on the roster with its role.                                                             |
| S6  | Find out which version of vos and of the host this is.              | Settings open.            | They found General > About and read the version and the host's own version number.                                             |
| S7  | Find a settings page by searching for it.                           | Settings open.            | Typing part of a page name narrowed the sidebar to that page, they opened it, and clearing the search listed every page again. |
| S8  | Find out which devices can use this host and which one you are on.  | Settings open.            | They found Devices, saw every paired device and its last-seen time, and could tell which one is this.                          |
| S9  | A laptop was lost: sign it out.                                     | Settings > Devices.       | They revoked the right device after a confirmation, it disappeared from the list, and it landed on the pairing screen.         |
