# TypeScript

- Prefer strict compiler checking; preserve the project's additional safety checks.
  Enable stricter configuration only within an authorized configuration change.
- Use `unknown` for untrusted values and narrow through validation. Type assertions
  do not validate data; avoid `any`, unchecked casts, and non-null assertions as escapes.
- Model alternatives as discriminated unions, not unrelated optional fields.
  Prefer immutable bindings and readonly contracts where mutation is not intended.
- Expected failures are typed `Error` subclasses that are thrown or reject (for
  example `ResultError`, `HostError`, `OpError`), caught where the caller can show
  or handle them. Follow the convention of the module being changed; do not introduce
  result tuples, Result wrappers or Result libraries.
- Give errors distinguishable classes and useful typed context. Narrow a caught value
  with `instanceof` before reading it; use a `never` exhaustiveness check where every
  case must be handled.
- Parse what crosses the host boundary with zod at the edge (see
  `client/src/shell/results.ts`); a failed parse is an error the caller shows, never
  a throw out of a render. Translate only known dependency failures; rethrow unknown ones.
- Preserve abort/cancellation behavior, and avoid catch-all conversions that turn
  programming errors into ordinary outcomes. Respect framework-required throw contracts.
- Prefer inference for local values and explicit public contracts. Use branded types
  only when domain distinctions warrant them; keep runtime validation at boundaries.
- Run the project's typecheck, lint, and relevant tests without bypass diagnostics.

Reference: [TypeScript strict checking](https://www.typescriptlang.org/tsconfig/strict.html).
