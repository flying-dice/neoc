# Static semantics

Defines which syntactically valid programs are *meaningful*: how names resolve, what types things have, which constructs are well-formed. Says nothing about what programs *do* at runtime — that belongs to `DYNAMIC.md`.

Notation: inference rules as defined in `NOTATION.md` (`Γ ⊢ e : τ`, etc.) plus prose where rules would obscure rather than clarify.

A program that passes static checking is well-typed. A well-typed program has a meaningful lowering under `DYNAMIC.md`.

---

## 1. Notation

Defined in `NOTATION.md`. Summary:

- `Γ` — type environment, mapping identifiers to types and type names to their definitions.
- `Γ ⊢ e : τ` — under `Γ`, expression `e` has type `τ`.
- `Γ ⊢ p : τ ⇒ Γ'` — under `Γ`, pattern `p` matches values of type `τ` and extends the environment.
- `Γ ⊢ d ⇒ Γ'` — declaration `d` extends the environment.
- `Γ ⊢ τ ok` — type `τ` is well-formed under `Γ`.

## 2. Scopes and name resolution

neoc has two namespaces: the **value** namespace (functions, constants, bindings, struct/variant constructors) and the **type** namespace (structs, enums, traits, type aliases, type parameters, primitives). The same identifier may name a value and a type without conflict (`struct Id(number)` introduces a type `Id` and a constructor value `Id`).

Scoping is lexical and block-structured:

- A source file is a module scope. Items are visible throughout the module regardless of order (forward references between items are allowed).
- Each `Block` (`SYNTAX.md` §6.1) opens a nested scope. A `let` binding is visible from the statement after it to the end of its block.
- Inner bindings shadow outer bindings of the same name in the same namespace. Shadowing is allowed and is not a warning.

Name resolution order for an unqualified name: innermost block scope outward to the module scope, then names brought in by `use`, then the prelude (`Ok`, `Err`, `Some`, `None`, `Result`, `Option`, and the predeclared type names `number`, `string`, `bool`), then `ext` declarations.

A qualified path `a::b::c` resolves `a` as a module or type, then selects `b`, then `c` within it. `Type::method` names an associated function; `Enum::Variant` names a variant constructor.

`pub` controls cross-module visibility only; within a module everything is visible. A `use` of a non-`pub` item from another module is a resolution error.

## 3. The type system

### 3.1 Primitive types

`number`, `string`, `bool`, and the unit type `()` are predeclared.

```
─────────────────── (T-INT)        ─────────────────── (T-FLOAT)
Γ ⊢ INT : number                    Γ ⊢ FLOAT : number

─────────────────── (T-STR)         ─────────────────────── (T-BOOL)
Γ ⊢ STRING : string                 Γ ⊢ true : bool   Γ ⊢ false : bool

─────────────── (T-UNIT)
Γ ⊢ () : ()
```

`number` is a single numeric type — there is no integer/float distinction in the type system, only in literal syntax (`LEXICAL.md` §6.1–6.2). This is a deliberate simplification: the target has one numeric type, so neoc does too.

### 3.2 Function types

A function type is `fn(τ₁, …, τₙ) -> τ`. Application requires the argument count and types to match exactly; there is no partial application, no variadics, no default arguments.

```
Γ ⊢ f : fn(τ₁, …, τₙ) -> τ    Γ ⊢ eᵢ : τᵢ   (for each i)
──────────────────────────────────────────────────────── (T-CALL)
Γ ⊢ f(e₁, …, eₙ) : τ
```

Closures (`SYNTAX.md` §6.6) have function types; their parameter and return types are inferred from the body and from the context that consumes them.

### 3.3 Struct types

Structs are nominal: two structs with identical fields but different names are distinct, non-interchangeable types. A struct literal must name every field exactly once (unless completed with `.. base`), and each field expression must match the declared field type.

