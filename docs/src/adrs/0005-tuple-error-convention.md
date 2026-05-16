---
status: Accepted
date: 2026-05-03
---

# 0005. Tuple-return convention for recoverable failures

Date: 2026-05-03

## Status

Accepted

## Context

Lua's standard library mixes failure shapes freely (`nil+msg`, `false+msg`, `error()`, `nan`). Embedded runtimes need discipline — script authors shouldn't memorise per-function shapes.

Two broad patterns:
- **Always tuples** — easy call sites, verbose chaining, conflates bugs with runtime failures.
- **Always raise** — pushes recovery cost everywhere, loses direct signal.

Honest hybrid: distinguish *recoverable* failures (file missing, network down) from *programming errors* (wrong type, closed handle).

## Decision

Two shapes, chosen per function:

`(value, err)` tuple
:   On success: value holds result, err is `nil`. On failure: value is `nil`, err is string. For recoverable failures.

Raised error
:   `error(...)` propagates until `pcall`/`xpcall`. For programming errors (script bugs).

Shape is fixed per function, documented on reference page. Not interchangeable.

Every error message begins with `"module.function:"` prefix — consistent, matchable:

```text
fs.read_to_string: No such file or directory (os error 2)
thread.spawn: shared.k is 'JsonNull', which is not Sendable.
```

## Consequences

- Signature tells handling strategy. Tuple = recoverable. Single return = may raise on misuse.
- Scales: every new module follows same rule.
- Shape changes are breaking. Initial choice matters.
- Adapter layer translates upstream behaviour; documented per module.
