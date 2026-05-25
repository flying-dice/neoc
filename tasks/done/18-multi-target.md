# Multi-target backends — Lua, JavaScript, Python

- **Status:** done
- **Area:** `crates/neocc-compiler` (dispatch), `crates/neocc-codegen-{lua,js,py}`,
  `crates/neocc` (`--target`), `lang/CONFORMANCE/`
- **Commits:** `d72e2c5` (target dispatch), `0c88749` (corpus 20→30 + Lua fixes),
  `13d6577` (JS target), `1a7df12` (Python target)

## Outcome

The codegen layer is now multi-target. `transpile(source, Target)` dispatches to
a per-language backend; `neocc build --target lua|js|python` selects it and the
output extension. The conformance corpus (`lang/CONFORMANCE/dynamic/`) is the
shared feature contract — **30 cases**, each with `.neoc` + closest-Rust `.rs` +
golden `.stdout`, covering every implemented feature (fns/recursion, structs +
methods, enums + methods, destructure, arithmetic, comparison/bool, string
concat, newtype, struct update, match over bool/enum/option/result/guards, if,
loops/while/for/continue/break, the `?` operator, traits + impls, all four
derives, closures, generics).

All three targets reproduce every golden byte-for-byte, verified by a per-target
runner that transpiles each case and runs it under the real toolchain:

- **Lua 5.1** (`neocc-codegen-lua`) — the original, completed: fixed `?`
  short-circuit, structural `Eq` (`==`→`eq`), generic struct projection.
- **JavaScript ES2022** (`neocc-codegen-js`) — runs under `node`; tagged-object
  reprs, `__call` registry dispatch, IIFEs for expression-position blocks.
- **Python 3.9** (`neocc-codegen-py`) — runs under `python3`; classes + `_Enum`,
  `match`→`if`/`elif`, a `_fmt` shim so floats print without `.0` and bools
  lowercase, matching the goldens.

## Not in the language surface (no cases)

`as` casts and `loop`-break-with-value — neither parses today (not in the
grammar/AST), so they are absent rather than deferred. See
`tasks/non-goals.md` for intentional deferrals.
