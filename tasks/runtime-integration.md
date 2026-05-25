# Runtime integration — `../neoc` embeds `neocc`

- **Status:** todo
- **Area:** the separate `neoc` runtime repo (Rust + mlua); `neocc-compiler` is
  the dependency
- **Priority:** P1
- **Depends on:** `cli.md` / a stable `neocc-compiler` API

## Goal

Wire the `neoc` runtime to consume `neocc` so `neoc main.neoc` compiles in memory
(`neocc::transpile`) and runs the emitted Lua via mlua — no `.lua` written. This
is master-plan steps 12/14 (see `../bun-oas/plan.md`).

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
