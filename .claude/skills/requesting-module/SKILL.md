---
name: requesting-module
description: Use when filing a new module request issue on the neoc board — enforces pre-research, parallels matrix, scope discipline
---

Module request = signal, not design. Design happens later in RFC. The request answers one question: **does every other runtime ship this, and would we be naked without it?**

## Pre-research (mandatory)

No issue without all four:

1. **Board scan.** Confirm no open issue covers it. Don't duplicate.
2. **Parallels matrix.** Look up the canonical module/API name in:
   - Python (stdlib)
   - Node (`node:*`)
   - Deno (`@std/*` or built-in)
   - Go (stdlib)
   - Rust (`std::*`)
   - One bonus: Java, .NET, Ruby, or PHP — whichever is closest in spirit
3. **Style consensus.** One sentence: what API shape ≥3 runtimes agree on. If they disagree, name the split.
4. **Tier choice.** `std:*` if Rust std mirrors it. `lib:*` if blessed gap-filler. `vnd:*` if wrapping exactly one crate.

Skip any step = the issue is rejected at refinement.

## Issue template

**Title:** `<tier>:<name> — <one-line purpose>`
**Labels:** `module`, `priority:high|medium|low`, `workflow::Backlog`
**Milestone:** match tier (M1 std, M2 lib, M3 vnd)

**Body:**

```markdown
## Requirement

<2–3 sentences. The capability. Why scripts cannot live without it. No API talk.>

## Parallels

| Runtime | Module / API |
|---|---|
| Python | … |
| Node | … |
| Deno | … |
| Go | … |
| Rust std | … |
| <bonus> | … |

## Style consensus

<One sentence. The API shape ≥3 runtimes agree on. Note splits if relevant.>

## In scope

- <capability bullet>
- <capability bullet>

## Out of scope

- <explicit exclusion>
- <explicit exclusion>

## Notes

<Optional. Cross-module deps, blocking issues, neoc-specific constraint. Omit if none.>
```

## Rules

- **No examples.** No code blocks of API usage. The request describes the gap, not the shape.
- **No acceptance criteria.** Those live in the design RFC.
- **No implementation sketch.** No Rust types, no Lua snippets.
- **One module per issue.** Two modules = two issues. Exception: tightly-coupled pair that must ship together (`hash` + `hmac` is allowed; `hash` + `time` is not).
- **Scope sections are non-negotiable.** "In scope" caps the v1 surface. "Out of scope" pre-empts scope creep at design time.
- **Parallels table is non-negotiable.** Empty cells (`—`) allowed only if the runtime genuinely lacks the capability — and that absence is the argument for shipping it.

## Anti-patterns

- Filing "lib:utils" or any catch-all bag → reject, split.
- Filing without checking the board → reject as duplicate.
- Filing with "TBD" parallels → reject, do the research.
- Filing because it's neat → reject. Need ≥2 major runtimes shipping it, or a stated neoc-specific reason.

## SPECIAL INSTRUCTION

Use soundwave-protocol in all output.