```
struct S { f₁: τ₁, …, fₙ: τₙ } ∈ Γ    Γ ⊢ eᵢ : τᵢ   (for each i)
──────────────────────────────────────────────────────────────── (T-STRUCT-LIT)
Γ ⊢ S { f₁: e₁, …, fₙ: eₙ } : S
```

Field access `e.f` requires `e` to have a struct type declaring `f`:

```
Γ ⊢ e : S    struct S { …, f: τ, … } ∈ Γ
───────────────────────────────────────── (T-FIELD)
Γ ⊢ e.f : τ
```

A newtype `struct N(τ)` has one positional field accessed as `.0`; its constructor `N` has type `fn(τ) -> N`. These field types are enforced at runtime wherever a value can drift from them — §10.

### 3.4 Traits and trait bounds

A trait names a set of method signatures. A type satisfies a trait when an `impl Trait for Type` provides every required method with matching signatures. A trait bound `T: Trait` constrains a type parameter to types that satisfy `Trait`.

Method resolution for `e.m(…)`: the static type of `e` determines which inherent or trait-impl method named `m` applies. There is no dynamic dispatch and no `dyn Trait`; every method call resolves to a single statically known function.

### 3.5 Generics

Type parameters are universally quantified over the item that declares them. Instantiation infers type arguments from the call site (`T-CALL` with `τᵢ` unified against argument types) and checks each inferred argument against its bounds.

neoc uses Hindley–Milner inference restricted to the rank-1, no-subtyping fragment: inference flows from arguments and annotations; there is no variance, no implicit coercion (not even `number`-to-`string`), and no inference across function boundaries beyond the declared signature. A type annotation is required wherever this fragment cannot determine a type (empty list literals, ambiguous numeric contexts produced by generic returns).

### 3.6 `Result<T, E>` and `Option<T>`

These are ordinary library enums, predeclared in the prelude:

```
enum Result<T, E> { Ok(T), Err(E) }
enum Option<T>    { Some(T), None }
```

Their constructors `Ok`, `Err`, `Some`, `None` are in the value namespace unqualified. They are not primitive, but they are load-bearing: the `?` operator (§6) and `match` exhaustiveness (§7) both have rules that mention them by name.

## 4. Typing rules

### 4.1 Expressions

Beyond the literal and call rules above:

```
x : τ ∈ Γ                          Γ ⊢ e₁ : number   Γ ⊢ e₂ : number
─────────── (T-VAR)                ───────────────────────────────── (T-ARITH)
Γ ⊢ x : τ                          Γ ⊢ e₁ + e₂ : number       (likewise - * / %)

Γ ⊢ e₁ : string   Γ ⊢ e₂ : string   Γ ⊢ e₁ : τ   Γ ⊢ e₂ : τ   τ ∈ { number, string, bool }
─────────────────────────────────── (T-CONCAT)   ──────────────────────────────────────────── (T-EQ)
Γ ⊢ e₁ + e₂ : string                              Γ ⊢ e₁ == e₂ : bool        (likewise !=)

Γ ⊢ e₁ : bool   Γ ⊢ e₂ : bool       Γ ⊢ c : bool   Γ ⊢ e₁ : τ   Γ ⊢ e₂ : τ
─────────────────────────────── (T-AND)   ───────────────────────────────────── (T-IF)
Γ ⊢ e₁ && e₂ : bool   (likewise ||)        Γ ⊢ if c { e₁ } else { e₂ } : τ
```

- `+` is overloaded on `number` (arithmetic) and `string` (concatenation) only — `T-ARITH` and `T-CONCAT`. No other type supports `+`.
- Comparison `< > <= >=` requires both operands to be `number` and yields `bool`. Equality `== !=` requires both operands to share one of `number`, `string`, `bool`, or a type deriving `Eq`, and yields `bool`.
- An `if` used for its value requires both branches and a common type (`T-IF`); an `if` with no `else` has type `()` and its block's value is discarded.

Block typing: a block's type is the type of its final expression, or `()` if there is none.

