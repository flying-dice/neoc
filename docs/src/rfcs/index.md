# RFCs

Design proposals for cross-cutting changes to `neoc`. Module-shaped changes are specced on the module reference pages directly under `docs/src/<ns>/<name>.md`; RFCs are for everything else — process, build, new namespaces, multi-module design, anything that does not fit on a single module page.

The development workflow is the agent-backlog Kanban, defined in [`.claude/skills/agent-backlog-kanban/SKILL.md`](https://github.com/flying-dice/neoc/blob/main/.claude/skills/agent-backlog-kanban/SKILL.md). RFCs are produced when an issue carries the `design-rfc` label at the Refinement gate.

## Status

Every RFC carries a `status` in its front matter. A draft banner is rendered at the top of any RFC that is not yet accepted, so readers of this site cannot mistake a proposal for the contract.

| Status | Meaning |
|---|---|
| `draft` | Under refinement. Not the contract. May be edited freely. |
| `accepted` | Merged. Awaiting implementation. |
| `implemented` | Merged and the runtime is in line. |
| `rejected` | Closed without acceptance. Kept for design history. |
| `superseded` | Replaced by a later RFC or by an in-repo skill. |

## Index

### Draft

- [0002 — Built-in feature request mechanism for LLM callers](./0002-feature-request-mechanism)
- [0001 — Sendable concurrency model](./0001-sendable-concurrency-model)
- [0002 — std:env — Environment, platform constants, and paths](./0002-std-env)
- [0002 — std:task — cooperative task scheduler](./0002-std-task)
- [0002 — `lib:mime` — extension-to-MIME mapping via `mime_guess`, extension preference layer](./0002-lib-mime-extension-and-filename-detection)

### Accepted

_None yet._

### Implemented

- [0002 — `lib:compile` — programmatic single-file executable bundler](./0002-lib-compile)

### Rejected

_None yet._

### Superseded

- [0000 — Spec-driven development](./0000-spec-driven-development) — replaced by the agent-backlog Kanban skill
