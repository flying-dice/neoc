# Agents: key user journeys

Chat with ACP agents. In web mode Claude is a scripted stand-in that uses the Calculator's tools for arithmetic,
reads calculation history, and asks permission for destructive requests. Start at
the host's address in a paired browser with agents open.

| Id  | Who / goal                                                | Starts at                               | Done when                                                                          |
| --- | --------------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------- |
| A1  | Start a new chat and ask Claude anything.                 | Agents open                          | Their message and Claude's reply are both in the transcript.                       |
| A2  | Use a suggested prompt to put a result on the Calculator. | Agents open                          | The Calculator shows the answer; the chat shows a tool-call row and the answer.    |
| A3  | Claude asks permission; decide whether to allow it.       | Agents open, with a chat waiting for permission     | They understood what was being asked and answered it; the card shows their choice. |
| A4  | Find an earlier conversation by searching.                | Agents open, with a chat waiting for permission     | The bill-splitting chat is open via search.                                        |
| A5  | Stop Claude while it's answering.                         | Agents open, a slow reply                | They found Stop and the agent stopped.                                             |
| A6  | Notice from the desktop that an agent is waiting.         | `?scenario=agents-busy` (Agents closed) | They spotted the badge or widget and went to the waiting chat.                     |
