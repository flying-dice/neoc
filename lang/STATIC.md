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

Name resolution order for an unqualified name: innermost block scope outward to the module scope, then names brought in by `use`, then the prelude (`Ok`, `Err`, `Some`, `None`, `Result`, `Option`, and the predeclared type names `number`, `string`, `bool`), then `extern` declarations.

A qualified path `a::b::c` resolves `a` as a module or type, then selects `b`, then `c` within it. `Type::method` names an associated function; `Enum::Variant` names a variant constructor.

`pub` controls cross-module visibility only; within a module everything is visible. A `use` of a non-`pub` item from another module is a resolution error.

A `use` names a module by its path and an item within it; the module path resolves to a single source file. The directory that path is resolved against — the source root of a project, or the importing file's own directory outside one — is defined in `PROJECT.md` §4. A `use` whose module path resolves to no file is a resolution error.

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

Inside an `impl` body, `Self` may head a struct literal (including `Self { f, .. base }`) and stands for the impl's target. The target must be a struct; the literal then types exactly as one headed by the target's name. `Self` heads no literal outside an impl, in a trait default body (`Self` is the unknown implementor), or in a blanket impl (`Self` is a type parameter). A `Self` literal is also rejected where a local binding shadows the target's name — the literal lowers through that name (`DYNAMIC.md` §4.4), so the local would capture it.

```
Γ ⊢ impl target = S    struct S { … } ∈ Γ    S ∉ dom(Γ_locals)    Γ ⊢ S { f₁: e₁, … } : S
────────────────────────────────────────────────────────────────────────────── (T-STRUCT-LIT-SELF)
Γ ⊢ Self { f₁: e₁, … } : S
```

Diagnostics: `` `Self` is only available inside an impl ``; `` `Self` in a trait default body is not a struct ``; `` `Self` is the type parameter `T`, which is not a struct ``; `` `Self` is `E`, which is not a struct ``; `` `Self` names `S`, which a local binding shadows here; rename the local ``.

Field access `e.f` requires `e` to have a struct type declaring `f`:

```
Γ ⊢ e : S    struct S { …, f: τ, … } ∈ Γ
───────────────────────────────────────── (T-FIELD)
Γ ⊢ e.f : τ
```

A newtype `struct N(τ)` has one positional field accessed as `.0`; its constructor `N` has type `fn(τ) -> N`. These field types are enforced at runtime wherever a value can drift from them — §10.

A named field — a struct field or a named enum-variant field — may not begin with `__` (double underscore). Such names are reserved: a named field becomes a data key in the value's record, and the `__` prefix is held for the runtime's own structural keys, so a user field spelled that way would collide with them and silently corrupt the value. This restricts field identifiers only: type names, variant names, local bindings, and function parameters never form record keys and are unaffected by this rule, and positional (tuple) slots use single-underscore names the compiler assigns. Module-level names carry their own reservation (§4.3).

```
f begins with `__`
─────────────────── (T-FIELD-RESERVED)
struct/variant field f ill-formed
```

### 3.4 Traits and trait bounds

A trait names a set of method signatures. A type satisfies a trait when an `impl Trait for Type` provides every required method with matching signatures; a `#[derive(Trait)]` on the type is an implicit such impl. A trait bound `T: Trait` constrains a type parameter to types that satisfy `Trait`.

A trait may declare **associated types** (`type Item;`): named types each implementor binds (`type Item = number;`), referred to within the trait and its methods as `Self::Item`. Every declared associated type must be bound by every impl, and a binding must name a declared associated type. Inside an impl, `Self::Item` resolves to the impl's binding — so a method may write either the bound type or `Self::Item`, and a call site sees the bound concrete type. (Associated types are erased at runtime.)

A trait may declare **supertraits** (`trait Greet: Show`): traits its implementors must also implement. An `impl Greet for P` is incomplete unless `P` also implements `Show`. A supertrait grants its methods through the subtrait — a bound `T: Greet` resolves methods declared on `Show`, and a `Greet` value is usable wherever `Show` is expected (subsumption is transitive and one-directional: the subtrait flows into the supertrait, never the reverse). A type satisfies a trait when it implements that trait or any subtrait of it. The supertrait relation must be acyclic, and each named supertrait must be a trait.

