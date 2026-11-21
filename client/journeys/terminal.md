# Terminal: key user journeys

Agent shell output. **Placeholder page:** it shows fixed sample output (an `vos status` listing); it is not a shell
and not connected to the host's agents, so the journeys describe what the page does today. Start at the host's address
in a paired browser with Terminal open.

Automated twins (same ids): `e2e/terminal.journeys.spec.ts`.

| Id  | Who / goal                                                 | Starts at                       | Done when                                                                |
| --- | ---------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------------------ |
| T1  | See at a glance which agents are running, waiting or idle. | Terminal open                   | They can name the agent that's waiting for review.                       |
| T2  | Read the output in a narrow window.                        | Terminal open                   | The lines wrap and stay readable; there's no sideways scrolling.         |
| T3  | Read it in light mode too.                                 | Terminal open, light appearance | The terminal stays dark and legible, and its title bar text is readable. |
