---
status: superseded
created: 2026-05-03
superseded-by: agent-backlog-kanban skill (in-repo)
implemented-in:
---

# 0000 — Spec-driven development

> [!WARNING]
> **Superseded.** Preserved for history. Two-PR workflow retired in favour of **agent-backlog Kanban** (`.claude/skills/agent-backlog-kanban/SKILL.md`). CI gate (`sdd-gate.yml`) deleted. Workflow gating now discipline-based.

## Summary

Non-trivial changes land in two PRs: **spec PR** (user-facing docs to Cloudflare preview) then **implementation PR** (runtime matches docs). Preview = review surface.

## Why superseded

Two-PR split optimised for refinement-as-artifact at cost of friction on every medium change:
- `trivial` escape caught small work; everything else paid overhead even with self-evident design.
- Reviewers read diffs anyway — Cloudflare preview unused in practice.
- CI gate (`sdd-gate.yml`) via checkboxes decayed under copy/paste.

Replacement: refinement and design as **kanban columns** with explicit gates. Design artifact (RFC/ADR/"no design needed") is a label, not a separate PR. Design docs live on `design/{id}-{slug}` branch, merge into issue branch.

## What carries forward

| Path | Kind | Lifecycle |
|---|---|---|
| `docs/src/<ns>/<name>.md` | Module reference | Living |
| `docs/src/rfcs/NNNN-name.md` | Forward-looking proposal | Frozen on accept/reject |
| `docs/src/adrs/NNNN-name.md` | Retrospective decision | Frozen on acceptance |

RFC numbering, status frontmatter, draft banner, RFC/ADR distinction.

## What is dropped

- Two-PR split
- `needs-spec`, `spec-pr`, `dev-pr`, `spec-approved`, `trivial` labels → replaced by `design-rfc`/`design-adr`/`design-na`
- `sdd-gate.yml` workflow
- PR-body checkboxes
