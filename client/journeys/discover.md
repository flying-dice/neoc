# Discover: key user journeys

The landing window: what AgentOS is, the apps you can install (from the host's registries, or by URL) and the agents on
this host. Start at the host's address in a paired browser; Discover opens by itself.

Automated twins have the same ids: V1 to V5 in `e2e/discover.journeys.spec.ts`, V6 and V7 in
`client/e2e/discover.spec.ts` (they install and remove Notes on the shared host, so they run one after another).

| Id  | Who / goal                                                                         | Starts at                                       | Done when                                                                                                                                                             |
| --- | ---------------------------------------------------------------------------------- | ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| V1  | A newcomer: understand what AgentOS does in under a minute.                        | Discover open (the default).                    | They can explain it in their own words; the welcome text, the Apps list (with the Calculator) and "Open Agents" were all in reach without scrolling far.               |
| V2  | See which agents this host has.                                                    | Discover open.                                  | They found the agents of this host (name and role), with nothing offered to install.                                                                                  |
| V3  | Search for something that doesn't exist ("spreadsheet") and understand the result. | Discover open.                                  | They saw plain "no apps match" and "no agents match" messages that name the search, and cleared it to get everything back.                                           |
| V4  | Look at only the agents, then only the apps.                                       | Discover open.                                  | The sidebar narrowed the page to the agents, then to the apps, and back to everything.                                                                                |
| V5  | Use Discover in a narrow window (Slide Over).                                      | Discover open, moved to Slide Over.             | The page still reads well: nothing is cut off and there is no sideways scrolling.                                                                                     |
| V6  | Get the Notes app, try it, switch it off for a while, then remove it.              | Discover open; Notes is offered, not installed. | Notes opened in a window and in the Dock; switched off, its window and Dock icon went but it stayed in Discover; removed, it was offered again.                       |
| V7  | Install an app from a link a colleague sent.                                       | Discover open.                                  | They saw what the app is and where it comes from before anything installed, were told plainly that it runs on this host and its tools reach agents, and could say no. |
