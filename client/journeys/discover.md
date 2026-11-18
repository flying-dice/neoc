# Discover: key user journeys

The landing window: what AgentOS is, the apps you can install, featured agents, privacy. Start at the host's address in a paired browser. V6 and V7 are automated in `client/e2e/discover.spec.ts` against the real host.

| Id  | Who / goal                                                                        | Starts at                    | Done when                                                                        |
| --- | --------------------------------------------------------------------------------- | ---------------------------- | -------------------------------------------------------------------------------- |
| V1  | A newcomer: understand what AgentOS does in under a minute.                       | Discover open (the default). | They can explain it in their own words.                                          |
| V2  | Find an agent that helps with code and get it.                                    | Discover open.               | The Coder agent shows as installed (OPEN).                                       |
| V3  | Search for an agent that doesn't exist ("spreadsheet") and understand the result. | Discover open.               | They saw a clear "nothing matches" message and could get back to everything.     |
| V4  | Find out what agents are allowed to read and do.                                  | Discover open.               | They found the privacy section and opened the related settings.                  |
| V5  | Use Discover in a narrow window (Slide Over).                                     | Discover open.               | The page still reads well: nothing is cut off and there's no sideways scrolling. |
| V6  | Get the Notes app, try it, switch it off for a while, then remove it. | Discover open; Notes is offered, not installed. | Notes opened in a window and in the Dock; switched off, its window and Dock icon went but it stayed in Discover; removed, it was offered again. |
| V7  | Install an app from a link a colleague sent. | Discover open. | They saw what the app is and where it comes from before anything installed, were told plainly that it runs on this host and its tools reach agents, and could say no. |
