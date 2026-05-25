# CLI — the `neocc` command-line compiler

- **Status:** done
- **Area:** `crates/neocc` (lib + bin), uses `neocc-compiler`
- **Commit:** `7ce6f2c`

## Outcome

`neocc build PATHS [-o DIR]` (glob-resolve, transpile, write `.lua`) and
`neocc check PATHS` (parse + type-check, exit code) are implemented and tested
(11 tests driving the built binary + helper units). `fmt`/`lsp` dispatch to their
stub crates with a not-yet-implemented message. Diagnostics print as
`file: stage: message`; exit 0 only when every file is clean. Smoke-verified:
`neocc build` emits correct Lua. Logic lives in `src/lib.rs` for testability.

Note: a private `fn main` lowers to a Lua `local`, so it isn't reachable by an
external runner via `dofile`/`require` — invoking the entry point is the
runtime's job (`tasks/runtime-integration.md`).

---

(original spec follows)

- **Was:** P0 — the library `transpile()` worked, but no binary read `.neoc` off
  disk and wrote `.lua`.

## Goal

A `clap`-based `neocc` binary. Today `src/main.rs` just prints "not implemented".

Subcommands (see `plan.md` CLI surface and the master plan in
`../bun-oas/plan.md`):

- `neocc build <file|glob> [-o <dir>]` — `.neoc` → `.lua` written next to the
  source (or under `-o`). Surfaces diagnostics with file:span context.
- `neocc check <path>` — parse + type-check only, no emit; exit non-zero on
  diagnostics.
- `neocc fmt <path>` — format in place (depends on `formatter.md`; stub until then).
- `neocc lsp` — launch the language server over stdio (depends on `lsp.md`).

## Acceptance

- [ ] `neocc build examples/foo.neoc` writes `examples/foo.lua`.
- [ ] Diagnostics print with a useful location and a non-zero exit.
- [ ] `neocc check` exits 0 on a clean program, non-zero on an ill-typed one.
- [ ] Integration tests under `crates/neocc/tests/` driving the binary.
- [ ] clippy + fmt clean; `anyhow` is fine in the bin (not in libraries).

## Notes / pointers

- `neocc_compiler::transpile(src) -> CompileOutput { lua, diagnostics, ok() }`.
- Diagnostics are currently span-free (carry only a message); the parser keeps
  CST spans — surfacing real spans may want a small diagnostic-span thread
  through `neocc-compiler` (coordinate with the AST-span limitation in `plan.md`).
- Keep host-language details out of user-facing output (golden rule).
