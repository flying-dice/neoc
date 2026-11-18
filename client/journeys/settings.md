# Settings: key user journeys

System preferences. Start at the host's address in a paired browser with Settings open. S8 and S9 are automated in `client/e2e/devices.spec.ts` against the real host.

| Id  | Who / goal                                               | Starts at        | Done when                                                                     |
| --- | -------------------------------------------------------- | ---------------- | ----------------------------------------------------------------------------- |
| S1  | Switch to dark mode, then back to automatic.             | Settings open | The whole desktop went dark, then followed the system again.                  |
| S2  | Change the accent colour to green.                       | Settings open | Buttons and selected rows across apps are green.                              |
| S3  | Make text bigger.                                        | Settings open | Text grew and nothing overlapped or was cut off.                              |
| S4  | Turn off notification sounds, then all notifications.    | Settings open | Sounds are off; with notifications off, the sub-options are visibly disabled. |
| S5  | Choose the fastest model for agents.                     | Settings open | Claude Haiku 5.5 has the checkmark.                                           |
| S6  | Start resetting all agents, then change your mind.       | Settings open | The confirmation appeared, they cancelled, and nothing was reset.             |
| S7  | Find the setting for reducing transparency by searching. | Settings open | They found it through search and turned it on.                                |
| S8  | Find out which devices can use this host and which one you are on. | Settings open. | They found Devices, saw every paired device and its last-seen time, and could tell which one is this. |
| S9  | A laptop was lost: sign it out. | Settings > Devices. | They revoked the right device after a confirmation, it disappeared from the list, and it landed on the pairing screen. |
