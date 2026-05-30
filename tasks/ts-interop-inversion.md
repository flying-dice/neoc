# TS interop, inverted: neoc-owned ports, generated TS contracts

Status: **proposed** (Design). Supersedes the autots TS→neoc generator.

## Problem

autots reverse-engineers the whole TypeScript world (`bun:sqlite` + globals +
transitive deps) into one `types.neoc`. On the `bun-http-sqlite` demo that is
~72k lines with **~19k `Unknown`s**: TS has shapes neoc can't express (arbitrary
unions, `any`, anonymous objects, conditional types) and they all degrade to
`Unknown` — which you can't `match`, can't call generically, can't trust. The
lossy mapping also forced host-shaped language features (optional `?` params,
#78) that exist only to mirror TS quirks.

The *direction* is the problem: mapping **into** neoc from an unbounded, messier
type universe is inherently lossy.

## Decision

Invert it. **neoc owns the contracts; TypeScript implements adapters.**

- You write the application in neoc and declare the *ports* it depends on — in
  neoc's own types (`struct`, `enum`, `Result`, `Option`).
- A single **neoc→TS projector** projects neoc types/signatures to TypeScript.
- The integrator writes the TS *adapter* that implements the generated contract
  (wrapping `bun:sqlite`, `fetch`, …).

neoc→TS is **total and lossless** — every neoc type has an exact TS form — so
`Unknown` cannot arise. Conformance is automatic (neoc authored the types): no
enforcement pass, no degradation, no `?`-to-mirror-host features.

## One projector, three surfaces

The neoc→TS projector (`neoc type/sig → TS type/decl`) is the keystone, reused by:

1. **Emitted-JS typings** — every `x.neoc` compiled to `x.js` gets a sibling
   `x.d.ts` declaring its exports, so TS consumers call neoc output type-safely.
2. **Runtime prelude** — today's inlined `Result`/`Option`/enum prelude is
   factored into a shared `neoc-lib.js` + `neoc-lib.d.ts`.
3. **Ports** — a neoc `ext` port projects to `<port>.port.ts` (a TS `interface`
   plus the referenced types). The adapter `implements` it.

A JS build therefore emits, in total: `neoc-lib.js`, `neoc-lib.d.ts`, and
`x.js` + `x.d.ts` per module — plus a `*.port.ts` per declared port.

## Projection (neoc → TS), lossless

| neoc | TypeScript |
| --- | --- |
| `number` / `string` / `bool` | `number` / `string` / `boolean` |
| `()` (unit) | `void` |
| `struct S { a: number }` | `interface S { a: number }` |
| `enum E { A, B(number), C { x: string } }` | discriminated union `{ tag: "A" } \| { tag: "B", _0: number } \| { tag: "C", x: string }` |
| `Result<T, E>` | generated `Result<T, E>` (tagged `Ok`/`Err`) — from `neoc-lib` |
| `Option<T>` | generated `Option<T>` (tagged `Some`/`None`) — from `neoc-lib` |
| `[T]` | `T[]` |
| `fn(A) -> B` | `(a: A) => B` |
| `ext type H` | an opaque branded handle type |
| generics `<T>` | TS generics `<T>` |

The enum / `Result` / `Option` tag shapes mirror the runtime lowering in
`DYNAMIC.md`, so the `.d.ts` describes the *actual* emitted JS values — that is
what makes `match` round-trip across the boundary.

## Ports & runtime wiring

A port is an `ext` block authored in neoc (the existing seam — no new construct
required; a first-class `port` keyword is a possible later nicety, out of scope):

```
// app.neoc
struct Row { id: number, body: string }
ext db { fn query(sql: string) -> Result<[Row], string>; }
```

projects to `db.port.ts`:

```ts
import type { Result } from "./neoc-lib";
export interface Row { id: number; body: string }
export interface Db { query(sql: string): Result<Row[], string>; }
```

The adapter implements `Db` over `bun:sqlite` and binds at the boundary via the
seam that already exists per target:

- **JS target** — the emitted neoc JS `import`s the adapter module, the same ESM
  import a `use` already lowers to. No new runtime mechanism.
- **Lua / neoc-vm target** — the adapter is a runtime `ext` binding, exactly like
  today's `std:` / `lib:` / `vnd:` modules.

To confirm during stage 3: that an `ext` port's binding is injectable as a plain
object of functions on both targets (expected yes).

## autots retires

Nothing imports a host module's types into neoc anymore — the adapter hides
`bun:sqlite` behind the port. So:

- deprecate `@neoc-sh/autots`; delete the ts-morph whole-world machinery,
- drop the `[autots]` block from `neoc.toml`,
- rewrite `bun-http-sqlite` to the ports+adapter shape (a `db` port + a small
  `bun:sqlite` adapter), replacing the 72k-line `types.neoc`.

If a "consume a clean existing TS lib directly, no adapter" path is ever wanted,
autots shrinks to a small conforming-subset importer rather than dying — but the
adapter pattern subsumes it, so the default is **retire**.

## Open questions

- `Option<T>` wire form: tagged (`Some`/`None`, matches the runtime) vs `T | null`
  (idiomatic TS). Leaning tagged, to match the emitted values.
- Prelude packaging: one `neoc-lib` per output dir, or per project root.
- Port construct: is an `ext` block enough, or is a first-class `port` warranted?
- Lua target has no `.d.ts`; the `.d.ts` surface is JS-target-only. Ports project
  regardless of target.

## Sequence

1. **neoc→TS projector** + emit `x.d.ts` siblings on the JS target — delivers the
   "typed neoc JS output" win immediately; independent of ports.
2. Factor the prelude into shared `neoc-lib.js` + `neoc-lib.d.ts`.
3. Project `ext` ports → `*.port.ts` (the projector's third consumer).
4. Rewrite `bun-http-sqlite` to ports + a `bun:sqlite` adapter.
5. Retire autots (deprecate package, drop config, delete machinery).
