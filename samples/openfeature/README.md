# OpenFeature in neoc

An in-process implementation of the [OpenFeature](https://openfeature.dev/specification/)
feature-flag evaluation flow, written in neoc. It implements the
specification's behaviour locally — there is no remote service — and doubles as
a multi-file benchmark for the neoc linter, formatter, and tooling.

## What it is

A typed flag-evaluation API over a pluggable provider, with evaluation context,
evaluation details, reasons, error codes, and a four-stage hook lifecycle. The
demo registers an in-memory provider, defines three flags, and evaluates them
through every path the specification cares about: a targeting match, a static
default, a disabled flag, a type mismatch, and a missing flag.

## Layout

```
samples/openfeature/
  main.neoc                 demo entry point (the only module with an I/O `ext`)
  openfeature/
    flags.neoc              FlagValue (Bool/Str/Num), Variant, Rule, FlagDefinition
    context.neoc            EvaluationContext (targeting key + attributes)
    evaluation.neoc         Reason, ErrorCode, ResolutionDetails, EvaluationDetails
    provider.neoc           Provider trait + in-memory provider with targeting
    hooks.neoc              Hook trait + LoggingHook (before/after/error/finally)
    client.neoc             typed evaluation API: get_{boolean,string,number}_details
```

The modules form a `use` graph rooted at `main.neoc`; `flags` is the leaf,
`client` and `main` sit at the top.

## Build, check, format

From the workspace root:

```sh
cargo run -p neoc -- check samples/openfeature/main.neoc
cargo run -p neoc -- fmt --check samples/openfeature/**/*.neoc samples/openfeature/main.neoc
cargo run -p neoc -- build samples/openfeature/main.neoc
```

`check` and `build` resolve the whole `use` graph from the entry. `build`
emits a `.lua` next to each `.neoc`.

## Running it

`neoc run` executes a single file; it does not resolve cross-module `require`,
and the runtime's `require` is namespace-only (`std:` / `lib:` / `vnd:`), so a
built multi-module project does not run directly under the runtime. To see the
program execute, build it and run the concatenated chunk (the emitted modules
share globals, so dropping the `require` bindings and the duplicate
`Result`/`Option` prelude yields one runnable chunk):

```sh
cargo run -p neoc -- build samples/openfeature/main.neoc
# concatenate flags→context→evaluation→hooks→provider→client→main, drop the
# `local X = require("openfeature.…")` lines and the repeated prelude, append
# `main()`, then:
cargo run -p neoc -- run <concatenated>.lua
```

Expected output: each evaluation prints its hook stages (`before`, then
`after` or `error`, then `finally`) followed by the resolved details — a
`TARGETING_MATCH`, two `STATIC`s, a `DISABLED`, a `TYPE_MISMATCH`, and a
`FLAG_NOT_FOUND`.

`print` and `tostring` are declared as `ext fn` and bind to the runtime's
built-in print and stringify primitives — the only seam to the host.

## OpenFeature features

Implemented:

- **Typed evaluation API** — `get_boolean_details`, `get_string_details`,
  `get_number_details` on the client, each returning evaluation details.
- **Provider** — a `Provider` trait and an `InMemoryProvider` with flag
  definitions, named variants, a default variant, and equality-based targeting
  rules driven by the evaluation context.
- **Evaluation context** — targeting key plus string attributes.
- **Evaluation / resolution details** — value, reason, variant, error code,
  error message, flag key.
- **Reasons** — the full standard set as an `enum` (`STATIC`, `DEFAULT`,
  `TARGETING_MATCH`, `SPLIT`, `CACHED`, `DISABLED`, `UNKNOWN`, `STALE`,
  `ERROR`).
- **Error codes** — the standard set as an `enum` (`PROVIDER_NOT_READY`,
  `FLAG_NOT_FOUND`, `PARSE_ERROR`, `TYPE_MISMATCH`, `TARGETING_KEY_MISSING`,
  `INVALID_CONTEXT`, `GENERAL`), plus a `None` for the no-error case (neoc has
  no null).
- **Hooks** — a `Hook` trait with `before` / `after` / `error` / `finally`,
  run in order around every evaluation, demonstrated end-to-end.
- **No-throw contract** — evaluation always returns the caller's default on any
  error path with reason `ERROR` and an error code.

Simplified, and why:

- **Object/structure flags** — the specification's fourth flag type is omitted.
  `FlagValue` covers boolean, string, and number; modelling arbitrary nested
  structures needs a recursive value type and a map, neither of which the demo
  needs to show the flow.
- **Single `resolve` instead of four typed provider methods** — `FlagValue` is
  already a tagged union, so one `resolve` returning a `ResolutionDetails`
  carries the type. The client recovers the per-type contract by checking the
  resolved value's type and raising `TYPE_MISMATCH` when it differs.
- **Hooks are one concrete type** — neoc has no dynamic dispatch (`dyn Trait`),
  so a heterogeneous list of differently-typed hooks cannot be stored. The
  client runs a list of `LoggingHook`; the `Hook` trait still defines the
  contract and the four stages fire in order.
- **Hook output via a sink** — hook stages emit through a `fn(HookLog)` sink
  rather than a returned list, because neoc has no list-append operator. The
  sink also surfaces each stage in real firing order.
- **Targeting is attribute equality** — one rule matches one attribute against
  one expected value. Richer predicates (ranges, fractions, set membership)
  are out of scope for the demo.

## Notes for the tooling (cross-module lowering)

This sample deliberately exercises the multi-module path, which surfaced
several places where the current neoc codegen loses information across a `use`
boundary. The source is written to stay correct under these; each is commented
at its site and collected here:

- **Type-directed `+`** — `+` lowers to Lua `..` (string concat) or `+`
  (arithmetic) based on the static type of its operands. When an operand's type
  comes from an imported method return or struct field, the emitter cannot see
  it is a `string` and emits `+`, which faults at runtime. Worked around by
  binding such operands to an annotated `let x: string = …` before
  concatenating.
- **Imported enum construction** — constructing an imported enum's variant
  (`FlagValue::Bool(b)` from another module) lowers to a bare, unbound call.
  Worked around with free-function constructors (`flag_bool`,
  `resolution_targeting_match`, …) defined in the enum's own module.
- **Imported associated functions** — `Type::assoc_fn` called from another
  module drops the type prefix and lowers to an unbound name. Worked around by
  exposing free functions (`provider_new`, `client_new`, `evaluation_of`,
  `anonymous_context`) instead of associated functions.
- **Imported enum `match`** — matching an imported enum's variants from another
  module emits the wrong runtime test. Worked around by keeping every `match`
  on a type inside the type's own module (e.g. `FlagValue::is_bool`).
- **Imported types must be `use`d to reach their methods/fields** — touching a
  method or field of an imported type requires an explicit `use` of that type,
  even when it is only reached transitively through another value's field.
- **Private `fn` definition order** — a private `fn` referenced before its
  definition lowers to a `local function` whose forward reference resolves to
  nil at runtime, even though name resolution accepts the forward reference.
  Private helpers are defined ahead of their callers.
