---
name: agent-backlog-kanban
description: Enforce and guide the agent-backlog Kanban workflow as tickets move through Backlog → Refinement → Design → Implementation → Review with strict quality gates. Use this skill whenever the user mentions tickets, issues, the agent backlog, kanban columns, refinement, design RFCs/ADRs, issue branches, design branches, definition of done, design labels (design-rfc, design-adr, design-na), or testing reports — even if the word "kanban" is not used. Also use it whenever the user wants to transition a ticket between states, name a branch for an issue, decide whether a design artefact is needed, or produce a Review-stage testing report. This skill defines the per-column rules, the branching model (issues/{id}-{slug} and design/{id}-{slug}), the mandatory in-repo skills (runtime-design, runtime-testing), and the testing report contents. Treat it as authoritative for the team's workflow — do not invent alternative paths.
---

# Agent Backlog Kanban Workflow

This skill governs how work moves through the agent backlog. The flow is intentionally strict: a ticket cannot leave a column unless its exit criteria are met. The point of the strictness is that this is an *agent* backlog — the cost of letting under-specified or under-tested work through is paid back with interest by the agent later.

Use this skill to guide users through transitions, enforce the gates, generate the right branch names, point to the right in-repo skills, and produce the expected artefacts.

## The columns

```
Backlog → Refinement → Design → Implementation → Review
```

Each column has its own rules. Walk them in order below. When a user asks about a transition, locate the *current* column, check its exit criteria, and only then talk about what comes next.

---

## 1. Backlog

Backlog is an **active, curated** roadmap — not a frozen one. Tickets here record intent, not commitment, but the backlog itself is something the team (and agents) keep tidy.

**Encouraged in this column:**
- Creating new tickets
- Deleting tickets that no longer make sense
- Consolidating duplicates and merging overlapping ticket bodies
- Editing titles and descriptions for clarity
- Re-ordering / re-prioritising

**Rule: do not *investigate* Backlog tickets.** No spikes, no POCs, no design work, no estimation, no implementation research. The distinction matters: shaping the *backlog itself* (what tickets exist, how they're worded, which are duplicates) is fine and encouraged. Doing investigation work *against* a ticket — exploring how to solve it — is not, because that work belongs in Refinement once an owner has pulled it in. Investigation against unconfirmed work burns time on things that may never ship and creates the illusion of progress.

If a user asks for analysis, estimation, or a solution sketch against a ticket that is currently in Backlog, push back: it must be pulled into Refinement by an owner first.

**Exit:** A ticket leaves Backlog only when an owner pulls it into Refinement.

---

## 2. Refinement

Refinement is where shaping happens. The ticket itself is the source of truth — comments, links, decisions, and outcomes of investigation belong on the ticket, not in side channels.

**Encouraged in this column:**
- Proof-of-Concepts
- Spikes
- Discovery sessions
- Comments and discussion on the ticket

**Mandatory tooling — design discussion uses the in-repo skill.**
Refinement must be performed from within the repo so that the in-repo skills are loaded. Any discussion of design aspects requires:

```
.claude/skills/runtime-design
```

If a user is discussing design without that skill loaded — for example, working from outside the repo — stop and tell them: design discussion needs the runtime-design skill. Do not improvise a design conversation without it.

**Exit criteria — both must be true:**

1. The ticket has a clear, well-defined Definition of Done.
2. Exactly one of these labels is applied:
   - `design-rfc` — a Request For Comments document is required
   - `design-adr` — an Architecture Decision Record is required
   - `design-na` — no design artefact is required

`design-na` is a *deliberate* label, not the absence of one. It records that the team considered design and decided none was needed — that decision is auditable. The absence of any label is not equivalent and does not satisfy the gate.

When helping a user transition a ticket out of Refinement, verify both criteria explicitly. If either is missing, name what is missing and stop. Do not wave the ticket through.

---

## 3. Design

Design is the first column where commits land. The work here is to set up the issue branch and, if required, get the design document onto it.

**Branching.**

Create the issue branch:
```
issues/{issue-id}-{slug}
```

If the ticket carries `design-rfc` or `design-adr`, also create the design branch:
```
design/{issue-id}-{slug}
```

**Slug rules.** Lowercase. Hyphenated. No special characters or spaces. Derive from the ticket title.

Example: ticket `NEOC-142` titled "Add retry policy to agent runner" →
- `issues/NEOC-142-add-retry-policy-to-agent-runner`
- `design/NEOC-142-add-retry-policy-to-agent-runner` (only if rfc/adr)

**Mandatory merge direction.**
```
design/{issue-id}-{slug}  →  issues/{issue-id}-{slug}
```

The design branch never merges anywhere else first. The issue branch is the integration point for everything related to the ticket. Do not propose any other merge target — not main, not develop, not a release branch.

**Exit:**
- For `design-rfc` / `design-adr`: the design document is merged into the issue branch.
- For `design-na`: the issue branch exists and is ready for code.

---

## 4. Implementation

Implementation happens on `issues/{issue-id}-{slug}`. The design document, if there is one, is already on this branch from the previous column.

**Exit:** Implementation is functionally complete *against the DoD agreed in Refinement*. Reference that DoD when checking — do not invent new criteria mid-flight, and do not let scope drift quietly. If new criteria genuinely emerge, they belong back in the ticket as an explicit DoD update, not as silent additions.

---

## 5. Review

Review verifies the implementation rigorously. This is the column where the strictness pays for itself.

**Mandatory tooling — testing uses the in-repo skill.**
Run the in-repo testing skill at:

```
.claude/skills/runtime-testing
```

**Required output: a testing report.** See `references/testing-report-template.md` for the exact structure. The report must include:

- Output of the runtime-testing skill execution
- Exploratory testing notes
- Any throwaway scripts used during investigation
- Exploratory / chaos testing exercised

**Failure handling — non-negotiable.**

If anything is uncovered during Review:

1. The ticket goes back to Implementation.
2. Coverage for the uncovered case is added to the repo's automated tests.

Both halves matter. Finding the bug is half the work; the other half is making sure the same class of issue cannot recur silently. A ticket only leaves Review when the testing report is clean *and* any new automated coverage is in place.

This is the part of the workflow most likely to decay under deadline pressure. When helping a user close out a ticket, ask explicitly about both halves before agreeing it's done.

---

## How to help with transitions

When a user asks to move a ticket forward, locate the *current* column and walk through its gate:

| Transition | What to verify |
|-----------|----------------|
| Backlog → Refinement | Owner is assigned; no investigation has happened yet |
| Refinement → Design | DoD defined; one of `design-rfc`/`design-adr`/`design-na` applied |
| Design → Implementation | Issue branch exists; design doc merged in (if required) |
| Implementation → Review | Functionality complete against the DoD |
| Review → Done | Testing report clean; regressions backfilled into automated tests |

If a gate is not met, name the missing item explicitly. Do not suggest workarounds, do not propose to "come back to it later," and do not let the ticket move.

---

## Quick reference

**Branch templates**
- `issues/{issue-id}-{slug}` — integration branch for the ticket
- `design/{issue-id}-{slug}` — design document branch; merges into the issue branch

**Refinement exit labels (exactly one)**
- `design-rfc`
- `design-adr`
- `design-na`

**In-repo skills referenced by this workflow**
- `.claude/skills/runtime-design` — required for design discussion in Refinement
- `.claude/skills/runtime-testing` — required for testing in Review

**Bundled references in this skill**
- `references/testing-report-template.md` — read this when producing a Review-stage testing report
