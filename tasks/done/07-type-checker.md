# Type checker (`neocc-types`)

- **Status:** done (intentional limits in `tasks/non-goals.md`; §9 in `attribute-wf.md`)
- **Area:** `crates/neocc-types`
- **Commits:** `5d6fefa` (checker), `71489aa` (elaborate), `92b33d8` (derived methods)

## What was done

- Declaration collection into a `SymbolTable` (two namespaces, `Result`/`Option`
  prelude seeded), then a walking checker per `STATIC.md` §4–§8: unknown names,
  type mismatches, struct-literal field errors, function arity/arg types, field
  access on non-structs, `match` exhaustiveness + redundant arms, `?`
  well-formedness with error-type unification, trait-impl completeness.
- `elaborate(&Module) -> Module` rewrites string-typed `+` to `BinOp::Concat`
  (reusing the checker's inference) so the emitter picks Lua `..`. Wired into
  `transpile` (parse → check → elaborate → emit).
- Derived-method awareness: a `#[derive(…)]` registers `clone`/`eq`/`debug`/
  `display` on the type so they're callable from neoc (user `impl` wins on clash;
  full generic support).
- `check(&Module) -> CheckResult`; `collect`, `Ty`, `SymbolTable` exposed for the
  emitter. 50+ static-stage tests.
