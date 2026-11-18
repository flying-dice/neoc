# Discover: key user journeys

The landing window: what AgentOS is, the apps you can install (from the host's registries, or by URL), sample featured
agents, and the privacy summary. Start at the host's address in a paired browser; Discover opens by itself.

Automated twins have the same ids: V1 to V5 in `e2e/discover.journeys.spec.ts`, V6 and V7 in
`client/e2e/discover.spec.ts` (they install and remove Notes on the shared host, so they run one after another).

| Id  | Who / goal                                                                        | Starts at                                       | Done when                                                                                                                                                       |
| --- | --------------------------------------------------------------------------------- | ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| V1  | A newcomer: understand what AgentOS does in under a minute.                       | Discover open (the default).                    | They can explain it in their own words; the welcome text, the Apps list (with the Calculator) and "Open Agents" were all in reach without scrolling far.         |
| V2  | Find an agent that helps with code and get it. **Does not work today.**           | Discover open.                                  | The Coder agent is on this host's roster and can be chatted with. (Today the Featured Agents are fixed sample listings: GET only flips the button to OPEN in that window and installs nothing; agents come from `agentos.toml` or `agents.create`. The spec is `test.fixme` until Discover lists real agents.) |
| V3  | Search for something that doesn't exist ("spreadsheet") and understand the result. | Discover open.                                  | They saw plain "No apps match" and "No agents match" messages that name the search, and cleared it to get everything back.                                      |
| V4  | Find out what agents are allowed to read and do.                                  | Discover open.                                  | They found the Agent Privacy section (can act, can read, never leaves the device) and used See Details to open Settings.                                         |
| V5  | Use Discover in a narrow window (Slide Over).                                     | Discover open, moved to Slide Over.             | The page still reads well: nothing is cut off and there is no sideways scrolling.                                                                                |
| V6  | Get the Notes app, try it, switch it off for a while, then remove it.             | Discover open; Notes is offered, not installed. | Notes opened in a window and in the Dock; switched off, its window and Dock icon went but it stayed in Discover; removed, it was offered again.                  |
| V7  | Install an app from a link a colleague sent.                                      | Discover open.                                  | They saw what the app is and where it comes from before anything installed, were told plainly that it runs on this host and its tools reach agents, and could say no. |

Not journeys, on purpose: the sidebar sections other than Discover (Agents, Create, Work, Play, Develop, Categories,
Updates), the Share and See All buttons, and the account row are placeholders with no behaviour yet, and the "4 agents",
"3 of 4" and Updates "2" numbers are fixed sample text, not the host's. They get a journey when they do something.
