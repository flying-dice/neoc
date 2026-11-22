# Luau

- Every app and fixture file starts with `--!strict` and must check clean under
  `bun run lint:luau` (no type error, no lint warning). Prefer locals and explicit
  module tables; avoid accidental globals and coercion-dependent logic.
- Type the module's surface: exported functions, state tables and unions of string
  literals for alternatives. Do not reach for `any` or a `::` cast to quiet the
  checker; where a cast is unavoidable, say why on the line above.
- Types enforce nothing at runtime. Validate values that cross a boundary (tool
  arguments, RPC payloads, stored rows, file contents) before trusting their type.
- Host and library calls that can fail return `(value, err)`, both optional. Check
  `err ~= nil` first, then narrow the value separately: checking the error does not
  narrow the value, and nil or false may be a valid success value.
- Preserve multiple return values when forwarding an outcome; assignment and
  expression position can discard the second result.
- Inside an app, a failure meant for a person or an agent is raised as an error
  object whose `tostring` is a plain sentence, and caught with `pcall` at one
  deliberate boundary (see `apps/calculator/server/calc.luau`: `fail`, `calc.reason`).
  Anything else caught there is a bug: show a generic sentence, never a source position.
- Reserve bare `error("...")` and failing `assert` for bugs and violated invariants.
  An `assert` that only narrows a type the code above has already established is fine.
- Do not introduce Result wrapper tables.
- Only require a relative path, `vos:app`, `vos:ui`, or a module the generated
  declarations offer (`types/generated/vos.d.luau`). Format with StyLua
  (`bun run lint:stylua`).

Reference: [Luau type checking](https://luau.org/typecheck).
