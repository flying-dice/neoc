# Agents: key user journeys

Chat with ACP agents. In web mode Claude is a scripted stand-in that uses the Calculator's tools for arithmetic,
reads calculation history, and asks permission for destructive requests. Start at
`http://localhost:6200/web.html?open=agents`.

| Id  | Who / goal                                                | Starts at                               | Done when                                                                          |
| --- | --------------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------- |
| A1  | Start a new chat and ask Claude anything.                 | `?open=agents`                          | Their message and Claude's reply are both in the transcript.                       |
| A2  | Use a suggested prompt to put a result on the Calculator. | `?open=agents`                          | The Calculator shows the answer; the chat shows a tool-call row and the answer.    |
| A3  | Claude asks permission; decide whether to allow it.       | `?open=agents&scenario=agents-busy`     | They understood what was being asked and answered it; the card shows their choice. |
| A4  | Find an earlier conversation by searching.                | `?open=agents&scenario=agents-busy`     | The bill-splitting chat is open via search.                                        |
| A5  | Stop Claude while it's answering.                         | `?open=agents&delay=400`                | They found Stop and the agent stopped.                                             |
| A6  | Notice from the desktop that an agent is waiting.         | `?scenario=agents-busy` (Agents closed) | They spotted the badge or widget and went to the waiting chat.                     |
