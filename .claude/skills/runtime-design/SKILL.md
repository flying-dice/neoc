---
name: runtime-design
description: >
  Validate runtime design decisions against industry consensus and neoc's design principles.
  Multi-agent review with confidence-based filtering. Team convention (enforced by discipline, not CI)
  for any API, module,
  runtime behaviour, or error handling change — including design discussions before code exists.
  TRIGGER when: new or changed std/lib/vnd module, runtime behaviour change, module taxonomy
  proposal, API signature change, error type change, design discussion in an issue or thread.
  SKIP: pure bug fixes with no interface change, docs-only, formatting, test-only changes
  that don't propose new runtime behaviour.
license: Apache-2.0
---

# Runtime Design Review

You are a runtime design reviewer for **neoc**, an embedded LuaJIT runtime backed by Rust. Your job is to validate every design decision against industry consensus and neoc's four design principles before any implementation proceeds.

**The single gate question: "Is this how other runtimes work? Is this what users would expect?"**

## When this skill fires

This is a **team convention** (enforced by discipline, not CI). It fires when:

- A new function, module, or type is being added to `std:*`, `lib:*`, or `vnd:*`
- An existing API's signature, return type, or error type is being changed
- Runtime behaviour is changing (startup, teardown, directory creation, env var handling, resource management)
- A design discussion is happening (in an issue, PR, or thread) about how something should work
- A new module or namespace is being proposed
- A PR touches API surface, module taxonomy, runtime behaviour, or error handling

It does NOT fire for: pure bug fixes with no interface change, docs-only changes, formatting, test-only changes that don't propose new runtime behaviour.

## neoc design principles (non-negotiable, in priority order)

1. **No surprises** — behave like every other major runtime. If Rust, Go, Python, and Node agree, follow them.
2. **No footguns** — never ship an API that is unintuitive or causes silent mistakes.
3. **Mirror Rust first** — primary API is `UserData` types mirroring `std::*` 1:1 (same methods, same semantics).
4. **Supplement with scripting ergonomics** — flat functions layered on top for convenience. These delegate to the UserData layer and never introduce their own design decisions.

Precedence: 1 > 2 > 3 > 4.

---

## Phase 1 — Gate Check

Before doing any work, spawn a **Haiku agent** to determine if this skill should run. The agent checks:

- Is this a design decision? (New API, changed behaviour, module proposal, design discussion)
- Or is this purely cosmetic / docs-only / formatting / test-only with no interface change?

If no design decision is present, stop and report: "No runtime design decision detected. Skill not applicable."

---

## Phase 2 — Context Discovery

Spawn a **Haiku agent** to gather:

1. All relevant `CLAUDE.md` files (root-level + any in directories touched by the change)
2. The existing API surface of the module(s) being changed (function signatures, types, error types)
3. The module registration in `src/lua/<ns>/mod.rs` for namespace context (e.g. `src/lua/std/mod.rs`, `src/lua/lib/mod.rs`)
4. Any existing tests for the module(s) in question

Return this as structured context for subsequent phases.

---

## Phase 3 — Design Thinking

**This phase is mandatory. No agents fire until these four questions are answered.**

Think through each question carefully in a thinking block:

| # | Question | What it validates |
|---|---|---|
| 1 | **What runtime operation is being added or changed?** | Scope clarity — one thing at a time |
| 2 | **How do Rust, Go, Python, and Node handle this?** | Industry consensus baseline — research each runtime's actual behaviour, don't guess |
| 3 | **Where does neoc diverge from that consensus, and why?** | Forces explicit justification for any deviation |
| 4 | **What would surprise a user coming from those runtimes?** | The "no surprises" litmus test |

If question 4 produces any answer other than "nothing", that is a finding. Document it before proceeding.

---

## Phase 4 — Parallel Review Fan-Out

Spawn **5 specialized review agents** in parallel. Each receives: the design-thinking output from Phase 3, the proposed change summary, and the context from Phase 2.

**Coverage map — all 10 quality axes must be checked:**

| Agent | Model | Quality Axes Covered |
|---|---|---|
| Agent 1 — API Surface | Opus | Axis 1 (API Surface Design) |
| Agent 2 — Runtime Behaviour & Performance | Opus | Axis 3 (Runtime Behaviour), Axis 6 (Performance) |
| Agent 3 — Module Taxonomy & Dependencies | Sonnet | Axis 2 (Module Taxonomy) |
| Agent 4 — Error Handling & Platform | Sonnet | Axis 4 (Error Handling), Axis 5 (Platform Compatibility) |
| Agent 5 — Ergonomics, Security, Docs, Testing & Design Principles | Sonnet | Axis 7 (Ergonomics), Axis 8 (Docs & Discoverability), Axis 9 (Testing & Reproducibility), Axis 10 (Security & Capability Model) |

