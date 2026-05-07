---
status: Proposed
date: 2026-05-04
---

# 0009. `--eval` / `-e` CLI flag for single-shot script execution

Date: 2026-05-04

## Status

Proposed

## Context

`neoc` is a batteries-included Luau runtime. The runtime already supports executing script files (`neoc script.luau`). The question before this decision is whether to add a second invocation mode — an `--eval` / `-e` flag that accepts a script string directly on the command line — and what contract that flag carries.

The motivation is the "sole LLM tool" premise: if a Luau runtime provides enough module coverage, a language model could use `neoc -e '...'` as its only tool call for filesystem, HTTP, data manipulation, process execution, database access, and more. This eliminates the current spread of bash, Python, Node, curl, jq, and bespoke CLI tools. The LLM constructs a complete script per invocation, sends it, and parses stdout.

The issue thread explored two invocation models before settling on eval:

**REPL (interactive loop)**

A persistent session that accepts statements one at a time, accumulates state, and echoes results. The natural model for human interactive use. Problematic for LLM callers: it requires managing session lifecycle, state may leak between unrelated script fragments, and the input/output boundary is ambiguous. The calling harness must track whether the session is in a clean state and handle prompts, continuation markers, and mode switches. Every major runtime that ships a REPL alongside a `-c`/`-e` flag treats them as distinct modes for good reason.

**Single-shot eval**

A flag that accepts a complete script string, executes it in an isolated process, and exits. Input is the flag argument; output is stdout and stderr. The LLM sends a script and gets a result — identical to how every other tool call works: invoke-and-return, not conversational.

The eval model is how every comparable runtime exposes this capability:

| Runtime | Flag | Behaviour |
|---|---|---|
| Python | `python -c 'code'` | Executes code string, exits. |
| Node.js | `node -e 'code'` | Executes code string, exits. |
| Ruby | `ruby -e 'code'` | Executes code string, exits. |
| Deno | `deno eval 'code'` | Executes code string, exits. |
| Bun | `bun -e 'code'` | Executes code string, exits. |
| Lune | `lune eval 'code'` | Executes code string, exits. |
| Lua | `lua -e 'code'` | Executes code string, exits. |
| Perl | `perl -e 'code'` | Executes code string (also supports multiple `-e` flags). |

The pattern is universal. The runtime industry has fully converged here.

Several design sub-decisions required explicit resolution:

**Implicit vs explicit print.** `node -e 'expr'` does not print the expression value. `python -c 'expr'` also does not. Printing is always the result of an explicit `print()` call in both. Implicit printing of the final expression would introduce ambiguity for side-effect-only scripts, scripts that intentionally return `nil`, and scripts that build intermediate values before a final output. The consistent choice across the runtime survey is explicit-only.

**Multi-statement scripts.** The flag accepts a full program string, not a single expression. Function definitions, conditionals, loops, and multi-step pipelines are all valid input. This is not a question requiring a decision — every runtime that has `-e` handles multi-statement input. The shell handles multi-line quoting.