Method resolution for `e.m(…)`: the static type of `e` determines which inherent or trait-impl method named `m` applies. When that static type is a **type parameter** `T`, `m` resolves against the methods of `T`'s declared bounds (the union of its `<T: …>` and `where` bounds); `Self` in the chosen signature is the parameter `T`. When that static type is a **trait used as a type** (below), `m` resolves against the trait's own methods, with `Self` read as the trait type. A method named on a parameter or trait type that does not provide it is an error.

**A trait name is usable directly as a type — this is the existential.** A parameter, field, list element, or return typed `Drawable` accepts a value of *any* type that satisfies `Drawable`; `[Drawable]` is a heterogeneous list of implementors. A list literal whose elements have differing concrete types infers its element type as the single trait every element implements; if the elements share no trait, or more than one, the list is ambiguous and must be annotated. There is no `dyn` keyword and no object-safety ceremony — a bare trait name *is* the existential. Assignability is directional: an implementor flows into the trait type, never the reverse (the existential is type-erasing — a trait-typed value is **not** assignable back to a concrete type; neoc has no implicit downcast). This is distinct from a generic `<T: Trait>`, which is type-*preserving*: `pick<T: Drawable>(xs: [T]) -> T` returns the concrete element type, whereas `pick(xs: [Drawable]) -> Drawable` returns the erased trait surface. Method calls on a trait-typed value dispatch dynamically at runtime; calls on a concrete or generic-parameter value resolve to a single statically known function.

### 3.5 Generics

Type parameters are universally quantified over the item that declares them. Instantiation infers type arguments — at a function call (`T-CALL` with `τᵢ` unified against argument types) and equally at the construction of a generic type (a struct literal, a tuple-struct or enum-variant constructor) — and checks each inferred argument against its bounds: the type bound to a parameter `T` must satisfy every trait in `T`'s bounds, or the instantiation is an error. When the argument is itself a type parameter of the enclosing item, it satisfies a bound exactly when its own declared bounds include that trait (bounds propagate; they are not re-derived).

