# Trait & type model: traits-as-types, no `dyn`, nominal runtime tags

- **Status:** plan — RFC-gated (changes `lang/STATIC.md`, `SYNTAX.md`, `DYNAMIC.md`)
- **Priority:** P2 — the trait system is first-edition-shallow; this is the gap
- **Depends on:** `#54` runtime type enforcement (the drift-point model this rides);
  `scope-aware-resolution` (the resolution map traits resolve through)
- **Supersedes:** the "full trait richness" framing in `cross-file-resolution.md` §5

## The decision (what we converged on)

neoc is **Rust's semantics minus lifetimes/ownership/borrowing** — GC replaces
those. Traits are in scope and must be rich, but the model is shaped by the
runtime (dynamic dispatch, GC, a tagged-table value model), not copied from Rust.

Concretely:

1. **Traits are nominal.** A type implements a trait only via an explicit
   `impl Trait for Type`. Structural ("has the methods") is rejected: it admits
   accidental matches and cannot be verified deeply in the target (Lua can't
   introspect a method's signature). Nominal is also what makes the `#54`
   runtime check cheap (a tag lookup, not a deep shape walk).

2. **A trait name is usable directly as a type — this *is* the existential.**
   `fn draw_all(items: [Drawable])` accepts a heterogeneous list of *any*
   implementors. There is **no `dyn` keyword**, no `Box<dyn>`/`&dyn`, no vtable,
   no object-safety rules. Rust's `dyn` flags "this costs a vtable indirection";
   in a tagged dynamic runtime there is no vtable and nothing to flag, so the
   keyword carries zero information. The capability survives; the ceremony goes.

3. **Generics (`<T: Trait>`) are the orthogonal, type-*preserving* tool.** They
   are not a different kind of dispatch (everything dispatches dynamically) — they
   are how the checker *remembers the concrete type through a signature*:
   `pick<T: Drawable>(xs: [T]) -> T` hands back a `Circle` (full concrete API),
   where `pick(xs: [Drawable]) -> Drawable` hands back the erased trait surface.
   Kept thin — only for the "preserve the concrete type" cases, not Rust's
   pervasive apparatus.

4. **The value-type ontology is exactly two leaves:** a *concrete type*, or
   *"implements a set of traits."* A generic `T` is not a third kind — it is a
   signature-level placeholder for a concrete type. Everything composes from
   those (a `[Drawable]` is a concrete list whose element leaf is the existential).

## Runtime model (ties to `#54`)

Every value carries `__struct: string` (its concrete type). Trait impls extend
this with `__traits: [string]` — the traits the value's type declares an `impl`
for, recorded on the type's metatable at codegen.

**Naming:** codegen emits `_struct` (single underscore) today; the decision is
to standardise on **double underscore** (`__struct`, `__traits`). A precursor MR
renames `_struct → __struct` across `codegen-lua`/`codegen-js` and the
conformance goldens (mechanical; pure churn, no behaviour). Phase 0 below.

**Emit `__traits` on *every* type unconditionally** — not only types used in a
trait-typed position. A value flowing existentially must already carry its tag,
or the `#54` check has nothing to read. Computed from the impls present in the
(post-expansion) module, so macro-generated impls are included automatically.

- **Existential use** (`x: Drawable`): the `#54` drift-point check is the
  *nominal assertion* `x.__traits ∋ "Drawable"`. O(1), strong, unambiguous —
  not a deep structural method-shape walk. Once asserted at the boundary, the
  value is a verified `Drawable`; internal neoc→neoc calls passing it on skip
  the check (`#54`'s "already-validated → redundant" rule).
- This is the answer to "what does the trait-type give us": it is the **declared
  type the enforcement keys on**, the **cheap nominal predicate**, and the
  **verified token** trait-gated code trusts.

**Hard dependency:** the `#54` runtime-enforcement machinery (validators,
drift-point checks) **does not exist yet**. The `__traits` tag is inert metadata
until #54 lands, so #54's drift-point mechanism is a prerequisite for the
existential's runtime safety — sequence it first (or co-deliver the trait-typed
drift point as part of #54).

**Concrete recovery is *not* part of this model.** Recovering the concrete type
from a `Drawable` value (to call `Circle`-only methods) needs a *downcast /
type-narrowing* construct neoc does not have (`match x as Circle`, `if x is
Circle`). The existential deliberately doesn't require it — heterogeneous
collections, trait-method dispatch, and bounds all work without ever recovering
the concrete type. Downcast is a **separate, later feature** (its own RFC); it
is where `__struct` earns a second use. Listing it here would smuggle an
unnamed language feature into the trait model — explicitly out of scope.

## What's missing today (the gap)

The checker has *syntactic* traits + concrete-type method lookup only:
- `infer_method_call` requires a `Ty::Named` receiver — a method on a **bounded
  type parameter** (`fn f<T: Display>(x: T) { x.show() }`) errors "no method".
  **Bound-directed method resolution is absent.**
- Bounds (`<T: Display>`, `where`) are **not enforced** — `attrs.rs`/`ty.rs`
  treat type parameters permissively. `T: Display` is documentation today.
- No trait-as-type existential, no heterogeneous `[Trait]`, no `__traits` tag.
- No associated types, supertraits, or blanket/generic impls.

## What we deliberately do NOT take from Rust

`dyn` keyword · `Box<dyn>`/`&dyn` · vtables · object safety · monomorphization.
All of these serve a static machine model (uniform vtable layout, zero-cost
static dispatch, known sizes) that a GC'd, dynamically-dispatched, tagged-table
runtime does not have. Dropping them is not a simplification *of* Rust's model —
it is the *correct* model for this runtime.

## Phased plan (each its own MR, spec-first)

Sequencing: **traits before macros** (`macro-expansion.md` is deferred behind
this). The two meet only at derives — see the note after the list.

0. `[x]` **Rename `_struct → __struct` + emit `__traits` unconditionally** — pure
   codegen + golden churn, no RFC (it's a `DYNAMIC.md` lowering detail, not a
   semantic change). Establishes the runtime tag the rest builds on. Confirm the
   wasm-leaf gate and conformance stay green. *Done: the rename predates this;
   `__traits` now emits on every trait-implementing type, transitively closed
   over supertraits, from `neoc_ast::trait_index` (shared by both backends).
   Lua puts it on the type table (`Type.__traits = { … }`); JS uses a `__t`
   registry. Inert until #54. `DYNAMIC.md` §3.2a.*
1. `[ ]` **Bound-directed method resolution + bound enforcement** — `x.show()`
   on `x: T where T: Display` resolves to the trait method; `<T: Display>` is
   enforced (the instantiating type must `impl Display`). ~150–260 lines wiring
   existing pieces (bounds are parsed but unread today; `TraitInfo` methods and
   `check_trait_impl` already exist). Smallest real slice; seeds the resolver.
   `STATIC.md` §method-resolution + §bounds. RFC-gated.
2. `[ ]` **Trait-as-type (the existential) + `__traits` tagging** — a trait name
   as a type; `[Trait]` heterogeneous collections; codegen emits `__traits` on
   each type's metatable; `#54` drift-point check becomes the nominal
   `__traits ∋ T` assertion. `SYNTAX.md` (trait-as-type), `STATIC.md` (where it's
   allowed, how `[Trait]` checks), `DYNAMIC.md` (the tag + the assertion).
3. `[ ]` **Associated types** — `dyn Iterator<Item = number>`-style binding at
   the type level (runtime unaffected; checker tracks the binding).
4. `[ ]` **Supertraits** — `trait A: B`; `__traits` includes the transitive set.
5. `[ ]` **Blanket / generic impls** — `impl<T: A> B for T`; the point where a
   real (still non-recursive-first) solver/coherence story is needed. (Coherence
   /overlap is genuinely hard — this is the one slice where "simple solver" is
   optimistic; budget for it.)

### Derives are the seam with macros

The four built-in derives (`Clone`/`Eq`/`Debug`/`Display`) **are trait impls**,
but today they are synthesised at *codegen* (`neoc_macros::derive` → Lua lines),
invisible to the checker. For traits to cover them — a derived-`Eq` type usable
as `Eq`, tagged in `__traits` — the checker must treat a derive as an `impl`.
Until macros land, do this with a **hardcoded bridge**: the checker/codegen know
the four built-in derives produce impls of those four traits. When macros land
(`macro-expansion.md`), the expansion stage turns `#[derive(..)]` into real
`impl` blocks in the AST and the bridge is deleted — macros *subsume* it. So
fixing traits first does not undermine macros; it pins the contract macros must
satisfy (emit `impl` AST, which `__traits` is computed from).

## Acceptance (per slice)

- A value usable wherever its `__traits` includes the required trait; rejected
  (at the `#54` boundary, at runtime; statically where provable) otherwise.
- Heterogeneous `[Trait]` type-checks and runs; a non-implementor is rejected at
  ingress with a `#54`-shaped, path-aware error.
- Bound methods resolve and bounds are enforced.
- Tooling (completion/goto/references/hover) keys on the trait surface for a
  trait-typed position and the concrete surface for a concrete one — through the
  scope-aware resolution map, not a bespoke resolver.
- `lang/CONFORMANCE` pairs per construct; `check`/codegen goldens green.

## Open questions (for the RFCs)

- Exact `__traits` representation (flat set on the metatable vs a registry) and
  how supertraits flatten into it.
- Generic *erasure vs dictionary-passing* at codegen — likely erased (one code
  path, dynamic dispatch), but spell it in `DYNAMIC.md`.
- How much of generics' type-preservation is worth tracking vs collapsing to the
  existential, given the runtime erases anyway (`__struct` recovers concretes).
- Coherence/overlap rules once blanket impls land (the first place a solver,
  even a simple one, is unavoidable).