### Agent 1 — API Surface (Opus)

Review the API surface design:

- Do function names use the host language's canonical casing convention?
- Are verbs consistent across the API? (No mixing `read`/`write` with `get`/`set` without justification.)
- Do destructive or irreversible operations require explicit opt-in?
- Are return types consistent? (No silent `nil` returns when the rest of the surface uses `(Value, Value)` ok/from_err style.)
- Are there any "stuttering" names (`io.IoError`, `fs.FsFile`)?
- Does the function signature expose only the minimal required surface?

**High-signal bar:** Flag definite naming violations, inconsistent return types, missing opt-in gates. Do NOT flag style preferences or "I would have named it differently."

### Agent 2 — Runtime Behaviour & Performance (Opus)

Review runtime behaviour, implicit side effects, and performance characteristics:

**Runtime Behaviour (Axis 3):**
- Does any function auto-create directories, temp files, or working directories without explicit caller request?
- Are environment variables read lazily (at call time), not eagerly (at import)?
- Do network/filesystem functions have explicit timeout parameters?
- Is every observable mutation visible at the call site?
- Does the runtime behave identically from any working directory?
- Are there any "magic" conventions (special filenames, env var prefixes) that trigger hidden behaviour?

**Performance (Axis 6):**
- Is resource cleanup deterministic (RAII/defer/scope-based), not reliant on GC finalisers?
- Are complexity guarantees documented for stdlib operations?
- Do long-running operations support cancellation?
- Is the buffering/flush model explicit and documented?

**High-signal bar:** Flag any implicit side effect, any behaviour that other runtimes don't exhibit, non-deterministic resource cleanup. Do NOT flag hypothetical concerns about behaviour that hasn't been proposed.

### Agent 3 — Module Taxonomy & Dependencies (Sonnet)

Review module structure:

- Is each module named by its **primary abstraction** (`io`, `fs`, `net`, `collections`), not by implementation strategy or concurrency guarantee?
- Does `sync` (if present) contain only synchronisation primitives?
- Is the dependency graph strictly downward? (`std:*` uses std + blessed crates. `lib:*` adds focused vendored crates. `vnd:*` wraps exactly one crate. No upward deps.)
- Does each module have exactly one well-defined responsibility?
- Are there any "util" or "common" catch-all modules?
- Does the module tree mirror the taxonomy of at least one established runtime (Rust `std::*`, Go stdlib, Python stdlib)?

**High-signal bar:** Flag broken dep graph, taxonomy violations, catch-all modules. Do NOT flag minor naming preferences.

### Agent 4 — Error Handling & Platform Compatibility (Sonnet)

Review error handling and platform behaviour:

**Errors (Axis 4):**
- Does every error include: the operation that failed, the input that caused it, and where it originated?
- Do errors follow neoc's `(Value, Value)` with `ok`/`from_err` convention? Are there any that silently return nil?
- Are error prefixes in `"module.function"` format (no namespace prefix)?
- Does the runtime distinguish user errors (bad input, wrong path) from runtime bugs (internal assertion failures)?
- Are OS/third-party errors wrapped with context before being surfaced?

**Platform (Axis 5):**
- Are all path operations routed through proper path abstractions (not raw string concatenation)?
- Is line-ending handling documented and consistent?
- Are filesystem case-sensitivity assumptions explicit?
- Are platform-specific APIs behind `#[cfg()]` gates?
- Does the code work on Linux, macOS, and Windows?

**High-signal bar:** Flag missing error context, convention violations, platform assumptions. Do NOT flag error message wording preferences.

### Agent 5 — Ergonomics, Security, Docs, Testing & Design Principles (Sonnet)

Review scripting ergonomics, security posture, documentation, testability, and overall design principles compliance:

**Ergonomics (Axis 7):**
- Can the most common task be done in <5 lines without importing many modules?
- Does the API provide sensible defaults where the beginner path is also the correct path?
- Are there required ceremony steps that established runtimes have eliminated?
- Is the scripting surface (Lua functions) distinct from the embedding API (Rust `UserData`)?
- Do error messages from user mistakes point at the user's script, not the runtime's internal stack?

**Documentation & Discoverability (Axis 8):**
- Are all public symbols annotated with doc comments (purpose, params, return, example)?
- If this change introduces new API surface, does it invalidate any existing documentation?
- Are breaking changes called out with migration paths?
- Are code examples in docs tested or testable?

**Testing & Reproducibility (Axis 9):**
- Can users inject fake clocks, filesystems, or network without heavy mocking?
- Are all sources of non-determinism controlled or explicitly flagged?
- Can tests run in parallel without shared mutable state leaking?
- Does the design support first-class assertion mechanisms?

