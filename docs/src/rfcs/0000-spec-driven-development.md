---
status: superseded
created: 2026-05-03
superseded-by: agent-backlog-kanban skill (in-repo)
implemented-in:
---

# 0000 — Spec-driven development

> [!WARNING]
> **This RFC is superseded.** It is preserved for historical record. The two-PR spec/implementation workflow it described has been retired in favour of the **agent-backlog Kanban workflow**, defined in `.claude/skills/agent-backlog-kanban/SKILL.md` in the repository root. The skill auto-loads in any Claude Code session opened from a clone of the repo and is the single source of truth for column rules, branching, gates, and Review-stage testing reports.
>
> The CI gate that enforced this RFC (`.github/workflows/sdd-gate.yml`) has been deleted. Workflow gating is now discipline-based, governed by the skill, not CI-enforced.

## Summary

_(Original draft, retained for context.)_

Non-trivial changes to `neoc` were to land in two pull requests: a **spec PR** that ships the user-facing description of the change to the public docs site (deploying to a Cloudflare branch preview), and an **implementation PR** that brings the runtime in line with what the merged docs promise. The deployed preview of the spec PR was the spec review surface.

## Why this was superseded

The two-PR split optimised for one thing — making refinement a reviewable artifact before code lands — at the cost of friction on every medium-sized change. In practice:

- The `trivial` escape hatch caught most genuinely small work, but everything else paid two-PR overhead even when the design was self-evident from the issue.
- The Cloudflare preview gave a rendered review surface, but reviewers still ended up reading the diff because the prose was small enough to fit there.
- The CI gate (`sdd-gate.yml`) enforced the split through PR-body checkboxes, which decayed under copy/paste and never caught the cases it was supposed to.

The replacement model treats refinement and design as distinct **kanban columns** with explicit gates, and lets the design artifact (RFC, ADR, or "no design needed") be a *label* on the issue rather than a separate merged PR. Where a design document is required, it lives on `design/{issue-id}-{slug}` and merges into the issue branch — internal to the ticket — rather than against `main`.

## Replacement

The current workflow is governed by:

- **`.claude/skills/agent-backlog-kanban/SKILL.md`** — column rules (Backlog → Refinement → Design → Implementation → Review → Done), gate criteria, branching model, label requirements, and the Review-stage testing report format.
- **`.claude/skills/runtime-design/SKILL.md`** — required for any design discussion in Refinement.
- **`.claude/skills/runtime-testing/SKILL.md`** — required for the Review gate. _(Not yet authored at time of supersede; the Review gate cannot be fully satisfied until it lands.)_

RFCs and ADRs remain in `docs/src/rfcs/` and `docs/src/adrs/` as long-lived design artefacts. They are produced when an issue carries the `design-rfc` or `design-adr` label at the Refinement gate.

## What carries forward

- The taxonomy of long-lived design artefacts:

  | Path | Kind | Lifecycle |
  |---|---|---|
  | `docs/src/<ns>/<name>.md` | Module reference page | Living |
  | `docs/src/rfcs/NNNN-name.md` | Forward-looking proposal | Frozen on accept/reject |
  | `docs/src/adrs/NNNN-name.md` | Retrospective decision record | Frozen on acceptance |

- RFC numbering, status front matter (`draft`, `accepted`, `rejected`, `superseded`, `implemented`), and the rendered draft banner.
- The distinction between RFCs (proposals) and ADRs (records of decisions already in force).

## What is dropped

- The two-PR split (spec PR + implementation PR).
- The `needs-spec`, `spec-pr`, `dev-pr`, `spec-approved`, and `trivial` labels — superseded by the kanban skill's `design-rfc` / `design-adr` / `design-na` triple at the Refinement gate.
- The `sdd-gate.yml` GitHub Actions workflow.
- The PR-body checkboxes for "Spec PR / Implementation PR / Trivial".