**Exit code semantics.** A clean exit maps to 0; any unhandled error maps to non-zero with the message on stderr. There is currently no programmatic exit mechanism available to scripts — `os.*` is stripped (ADR 0002) and `std:process` does not yet exist (planned in issue #23). Scripts that reach the end of their body without error produce exit code 0; unhandled `error()` calls produce a non-zero exit code. A `process.exit(n)` equivalent will be available once `std:process` ships.

**stdin piping.** Reading stdin in eval mode is desirable for pipeline composition (`cat data.json | neoc -e '...'`). However, `std:io` currently exposes reader handles via `io.from_string()` with `reader:read(n)`, `reader:read_line()`, and `reader:read_all()` — there is no stdin reader factory. Stdin support in eval mode is deferred until a stdin reader is added to `std:io`.

**Startup time.** Eval pays cold-start cost on every invocation. A REPL amortises it across many inputs. At 30–50 eval calls per LLM task, a 500ms cold start adds 15–25 seconds of pure overhead. The performance envelope is a commitment, not a preference: <100ms target, <200ms hard ceiling for `neoc -e 'print("ok")'` on CI hardware. Lazy module loading — only loading what `require()` touches — is the mechanism that makes this achievable. Eager loading of all registered modules at process start is incompatible with this target.

**Multiple `-e` flags.** Perl and Ruby support multiple `-e` flags that compose into one script (executed in order, sharing the same scope). The capability is real and useful for LLM use cases where a caller builds scripts from fragments. However, a single `-e` with embedded newlines covers the same use cases. Multiple `-e` composition is deferred to v2.

**Structured output wrapper.** A `--json` flag wrapping stdout in `{"ok": true, "result": ...}` / `{"ok": false, "error": "...", "line": N}` would simplify LLM consumption. The value is real. However, it adds API surface that can be added without any compatibility break. v1 ships with raw stdout/stderr. Scripts that need structured output use `vnd:serde_json` explicitly: `local serde_json = require("vnd:serde_json"); print(serde_json.to_string(result))`.

**Environment variable access.** `std:env` and `std:process` are not currently registered modules. Issue #23 plans `std:env` for environment variables, CLI arguments, and platform detection. Process spawning is planned under `std:process`. Eval scripts will access both via `require()` once those modules ship.

## Decision

`neoc` ships an `--eval` / `-e` CLI flag that executes a script string in a clean, isolated process and exits.

The contract is:

1. **Invocation.** `neoc -e 'script'` or `neoc --eval 'script'` accepts a complete Luau program as a string argument. The string is a full Luau program: multiple statements, function definitions, loops, and conditionals are all valid.

2. **Output.** Only explicit `print()` calls write to stdout. There is no implicit printing of the final expression's value, no welcome banner, no prompt, no decoration. stdout is exclusively what the script writes.

3. **Errors.** Unhandled Lua `error()` calls and uncaught runtime errors write the error message and traceback to stderr and produce a non-zero exit code. Clean completion produces exit code 0. Programmatic exit with a specific code is not available until `std:process` ships (issue #23).

4. **stdin.** Stdin piping is not available in v1 — `std:io` has no stdin reader factory. Pipeline composition (`cat data.json | neoc -e '...'`) is deferred until a stdin reader is added to `std:io`.

5. **Module access.** All `std:*`, `lib:*`, and `vnd:*` modules are available via `require()` in eval mode, identical to script-file mode.

6. **Startup time.** Modules are loaded lazily on `require()`, not eagerly at process start. The startup benchmark `neoc -e 'print("ok")'` is measured on CI. The target is <100ms; the hard ceiling is <200ms.

## Consequences

- LLM callers can use `neoc -e` as a single tool that covers filesystem access, HTTP, data processing, database access, and everything else in the module library. The "sole tool" premise becomes achievable as the module library grows.
- The flag's behaviour is immediately recognisable to anyone who has used `python -c`, `node -e`, `ruby -e`, `deno eval`, or `lua -e`. There is no learning curve; there is no neoc-specific behaviour to explain.
- The startup-time commitment creates a measurable CI benchmark. Future module authors must not introduce eager loading. Violations are caught before they compound.
- Deferring the `--json` wrapper and multiple `-e` composition to v2 keeps the v1 surface minimal. Both features can be added without breaking backward compatibility when there is evidence the use case demands them.
- Module coverage — not the flag itself — determines whether the "sole tool" premise holds. The flag is a day of engineering; the module library is months. The gap analysis documented in issue #14 identifies the priority order: `std:process` (#7), `lib:regex` (#16), `lib:glob` (#24), `lib:datetime` (#17), `lib:diff` (#29), and `lib:crypto` (#9) are the blocking modules before the sole-tool claim is true.
- Environment variable access and process spawning require `std:env` and `std:process` (issue #23) before eval scripts can fully replace shell one-liners. These are planned; they are not available in v1.
- v1 raw stdout/stderr output means LLM callers that want structured output must either serialise explicitly via `require("vnd:serde_json")` and `print(serde_json.to_string(result))`, or parse stdout themselves. This is the same contract as `python -c` and `node -e`. It is not a regression.

## References

- Originating issue: [flying-dice/neoc#14](https://github.com/flying-dice/neoc/issues/14)
- `std:env` / `std:process` split: [flying-dice/neoc#23](https://github.com/flying-dice/neoc/issues/23)
- `std:process` (process execution): [flying-dice/neoc#7](https://github.com/flying-dice/neoc/issues/7)
- `lib:crypto`: [flying-dice/neoc#9](https://github.com/flying-dice/neoc/issues/9)
- `lib:regex`: [flying-dice/neoc#16](https://github.com/flying-dice/neoc/issues/16)
- `lib:glob`: [flying-dice/neoc#24](https://github.com/flying-dice/neoc/issues/24)
- `lib:datetime`: [flying-dice/neoc#17](https://github.com/flying-dice/neoc/issues/17)
- `lib:diff`: [flying-dice/neoc#29](https://github.com/flying-dice/neoc/issues/29)
- [Lune CLI reference](https://lune-org.github.io/docs/the-book/1-getting-started/)
- [Deno eval subcommand](https://docs.deno.com/runtime/reference/cli/eval/)