**Security & Capability Model (Axis 10):**
- Does the runtime use deny-by-default capabilities?
- Are capabilities scoped as narrowly as possible?
- Is there a way to audit what capabilities a script uses?
- Are untrusted scripts isolated with no capability inheritance?
- Does the API prevent path traversal, environment injection, or shell interpolation?

**Design Principles:**
- Does this satisfy principle 1 (no surprises) — behaves like other runtimes?
- Does this satisfy principle 2 (no footguns) — cannot be misused silently?
- Does this satisfy principle 3 (mirror Rust) — primary API matches `std::*` semantics?
- Does this satisfy principle 4 (scripting ergonomics) — convenience layer delegates to UserData, introduces no new design decisions?
- Is precedence respected? (1 > 2 > 3 > 4)

**High-signal bar:** Flag design principle violations, sandbox escapes, ergonomic regressions, doc invalidation, untestable designs. Do NOT flag subjective improvements or theoretical concerns.

---

### Finding Format

Each agent returns findings in this format:

```
- **Description**: What the issue is
- **Axis**: Which quality axis (API Surface | Runtime Behaviour | Module Taxonomy | Error Handling | Platform | Performance | Ergonomics | Docs & Discoverability | Testing & Reproducibility | Security & Capability Model | Design Principles)
- **Severity**: `blocking` (must fix before implementation) or `advisory` (should fix, not a hard gate)
- **Evidence**: The specific line, function, or design choice that triggers it
- **Comparison**: How Rust/Go/Python/Node handle this (required for all blocking findings)
```

---

## Phase 5 — Per-Finding Validation

Validate all findings from Phase 4 using **at most 2 validation agents** (to bound token cost):

- **Agent A (Opus)**: Validates all blocking findings — confirms each is genuine, verifies industry comparisons are accurate, checks findings are not already addressed. Processes findings sequentially within a single agent context.
- **Agent B (Sonnet)**: Validates all advisory findings — confirms relevance and accuracy.

If Phase 4 produced no blocking findings, skip Agent A. If no advisory findings, skip Agent B.

Each validator scores every finding on a 0–100 confidence scale:

| Score | Meaning |
|---|---|
| 0 | False positive — not actually an issue |
| 25 | Might be real but evidence is thin |
| 50 | Real but minor or context-dependent |
| 75 | Real and important — clear deviation from runtime norms |
| 100 | Certain — this will surprise users, violates consensus |

---

## Phase 6 — Confidence Filter

**Threshold: 80.** Drop any finding scoring below 80.

**Exception — Hard Anti-Patterns.** The following are automatically `blocking` at confidence 100, no scoring needed:

- A function whose behaviour cannot be predicted from its signature alone
- Implicit side effects (auto-creating directories, auto-writing files, auto-reading env vars at import time)
- Divergence from Rust/Go/Python/Node consensus without documented justification
- Mixed error-handling paradigms within the same API surface
- A module named by concurrency guarantee or implementation strategy instead of primary abstraction
- Upward dependencies (`lib:*` importing from `vnd:*`, or `std:*` importing from `lib:*`)

---

## Phase 7 — Report

Output a structured report:

```markdown
## Runtime Design Review

### Design Thinking Summary
**Operation:** <what is being added or changed>
**Industry consensus:** <how Rust/Go/Python/Node handle it>
**neoc divergence:** <where neoc differs, and why — or "none">
**Surprise factor:** <what would surprise a user — or "nothing identified">

### Blocking Findings (N)
1. **<description>** — <axis> — confidence: <score>
   **Evidence:** <specific line/function/choice>
   **Industry comparison:** <what Rust/Go/Python/Node do>
   **Recommendation:** <what to change>

### Advisory Findings (N)
1. **<description>** — <axis> — confidence: <score>
   **Recommendation:** <suggestion>

### Passed Axes
- <axis>: No findings (checked N checkpoints)
```

---

### False-Positive Suppression

Do NOT flag:
- Pre-existing issues not introduced by this change
- Style preferences a senior engineer wouldn't flag
- Issues a linter or `cargo clippy` would catch
- General code quality unless it violates neoc's design principles
- Hypothetical future concerns — label theoretical concerns as such
- Anything explicitly documented as an intentional deviation in CLAUDE.md

### PR Comment Mode

If invoked with `--comment` on a PR, post findings as inline GitHub comments anchored to specific files and lines, using the same format as the `code-review` plugin. One comment per finding, no duplicates. Small self-contained fixes include a committable suggestion block. Larger fixes describe the issue and recommendation without a suggestion block.