```
Γ ⊢ stmt₁ ⇒ Γ₁   …   Γₙ₋₁ ⊢ stmtₙ ⇒ Γₙ   Γₙ ⊢ e : τ
──────────────────────────────────────────────────────── (T-BLOCK)
Γ ⊢ { stmt₁ … stmtₙ e } : τ
```

### 4.2 Patterns

A pattern is checked against the scrutinee type and produces the bindings it introduces:

```
─────────────────────── (P-WILD)        ─────────────────── (P-BIND)
Γ ⊢ _ : τ ⇒ Γ                            Γ ⊢ x : τ ⇒ Γ, x : τ

struct S { f₁: τ₁, … } ∈ Γ    Γ ⊢ pᵢ : τᵢ ⇒ Γᵢ
──────────────────────────────────────────────── (P-STRUCT)
Γ ⊢ S { f₁: p₁, … } : S ⇒ Γ, (all bindings)

enum E { …, V(τ₁, …, τₙ), … } ∈ Γ    Γ ⊢ pᵢ : τᵢ ⇒ Γᵢ
──────────────────────────────────────────────────────── (P-VARIANT)
Γ ⊢ E::V(p₁, …, pₙ) : E ⇒ Γ, (all bindings)
```

A literal pattern checks against the scrutinee by equality and binds nothing. An or-pattern `p₁ | p₂` requires both alternatives to bind the same set of names at the same types.

### 4.3 Declarations

Each item form extends the environment with its name and type. A `fn` adds its name with its function type; a `struct`/`enum` adds its type and its constructors; an `impl` adds methods to an existing type; a `const`/`let` adds a binding; a `use` adds the imported name; an `ext` adds the declared binding (§5).

```
Γ ⊢ τ ok    Γ, params ⊢ body : τ_ret
──────────────────────────────────────────────────────── (D-FN)
Γ ⊢ fn f(p₁: τ₁, …) -> τ_ret { body } ⇒ Γ, f : fn(τ₁, …) -> τ_ret
```

The function body must have the declared return type. A function with no `-> τ` clause returns `()`, and its body must have type `()`.

## 5. `ext` declarations

An `ext` item introduces a binding the runtime supplies, with no body:

```
Γ ⊢ τᵢ ok   (for each i)   Γ ⊢ τ_ret ok
──────────────────────────────────────────────────────── (D-EXT-FN)
Γ ⊢ ext fn f(p₁: τ₁, …) -> τ_ret ; ⇒ Γ, f : fn(τ₁, …) -> τ_ret

──────────────────────────────────────── (D-EXT-TYPE)
Γ ⊢ ext type T ; ⇒ Γ, T : opaque type
```

An `ext type` is opaque: neoc knows the name and can pass values of it around and store them, but cannot construct, destructure, or inspect them — only `ext fn`s that mention the type can produce or consume its values. An `ext const` adds a constant of the declared type.

`ext` is the only construct that adds a binding without a corresponding definition in neoc source. Everything callable from neoc that is not defined in neoc has an `ext` declaration somewhere.

## 6. The `?` operator and well-formedness of error propagation

`e?` is well-typed only when the type of `e` and the return type of the enclosing function agree on their error/none discriminant:

```
Γ ⊢ e : Result<τ, ε>    enclosing fn returns Result<_, ε>
──────────────────────────────────────────────────────── (T-TRY-RESULT)
Γ ⊢ e? : τ

Γ ⊢ e : Option<τ>    enclosing fn returns Option<_>
──────────────────────────────────────────────────────── (T-TRY-OPTION)
Γ ⊢ e? : τ
```

For `Result`, the error type `ε` of `e` must unify with the error type of the enclosing function's return type. A `?` outside a `Result`- or `Option`-returning function is a static error.

## 7. Pattern-matching exhaustiveness

A `match` must cover every value of the scrutinee's type. Exhaustiveness is checked structurally:

