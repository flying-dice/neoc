# Runtime integration — the `crates/neoc` runtime embeds `neoc-compiler`

- **Status:** todo
- **Area:** the `crates/neoc` runtime (Rust + mlua); `neoc-compiler` is
  the dependency
- **Priority:** P1
- **Depends on:** `cli.md` / a stable `neoc-compiler` API

## Goal

Wire the `crates/neoc` runtime to consume `neoc-compiler` so `neoc main.neoc`
compiles in memory (`neoc_compiler::transpile`) and runs the emitted Lua via
mlua — no `.lua` written.

Also: the runtime owns its API surface, exposed to neoc through `ext`
declarations (a `types/` directory of `.neoc` files the compiler seeds into the
type environment). The compiler reads them via `parse_ext` / a `TypeEnv` — note
the current `transpile(src)` has no env parameter yet; add the seam when this
lands.

## Acceptance

- [ ] `neoc main.neoc` (in the runtime repo) compiles + runs a `.neoc` program.
- [ ] The runtime's `ext` declarations type-check against real bindings.

## Open question (parked)

`Result` at the `ext` edge: runtime functions tend to `error()`/`pcall` or return
tuple errors. Decide whether the conversion lives in runtime-side wrappers
(leaning yes) and whether neoc gets a panic path. Revisit when designing the
boundary — don't pre-build compiler glue.
