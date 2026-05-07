# Testing Report Template

Use this template for every Review-stage testing report. The report is the artefact that authorises a ticket to leave Review.

---

## Ticket
- **Issue ID:**
- **Issue branch:** `issues/{issue-id}-{slug}`
- **DoD reference:** (link or quote the DoD agreed in Refinement)

## Runtime testing skill output
Result of running `.claude/skills/runtime-testing`. Paste the output, or summarise it and link to the full log.

## Exploratory testing
What did you try beyond the formal cases? What edge cases or unexpected paths did you probe?

For each scenario, record:
- What you did
- What you expected
- What actually happened

## Throwaway scripts
Any scripts written during investigation — even ones that were "just for poking around." Include them inline or link to them. They are part of the audit trail and may inform future automated coverage.

## Exploratory / chaos testing
What failure modes did you simulate? Examples:
- Network drops, latency spikes, packet loss
- Malformed or adversarial input
- Concurrent / racing calls
- Resource starvation (CPU, memory, file descriptors)
- Unexpected restarts or partial failures

For each: what broke, what held, and why.

## Findings
For anything that surfaced — even minor:

| # | Description | Sent back to Implementation? | Automated coverage added? |
|---|-------------|------------------------------|---------------------------|
| 1 |             | yes / no                     | yes / no — link to test   |

## Sign-off
Review passes only when **both** are true:

- [ ] All findings have been resolved
- [ ] All findings have automated test coverage in the repo

If either box is unchecked, the ticket stays in Review (or returns to Implementation).
