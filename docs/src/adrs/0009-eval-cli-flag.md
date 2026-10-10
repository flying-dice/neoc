---
status: Proposed
date: 2026-05-04
---

# 0009. `--eval` / `-e` CLI flag for single-shot script execution

Date: 2026-05-04

## Status

Proposed

## Context

neoc supports script files (`neoc script.lua`). Question: add `--eval`/`-e` for inline script strings?

Motivation: "sole LLM tool" premise. If module coverage is sufficient, `neoc -e '...'` replaces bash, Python, Node, curl, jq as a single tool call. LLM constructs script, sends it, parses stdout.

**REPL vs eval:** REPL requires session lifecycle management, leaks state, ambiguous I/O boundaries. Eval is invoke-and-return — matches every other tool call pattern.

Universal convergence: `python -c`, `node -e`, `ruby -e`, `deno eval`, `bun -e`, `lune eval`, `lua -e`, `perl -e`. Pattern is settled.

**Design sub-decisions:**
- **Print:** explicit only (no implicit final-expression print). Matches Node, Python.
- **Multi-statement:** full program, not single expression. Shell handles multi-line quoting.
- **Exit code:** clean = 0, unhandled error = non-zero + stderr. Programmatic exit deferred until `std:process`.
- **stdin:** deferred until `std:io` gets stdin reader.
- **Startup time:** <100ms target, <200ms ceiling. Lazy module loading required.
- **Multiple `-e`:** deferred to v2 (single `-e` with newlines covers same cases).
- **`--json` wrapper:** deferred to v2 (scripts use `vnd:serde_json` explicitly).

## Decision

`neoc -e 'script'` / `neoc --eval 'script'` — executes complete Lua program, exits.

Contract:
1. **Invocation.** Full Lua program as string argument.
2. **Output.** Only explicit `print()` writes stdout. No decoration.
3. **Errors.** Unhandled errors → stderr + non-zero exit.
4. **stdin.** Not available v1.
5. **Modules.** All `std:*`/`lib:*`/`vnd:*` available via `require()`.
6. **Startup.** Lazy loading. CI benchmark: `neoc -e 'print("ok")'` < 100ms target, < 200ms ceiling.

## Consequences

- LLM callers get single tool for filesystem, HTTP, data, DB access. "Sole tool" achievable as modules grow.
- Behaviour matches `python -c`/`node -e`/`deno eval`. Zero learning curve.
- Startup commitment creates measurable CI benchmark. No future eager loading.
- Module coverage (not the flag) determines sole-tool viability. Priority modules: `std:process` (#7), `lib:regex` (#16), `lib:glob` (#24), `lib:datetime` (#17), `lib:diff` (#29), `lib:crypto` (#9).

## References

- Issue: [flying-dice/neoc#14](https://github.com/flying-dice/neoc/issues/14)
- `std:env`/`std:process`: [flying-dice/neoc#23](https://github.com/flying-dice/neoc/issues/23)
- [Lune CLI](https://lune-org.github.io/docs/the-book/1-getting-started/)
- [Deno eval](https://docs.deno.com/runtime/reference/cli/eval/)