**Blanket impls.** An `impl<T: Bound> Trait for T` (the target is the impl's own parameter) makes *every* type satisfying `Bound` satisfy `Trait` and gain its methods; satisfaction is transitive (a blanket condition may itself be met through another blanket). Inside the impl, the target `T` is an ordinary bounded parameter — `self.m()` resolves through `T`'s bounds exactly as in a `<T: Bound>` function. neoc has no general coherence solver; it rejects only the two detectable overlaps — two blanket impls of the same trait, and a blanket impl of a trait a concrete type also implements directly.

neoc uses Hindley–Milner inference restricted to the rank-1, no-subtyping fragment: inference flows from arguments and annotations; there is no variance, no implicit coercion (not even `number`-to-`string`), and no inference across function boundaries beyond the declared signature. A type annotation is required wherever this fragment cannot determine a type (empty list literals, ambiguous numeric contexts produced by generic returns).

### 3.6 `Result<T, E>` and `Option<T>`

These are ordinary library enums, predeclared in the prelude:

```
enum Result<T, E> { Ok(T), Err(E) }
enum Option<T>    { Some(T), None }
```

Their constructors `Ok`, `Err`, `Some`, `None` are in the value namespace unqualified. They are not primitive, but they are load-bearing: the `?` operator (§6) and `match` exhaustiveness (§7) both have rules that mention them by name.

Both carry **inherent methods** (`self`-receiver, snake_case), resolved on the concrete instantiation like any nominal method (§4.1) — `Option<number>::is_some`, `Result<string, E>::map`, etc.:

```
Option<T>:
  fn is_some(self) -> bool
  fn is_none(self) -> bool
  fn unwrap(self) -> T
  fn unwrap_or(self, default: T) -> T
  fn map<U>(self, f: fn(T) -> U) -> Option<U>
  fn ok_or<E>(self, err: E) -> Result<T, E>

Result<T, E>:
  fn is_ok(self) -> bool
  fn is_err(self) -> bool
  fn unwrap(self) -> T
  fn unwrap_err(self) -> E
  fn map<U>(self, f: fn(T) -> U) -> Result<U, E>
  fn map_err<F>(self, f: fn(E) -> F) -> Result<T, F>
```

`map`, `map_err`, and `ok_or` introduce a **method-level** type parameter (`U`, `F`, `E`) not bound by the receiver. The receiver's own arguments project as usual (`T`/`E` from the receiver type); the fresh parameter is left open and bound by unifying the declared parameter types against the argument types at the call site, so `map`'s `U` follows the closure's return type and `ok_or`'s `E` follows the supplied error. `unwrap`/`unwrap_err` are well-typed total projections statically (`-> T`/`-> E`); calling on the inert variant is a runtime abort (`DYNAMIC.md` §7), not a static error. Method-bearing dispatch is purely a runtime concern (`DYNAMIC.md` §3.2); it changes nothing about `?` (§6), `match` exhaustiveness (§7), derive, or equality.

### 3.7 `Unknown` — the dynamic boundary type

`Unknown` is the predeclared **dynamic** type: an honest "foreign value whose runtime type neoc cannot know", used at the host boundary (a host/FFI value reaching neoc through an `extern` declaration or a binding wrapper). It is the one type that opts out of static structure checking, so a binding author can describe a foreign value's shape through ordinary neoc declarations and assert concrete types at the surface.

`Unknown` is **bidirectionally assignable**: a value of any type τ is assignable to `Unknown`, and an `Unknown` is assignable to any type τ. Flowing a concrete value *into* `Unknown` loses nothing; flowing an `Unknown` *out* to a concrete type τ is a best-effort assertion — the binding author's promise that the foreign value really is a τ. (This is unlike the trait existential of §3.4, which is one-directional and type-erasing.) `Unknown` is erased at runtime: it has no representation of its own; it *is* the underlying host value.

A value of type `Unknown` supports the operations a static type would gate, each yielding `Unknown`:

```
Γ ⊢ e : Unknown                       Γ ⊢ e : Unknown   Γ ⊢ eᵢ : τᵢ  (any τᵢ)
──────────────────── (T-DYN-FIELD)    ──────────────────────────────────────── (T-DYN-METHOD)
Γ ⊢ e.f : Unknown                     Γ ⊢ e.m(e₁, …, eₙ) : Unknown

Γ ⊢ e : Unknown   Γ ⊢ i : τ  (any τ)  Γ ⊢ e : Unknown   Γ ⊢ eᵢ : τᵢ  (any τᵢ)
───────────────────────────── (T-DYN-INDEX)   ──────────────────────────────── (T-DYN-CALL)
Γ ⊢ e[i] : Unknown                     Γ ⊢ e(e₁, …, eₙ) : Unknown
```

Member assignment to an `Unknown` is likewise unrestricted: in `e.f = v` and `e[i] = v` where `e : Unknown`, the assigned value `v` may be of any type. No member of an `Unknown` is statically resolved (there is nothing to resolve to), so field/method/index access on it never reports a "no field"/"no method"/"not a struct" error.

`Unknown` is **confined**: this permissiveness applies only where the `Unknown` type itself appears. A genuine mismatch on a concrete type elsewhere in the same program still reports normally — `Unknown` never widens unification for any other type.

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
- Compound assignment `p op= e` (`op` one of `+ - * / %`) requires `p op e` to be well-typed with the type of `p`, so `+=` is arithmetic on `number` and concatenation on `string`. It means `p = p op e` with the place `p` evaluated **once**: the subexpressions of `p` — a field base, an indexed list, an index, any `?` among them — run a single time, before `e`, and the read and the write address the same place.

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

Each item form extends the environment with its name and type. A `fn` adds its name with its function type; a `struct`/`enum` adds its type and its constructors; an `impl` adds methods to an existing type; a `const`/`let` adds a binding; a `use` adds the imported name; an `extern` block adds the declared bindings (§5).

```
Γ ⊢ τ ok    Γ, params ⊢ body : τ_ret
──────────────────────────────────────────────────────── (D-FN)
Γ ⊢ fn f(p₁: τ₁, …) -> τ_ret { body } ⇒ Γ, f : fn(τ₁, …) -> τ_ret
```

The function body must have the declared return type. A function with no `-> τ` clause returns `()`, and its body must have type `()`.

A module-level name — a `fn`, `const`, `struct`, `enum`, `trait`, or type alias; the name a `use` binds (its alias, else the path's last segment); or a `fn`, `struct`, or `mod` declared at the top level of an `extern` block — may not begin with `__` (double underscore). Each is a bare name at module scope in the lowering, and the `__` prefix is held for helpers the compiler emits at module scope (e.g. the derive helpers, `DYNAMIC.md` §6); a user name spelled that way would shadow a helper, or resolve to one in place of the host value, and silently change behaviour. Local bindings, parameters, and `extern` items nested in a `mod` (reached as `m.f`, never bare) are unaffected.

```
name x bound at module scope    x begins with `__`
────────────────────────────────────────────────── (D-NAME-RESERVED)
binding of x ill-formed
```

A `type` alias is transparent: every use expands to its value (`SYNTAX.md` §3.3). A use of a generic alias supplies exactly the type arguments the alias declares. A type parameter in scope shadows an alias of the same name, and a `Self::` projection names an associated type, so neither is a use of the alias. An ill-formed use is reported once, at the annotation that spells it; the annotation then checks as no type, so nothing downstream cascades.

```
type A<α₁, …, αₙ> = τ ∈ Γ    use A<τ₁, …, τₘ>    m ≠ n
──────────────────────────────────────────────────────── (D-ALIAS-ARITY)
use ill-formed
```

An alias may not reach itself through its own value, directly or through other aliases — its expansion would never end. Each alias on the cycle is ill-formed, reported at its declaration.

```
type A<…> = τ ∈ Γ    τ mentions A, directly or via aliases
──────────────────────────────────────────────────────── (D-ALIAS-RECURSIVE)
declaration of A ill-formed
```

## 5. `extern` declarations

An `extern { … }` block introduces bindings the target supplies, with no bodies. It describes the target's surface — see `EXTERN.md` for the authoritative model (items, attributes, lowering, errors). The well-formedness rules:

```
Γ ⊢ τᵢ ok   (for each i)   Γ ⊢ τ_ret ok
──────────────────────────────────────────────────────── (D-EXT-FN)
Γ ⊢ extern { fn f(p₁: τ₁, …) -> τ_ret ; } ⇒ Γ, f : fn(τ₁, …) -> τ_ret

──────────────────────────────────────── (D-EXT-STRUCT-OPAQUE)
Γ ⊢ extern { struct T ; } ⇒ Γ, T : opaque type

Γ ⊢ τᵢ ok   (for each field i)
──────────────────────────────────────────────────────── (D-EXT-STRUCT-FIELDS)
Γ ⊢ extern { struct T { f₁: τ₁, … } } ⇒ Γ, T : struct { f₁: τ₁, … }
```

An opaque extern `struct T ;` (no readable fields) is a pure handle: neoc knows the name and can pass values of it around, but cannot inspect them — only extern `fn`s/methods mentioning the type produce or consume its values. An extern `struct` with fields exposes those as the target value's data/callback properties (a field read `x.f`, a struct literal `T { f: v }`). An extern `impl` adds methods/associated functions; an `#[extern(constructor)]` associated `fn` yields the enclosing type. A `mod` adds a target-module scope reached by its path.

`extern` is the only construct that adds a binding without a corresponding definition in neoc source. Everything callable from neoc that is not defined in neoc has an `extern` declaration somewhere.

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

A `match` must cover every value of the scrutinee's type. Exhaustiveness is checked structurally, and recursively into a constructor's fields:

- Scrutinee of an `enum`: every variant must be covered, either by a matching variant pattern or by a wildcard/binding pattern. A `match` over `Result<T, E>` is exhaustive iff it covers both `Ok` and `Err` (or has a catch-all); likewise `Some`/`None` for `Option`. A variant pattern covers the variant only when its sub-patterns cover the variant's fields: `match o { Some(true) => …, None => … }` is *not* exhaustive — `Some(false)` is uncovered.
- Scrutinee of `bool`: both `true` and `false`, or a catch-all.
- Scrutinee of a `struct`: a single irrefutable pattern (every field bound by a wildcard or binding) covers it; a refutable sub-pattern on a field leaves a gap.
- Scrutinee of `number`, `string`, or any type with unbounded values: a binding or wildcard arm is required, since the literals can never be exhaustive alone.

A guard makes an arm conditional, so a guarded arm does not contribute to coverage — the value it matches may still fall through. A non-exhaustive `match` is a static error — never a runtime failure. Redundant arms (a pattern that can never match because earlier arms already cover it) are a static error.

## 8. Trait-impl completeness

An `impl Trait for Type` must provide every required method of `Trait` (those without a default body) with a signature matching the trait's, after substituting `Self` with `Type`. A missing required method is a static error. Providing a method not declared by the trait is a static error. A default-bodied trait method may be overridden or omitted. An omitted default — receiver method or associated function — is inherited: `x.m()` and `Type::m()` resolve to it, with `Self` in its signature read as `Type`. A default body is checked once and shared by every implementor, so it may not name `Self` in a path (`Self::f()`, `Self::new()`) — a static error; it reaches the implementor only through `self`.

A type has one method table, so each method name on it must come from exactly one place. An inherited default whose name the type already has — from an inherent impl, another trait impl's method, or another implemented trait's inherited default — is a static error at the inheriting impl. Override the default in the impl to resolve it.

## 9. Attribute well-formedness

Each built-in attribute constrains what it may attach to:

- `#[derive(…)]` — only on a `struct` or `enum`. Each derive name must be one of the built-in derivable traits (`Clone`, `Eq`, `Debug`, `Display`; `DYNAMIC.md` §6). Deriving `Eq`, `Debug`, or `Display` requires every field's type to satisfy that trait. A user `struct`/`enum` satisfies it by deriving or implementing it, and only when its type arguments do too; `[T]`, `Option<T>`, and `Result<T, E>` satisfy `Eq` and `Debug` when their element or payload types do, and never satisfy `Display` — they have no user-facing form.
- `#[test]` — only on a `fn` taking no parameters and returning `()` or `Result<(), E>`.
- Field-validation attributes — only on a struct field, and only when the field's type matches the constraint (`#[min(_)]` / `#[max(_)]` on `number`, `#[max_len(_)]` on `string` or `[T]`).

## 10. Drift-point analysis

Runtime type enforcement (`DYNAMIC.md` §9) emits a check only where a runtime value of a declared type can diverge from that type. This analysis classifies each value position as **drift** — a check is required — or **stable** — the type system already guarantees the shape, so a check would be dead code. The classification is the contract this file hands to `DYNAMIC.md` §9: a drift position lowers through its type's validator; a stable position lowers unchanged.

The judgements, in the predicate style of `Γ ⊢ τ ok`:

- `Γ ⊢ π drift` — value position `π` may carry a value whose runtime shape is not statically guaranteed; check it against its declared type.
- `Γ ⊢ π stable` — `π` cannot drift; emit no check.

### 10.1 Provenance

A value is **vouched** when the type system alone guarantees its runtime shape matches its static type, and **unvouched** otherwise. Every value is born vouched — a literal, or a value built from vouched parts — or enters unvouched across the host boundary (`extern`, decode, entry). A check converts unvouched to vouched.

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
f : extern fn(…) -> τ ∈ Γ
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
- **`STABLE-READ`** — reading an immutable binding cannot reintroduce drift; it was vouched when introduced and cannot have changed. The immutable bindings are `const` and parameters never reassigned in scope; a `let` is reassignable, so a read through it does not qualify.

### 10.4 Host-reachability

A function is **host-reachable** iff it is `pub` (lowered to a binding shared with the host, `DYNAMIC.md` §2) or is the designated runtime entry. Every other function is **internal-only**: all its call sites are neoc with statically-typed arguments. Host-reachability is what separates `DRIFT-PARAM` from `STABLE-PARAM` — the analysis must compute it before classifying parameters.

### 10.5 Depth and limits

- **Deep.** A drift check recurses through the full declared shape — nested structs, list elements, `Option`/`Result` payloads, enum-variant fields — so divergence at any depth is caught.
- **Opaque extern `struct`** (§5) has no shape neoc can inspect; its validator is the identity — it returns the value unexamined (`DYNAMIC.md` §9.1) — and recursion stops at the opaque boundary.
- **`fn(…) -> _`** is checked as callable; a signature is not runtime-observable.
- **Bare type parameter** with no concrete witness at the position is checked only to the depth its static type reveals; the deep check lands where the parameter is instantiated to a concrete type — itself a `DRIFT-CONSTRUCT` or `DRIFT-INGRESS` position.

### 10.6 Always-on

Drift checks are unconditional — no debug/release split, no opt-out. A well-typed program's checks fire identically in every build. Conversely, a stable position emits no check at all, so "always-on" never means "everywhere": it means every *drift* position is always checked. Stable is a pass-through property only (§10.1, §10.3); a typed write — construction, mutation — is never stable, so the always-on guarantee covers every commit into typed storage.