- Scrutinee of an `enum`: every variant must be covered, either by a matching variant pattern or by a wildcard/binding pattern. A `match` over `Result<T, E>` is exhaustive iff it covers both `Ok` and `Err` (or has a catch-all); likewise `Some`/`None` for `Option`.
- Scrutinee of `bool`: both `true` and `false`, or a catch-all.
- Scrutinee of `number`, `string`, or any type with unbounded values: a binding or wildcard arm is required, since the literals can never be exhaustive alone.

A non-exhaustive `match` is a static error — never a runtime failure. Redundant arms (a pattern that can never match because earlier arms already cover it) are a static error.

## 8. Trait-impl completeness

An `impl Trait for Type` must provide every required method of `Trait` (those without a default body) with a signature matching the trait's, after substituting `Self` with `Type`. A missing required method is a static error. Providing a method not declared by the trait is a static error. A default-bodied trait method may be overridden or omitted.

## 9. Attribute well-formedness

Each built-in attribute constrains what it may attach to:

- `#[derive(…)]` — only on a `struct` or `enum`. Each derive name must be one of the built-in derivable traits (`Clone`, `Eq`, `Debug`, `Display`; `DYNAMIC.md` §6). Deriving `Eq` requires every field to be of a type that supports equality; deriving `Display` requires every field's type to itself satisfy `Display`.
- `#[test]` — only on a `fn` taking no parameters and returning `()` or `Result<(), E>`.
- Field-validation attributes — only on a struct field, and only when the field's type matches the constraint (`#[min(_)]` / `#[max(_)]` on `number`, `#[max_len(_)]` on `string` or `[T]`).

## 10. Drift-point analysis

Runtime type enforcement (`DYNAMIC.md` §9) emits a check only where a runtime value of a declared type can diverge from that type. This analysis classifies each value position as **drift** — a check is required — or **stable** — the type system already guarantees the shape, so a check would be dead code. The classification is the contract this file hands to `DYNAMIC.md` §9: a drift position lowers through its type's validator; a stable position lowers unchanged.

The judgements, in the predicate style of `Γ ⊢ τ ok`:

- `Γ ⊢ π drift` — value position `π` may carry a value whose runtime shape is not statically guaranteed; check it against its declared type.
- `Γ ⊢ π stable` — `π` cannot drift; emit no check.

### 10.1 Provenance

A value is **vouched** when the type system alone guarantees its runtime shape matches its static type, and **unvouched** otherwise. Every value is born vouched — a literal, or a value built from vouched parts — or enters unvouched across the host boundary (`ext`, decode, entry). A check converts unvouched to vouched.

Positions split into two kinds, and the analysis classifies them on different principles:

- **Pass-through** — a parameter, a call argument, a read (§10.2–§10.3). The value flows on unchanged. Here classification is *minimal*: a check goes exactly where an unvouched value first reaches such a position — the host boundary (`DRIFT-PARAM`, `DRIFT-INGRESS`) — and a vouched value passing through is never re-checked. This is sound because within neoc the type system is sound: a well-typed expression of type `τ` evaluates to a `τ`, provided every value it was built from was itself a `τ`. The host honours no such guarantee — it may hand neoc a number where a `string` was declared — so each crossing is suspect, and nothing purely internal is.
- **Typed write** — a construction component or a mutation target (§10.2, `DRIFT-CONSTRUCT` / `DRIFT-MUTATE`). Committing a value into typed storage is an *unconditional* barrier, checked at the write regardless of the component's provenance. This is deliberately not minimal, for two reasons: it is the site where a generic or opaque value carries a concrete witness, so its deep check lands here (§10.5) instead of being lost at a pass-through that knew only `T`; and it is the one moment neoc governs what enters storage that may later be aliased or handed back to the host.

So **stable** (no check) is a property of pass-through positions only. A typed write is always drift — see §10.6.

### 10.2 Drift positions

