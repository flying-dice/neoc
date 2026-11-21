# Pairing: key user journeys

How a browser becomes a device the host trusts. Automated twins (same ids): `e2e/pairing.journeys.spec.ts`. The host
prints a code at start (`agentos-host pair` prints another); there is no trusted-network shortcut, not even from the same
machine. Rate limiting (5 wrong codes a minute) is covered by the host's pairing tests and the "Too many attempts" story,
not here: exhausting it would lock every other journey out.

| Id | Who / goal | Starts at | Done when |
|---|---|---|---|
| P1 | Someone opens AgentOS in a browser for the first time. | The host's address, unpaired. | They saw a pairing screen that says what to do, not an empty or broken desktop. |
| P2 | Mistype the code, then understand what went wrong. | The pairing screen. | They saw one clear message (wrong, used or older than 10 minutes), knew how to get a new code, and could try again. |
| P3 | Pair an iPad with the code from the host. | The pairing screen, a fresh code. | The desktop appeared; reopening the page went straight to it. |
| P4 | Try a code a second time (or on a second device). | A code that was already used. | They were told it was already used, and nothing was paired. |
