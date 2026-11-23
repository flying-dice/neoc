---
name: coding-styleguide
description: Write or review ecosystem-idiomatic code with strong types (every API surface fully typed, no `any` unless the source is genuinely unknown), strict checks, explicit expected-error handling, and DDD/hexagonal boundaries. Includes language-specific references.
---

# Coding Styleguide

Read neighboring code, supported language versions, and project checks first.
Apply these defaults to the requested change; preserve required framework contracts
and avoid unrelated API migrations, dependency additions, or configuration churn.

- Prefer idiomatic standard-library patterns and the strongest practical type safety.
  Model distinct states explicitly; validate external data before trusting its type.
- Preserve strict checking. Do not silence errors with unsafe casts, `any`, ignored
  diagnostics, or fallback values that disguise failure as success.
- Use a language/standard-library `Result` where provided; otherwise follow the
  convention in the language reference below. Do not invent Result wrappers or add
  Result libraries. Check, handle, or propagate the error before consuming the value.
- Reserve throws/panics for unexpected bugs and broken invariants, not validation,
  business rejection, or foreseeable operational failures. Preserve checked-error APIs.
- At dependency boundaries, translate known exceptions into expected outcomes with
  useful context and causes. Let unknown bugs escape; preserve cancellation and cleanup.
- Verify success, relevant expected failures, and propagation using existing checks.

## Fully typed APIs

Every API surface MUST be fully typed: every parameter, option, body, return value, stream item
and error kind gets its real shape in the declarations (`types/generated/vos.d.luau`, `vos.d.ts`)
and in the code that produces them. This is not a preference; an untyped surface is a defect.

- `any`, `unknown`, `{ [key: string]: any }`, `Value::decl_type()`, an erased `serde_json::Value`
  or an equivalent "anything" type is banned on a surface, unless the underlying API is
  **genuinely unknown**: its shape is decided at run time by data no one controls (what
  `json.decode` parses, what a script stored in `kv`, a user's own row). Where it is, say so
  in a comment on that line naming why no shape exists.
- A shape the upstream library already knows is not unknown. When a dependency has typed models
  (`bollard::models::ContainerSummary`, a generated OpenAPI type), expose those types; do not
  serialize them to JSON and erase them. If the type system cannot yet express them, extend the
  declaration machinery: that work is part of the change, not a follow-up.
- Types are read from the code, never written as strings beside it. A hand-written type string
  in an attribute or declaration helper (`#[lua(takes("err: string?"))]`, a literal Luau/TS type
  passed to a macro) is banned: it is a second source that drifts from the implementation. Give
  the Rust function real typed parameters and returns and let the declarations derive from them;
  if the machinery cannot read a case yet, extend it as part of the change.
- Shapes are never described in prose instead of types. A doc comment listing fields
  (`` `Id`, `Names`, `State`, ... ``) in place of a typed return is the defect this rule exists for.
- Errors are typed too: their kinds are a declared union of string literals (or a typed error
  value), not kinds listed only in a doc comment, and the success value is narrowed separately.
- Distinguish list from map, required from optional, and alternatives as unions of literals or
  discriminated unions. A request body is read through its Rust type, so the type decides an
  ambiguous value (an empty table is a list where the field is a `Vec`).
- In review, treat each untyped surface as blocking. Do not add a new one to match neighbors;
  when touching one, type it or record why its source is genuinely unknown.

## Architecture

When using DDD/hexagonal architecture, preserve the project's layout and keep dependencies inward:

- Domain owns models, policies, errors, and ports; no framework, platform, I/O, or outer-layer dependencies.
  Contexts communicate through ports and public APIs. Keep pure logic synchronous and port-free.
- Driving ports expose use cases; driven ports describe external needs. Technology-named adapters
  implement ports and translate known failures into domain errors using the return conventions above.
  Normal lookup absence is a successful absent value; distinguish it from an operation-blocking `NotFound`.
- UI/delivery calls injected ports. Composition roots construct adapters and own lifecycle,
  listeners, scheduling, and top-level logging. Prefer constructor/factory injection over service locators or globals.
- New surfaces reuse driving ports; new providers implement driven ports. Keep business rules in
  domain and shared code in its owning layer; avoid catch-all `util`/`common` modules.
- Test domain with plain units, UI with fake ports, adapters at technology boundaries including
  returned errors, and composition roots with smoke tests.
- Check imports and platform globals for isolation, public context boundaries, cycles, cross-surface
  or adapter coupling, and construction outside roots. Run existing boundary checks after moves;
  document debatable boundaries and intentional exceptions. Add enforcement only when in scope.

## Languages

Read only the references matching the code being changed:

| Language | Reference |
| --- | --- |
| Rust | [Rust](references/rust.md) |
| TypeScript | [TypeScript](references/typescript.md) |
| Svelte | [Svelte](references/svelte.md), plus TypeScript for its scripts |
| Luau | [Luau](references/luau.md) |

For other languages, apply the common principles through their native idioms.
