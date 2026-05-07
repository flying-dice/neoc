---
status: Accepted
date: 2026-05-03
---

# 0005. Tuple-return convention for recoverable failures

Date: 2026-05-03

## Status

Accepted

## Context

Lua and Luau permit functions to fail in several ways. The standard library mixes these freely:

- Some functions return `nil` plus an error message: `io.open` returns `nil, "<filename>: No such file or directory"` on failure.
- Some return `false` plus an error message: `os.rename` follows the same shape.
- Some raise (`error(...)`): `assert`, `tonumber` with a strict base, `package.loadlib` on certain failures.
- Some produce `nan` or `inf` rather than failing: `math.sqrt(-1)`, division by zero.

Embedded host runtimes need a discipline. A script reading documentation and writing call sites should not have to remember per-function which shape applies.

Two broad patterns dominate:

- **Always return tuples.** Every fallible function returns `(value, err)` where exactly one is `nil`. Easy to write call sites for; verbose for chained calls; conflates programming errors with runtime failures.
- **Always raise.** Failures propagate as Lua errors, caught with `pcall`. Idiomatic for some Lua libraries; pushes recovery cost onto every call site that might fail; loses the direct call-site signal.

A hybrid is more honest: distinguish *recoverable* failures (the file is missing; the network is down; the parser was given malformed input) from *programming errors* (the wrong type was passed; a closed handle was reused; a non-sharable userdata was given to a sharing surface). Recoverable failures are runtime conditions; programming errors are bugs.

## Decision

`neoc` modules use two failure shapes, chosen per function based on the kind of failure being signalled:

`(value, err)` tuple
:   The function returns two values. On success, `value` holds the result and `err` is `nil`. On failure, `value` is `nil` and `err` is a string. This shape is used for *recoverable* failures — conditions a script is expected to inspect and react to.

Raised error
:   The function calls `error(...)`, propagating up the stack until caught by `pcall` or `xpcall`. This shape is used for *programming errors* — conditions that indicate a bug in the script rather than a runtime condition the script should handle.

The choice of shape is fixed per function and documented on the relevant module reference page. The two shapes are not interchangeable.

Every error message — whether returned in the `err` slot of a tuple or raised — begins with a qualified prefix that names the module and function: `"fs.read_to_string: No such file or directory (os error 2)"`, `"thread.spawn: shared.k is 'JsonNull', which is not Sendable. Use args for by-value transfer, or use a Sendable type (Map, Counter, TcpListener)."`. The prefix is consistent across every module so that scripts can match against it programmatically and readers of stack traces can locate the failing call site.

## Consequences

- Script authors can read a function's signature and know how to handle its failures without consulting prose. A function that returns `(value, err)` is recoverable; a function that returns a single value may still raise but only on misuse.
- The discipline scales: every new module follows the same rule, and every existing module's surface is auditable against it.
- Recoverable failures and programming errors look different at the call site. A script that wraps a `pcall` around an I/O call has misunderstood the surface.
- Functions that change shape — moving from raise to tuple, or vice versa — are breaking changes for scripts that handle their failures. The choice of shape at first introduction matters.
- Some upstream crates raise where the neoc module surfaces a tuple, or vice versa. The Lua adapter layer is responsible for translating, and that translation is documented per module.