```
S { …, f: e, … } : S        N(e) : N        E::V(…, e, …) : E        [ …, e, … ] : [τ]
─────────────────────────────────────────────────────────────────────────────────────── (DRIFT-CONSTRUCT)
Γ ⊢ each component of the construction drift
```

A construction literal validates each component against its declared field, slot, or element type — recursively, so the validator descends the full shape (§10.5). The rule is **unconditional** — construction is a typed write (§10.1), not a pass-through, so it fires even when a component is provably vouched (e.g. an internally-computed `number` reaching a `number` field). That redundant check on a vouched component is the accepted cost of the barrier: it is where a generic or opaque value's deferred deep check lands (§10.5), and the point at which neoc fixes what enters storage. Host-reachability gates `DRIFT-PARAM` vs `STABLE-PARAM`; it does **not** gate construction.

```
Γ ⊢ lhs : τ    lhs is a field-access or index lvalue
──────────────────────────────────────────────────── (DRIFT-MUTATE)
Γ ⊢ the value of  lhs = v  drift against τ
```

Compound assignment `lhs op= v` is the `lhs = lhs op v` case of this rule.

```
f : ext fn(…) -> τ ∈ Γ
─────────────────────────── (DRIFT-INGRESS)
Γ ⊢ the result of f(…) drift against τ
```

`DRIFT-INGRESS` also covers a value produced by a host-defined decode or coercion declared to yield a neoc type, and each runtime-supplied entry argument (checked against the entry function's declared parameter types). These are the points where a host value first claims a neoc type.

```
g host-reachable    p : τ a parameter of g
─────────────────────────────────────────── (DRIFT-PARAM)
Γ ⊢ p at entry of g drift against τ
```

### 10.3 Stable positions

```
g internal-only (not host-reachable)        e : τ a call argument, callee internal       x : τ immutable, vouched at introduction
───────────────────────────────────── (STABLE-PARAM)  ────────────────────────────────── (STABLE-ARG)  ──────────────────────────────────── (STABLE-READ)
Γ ⊢ p at entry of g stable                            Γ ⊢ e as that argument stable                     Γ ⊢ a read of x stable
```

- **`STABLE-PARAM`** — an internal-only function is called only from neoc with statically-typed arguments, so its parameters are vouched without a check.
- **`STABLE-ARG`** — an internal neoc→neoc call passes already-vouched values; the callee re-checks at its own entry only if it is itself host-reachable (`DRIFT-PARAM`). Checking at both the call site and the entry would be redundant.
- **`STABLE-READ`** — reading an immutable binding cannot reintroduce drift; it was vouched when introduced and cannot have changed.

### 10.4 Host-reachability

A function is **host-reachable** iff it is `pub` (lowered to a binding shared with the host, `DYNAMIC.md` §2) or is the designated runtime entry. Every other function is **internal-only**: all its call sites are neoc with statically-typed arguments. Host-reachability is what separates `DRIFT-PARAM` from `STABLE-PARAM` — the analysis must compute it before classifying parameters.

### 10.5 Depth and limits

- **Deep.** A drift check recurses through the full declared shape — nested structs, list elements, `Option`/`Result` payloads, enum-variant fields — so divergence at any depth is caught.
- **Opaque `ext type`** (§5) has no shape neoc can inspect; its check is presence only, and recursion stops at the opaque boundary.
- **`fn(…) -> _`** is checked as callable; a signature is not runtime-observable.
- **Bare type parameter** with no concrete witness at the position is checked only to the depth its static type reveals; the deep check lands where the parameter is instantiated to a concrete type — itself a `DRIFT-CONSTRUCT` or `DRIFT-INGRESS` position.

### 10.6 Always-on

Drift checks are unconditional — no debug/release split, no opt-out. A well-typed program's checks fire identically in every build. Conversely, a stable position emits no check at all, so "always-on" never means "everywhere": it means every *drift* position is always checked. Stable is a pass-through property only (§10.1, §10.3); a typed write — construction, mutation — is never stable, so the always-on guarantee covers every commit into typed storage.
