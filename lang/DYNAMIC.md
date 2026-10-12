# Dynamic semantics

Defines what every meaningful neoc program *does* when executed, expressed as a translation to Lua 5.1. neoc is a transpiler; its dynamic semantics are the dynamic semantics of Lua 5.1 applied to the lowering of each construct.

This file is a **lowering table**: one entry per neoc construct, giving the Lua 5.1 source it produces. Once a program is lowered, its runtime behaviour is whatever Lua 5.1 would do with that source — see the Lua 5.1 Reference Manual for the underlying semantics.

> **Note on targets.** `lua` is the only build target. It emits the Lua 5.1 lowering described here, which LuaJIT runs unchanged; read it as the meaning each construct carries.

Notation: lowering brackets `⟦ ⟧` as defined in `NOTATION.md`. Metavariables in Lua snippets are `⟪ ⟫`. Statement-context lowerings are subscripted `⟦ e ⟧ₛ`; expression-context `⟦ e ⟧ₑ`.

---

## 1. Notation

Defined in `NOTATION.md`. Summary: `⟦ neoc ⟧ = lua`; `⟦ e ⟧` recurses; `⟪ x ⟫` is a metavariable; subscripts `ₛ`/`ₑ` distinguish statement and expression context.

The fundamental tension this file resolves: neoc is expression-oriented (`if`, `match`, and blocks have values), Lua 5.1 is statement-oriented (`if`, blocks do not). The bridge is the **immediately-invoked function expression** (IIFE): an expression-position construct that Lua can't express as an expression is wrapped in `(function() … end)()`. Constructs in statement position skip the wrapper.

## 2. Source unit

A `.neoc` file lowers to a single Lua chunk:

1. If the module references `Result` or `Option`, the corresponding prelude (§7) is emitted first.
2. Each item is lowered in source order.
3. A `pub` item lowers to a global; a private item lowers to a `local`. mlua-hosted scripts share globals with the host, which is the interop path the embedding runtime relies on.

```
⟦ pub fn f(…) { … } ⟧   = function f(…) … end
⟦ fn f(…) { … } ⟧       = local function f(…) … end
```

A `fn` declared as a statement inside a block (`STATIC.md` §2) is hoisted to the head of that block, ahead of every other statement, so it is in scope from the block's first line. One such `fn` lowers to a `local function`, which can call itself. Several are declared together first and assigned after, so siblings can call one another:

```
⟦ { fn f(…) { … } … } ⟧           = local function f(…) … end …
⟦ { fn f(…) { … } fn g(…) { … } … } ⟧ = local f, g
                                      function f(…) … end
                                      function g(…) … end  …
```

## 3. Items

### 3.1 `struct`

A struct value is a Lua table tagged with its type name. The type's own table doubles as the method table and the shared metatable: `S.__index = S` makes instances find their methods (§3.3), and the constructor stamps each new value with `setmetatable`. Named struct:

```
⟦ struct S { f₁: τ₁, …, fₙ: τₙ } ⟧ =
  local S = {}
  S.__index = S
  function S.new(fields)
    fields.__struct = "S"
    return setmetatable(fields, S)
  end
```

`S.new` is an internal constructor; `S { … }` literals (§4) call it. The `__struct` tag is the value's runtime identity, read by `match` (§4.5) and `Eq`/`Debug` (§6); the metatable carries dispatch (§4.7).

- **Unit struct** `struct S;` lowers to a single shared value `local S = { __struct = "S" }`. Methods (§3.3) attach to `S` itself, so `S:m(…)` resolves directly with no separate metatable.
- **Newtype** `struct N(τ)` lowers like a named struct with one field stored at key `[1]`; `N(x)` constructs `setmetatable({ __struct = "N", [1] = x }, N)` and `.0` reads `[1]`.

Methods are not emitted here; they come from `impl` (§3.3).

### 3.2 `enum`

An enum value is a tagged table carrying its variant name and payload:

```
⟦ enum E { … } ⟧ = (* no standalone output for a plain enum; constructors are
                      inlined at use sites *)
```

A variant constructor lowers at each use site (§4.4):

- **Unit variant** `E::V` → `{ __enum = "E", __tag = "V" }`
- **Tuple variant** `E::V(a, b)` → `{ __enum = "E", __tag = "V", ⟦a⟧, ⟦b⟧ }` (payload at `[1]`, `[2]`, …)
- **Struct variant** `E::V { x: a }` → `{ __enum = "E", __tag = "V", x = ⟦a⟧ }`

`__tag` is read by `match`; `__enum` guards against matching a same-tagged variant of a different enum.

**Enums with methods.** When an enum has any `impl` block or `#[derive(...)]`, it emits a method table doubling as the shared metatable — exactly like a struct (§3.1):

```
⟦ enum E { … } with methods ⟧ =
  local E = {}
  E.__index = E
```

Every variant construction is then `setmetatable`d onto `E`, so a value finds its methods through the metatable and a method call lowers to a Lua colon call (§3.3, §4.7):

- **Unit variant** `E::V` → `setmetatable({ __enum = "E", __tag = "V" }, E)`
- **Tuple variant** `E::V(a, b)` → `setmetatable({ __enum = "E", __tag = "V", ⟦a⟧, ⟦b⟧ }, E)`
- **Struct variant** `E::V { x: a }` → `setmetatable({ __enum = "E", __tag = "V", x = ⟦a⟧ }, E)`

`impl` methods (§3.3) and derived methods (§6) attach to `E` as for a struct. The metatable changes nothing `match` or `?` rely on — they read `.__tag`/`.__enum`, which a metatable does not shadow. A plain enum with no methods keeps the bare tagged table (no metatable). The `Result`/`Option` prelude enums are themselves method-bearing — they carry the inherent operations of §7 — and take this same metatable shape.

### 3.2a The `__traits` tag

Alongside `__struct`/`__enum`, a type that implements one or more traits carries `__traits`: the names of those traits, transitively closed over supertraits (a type implementing `Greet` where `trait Greet: Show` lists both). A `#[derive(T)]` counts as an `impl T`. The set is computed from the impls present in the lowered module, so a blanket impl (§3.5) that expanded to a concrete `impl` is included. Type-keyed and sorted, so the emission is deterministic.

`Type.__traits = { "A", "B" }` lands on the type's table. For a struct or method-bearing enum that table is the metatable, so `value.__traits` resolves through `__index`; for a unit struct the table *is* the value.

A type implementing no trait carries no entry: a reader treats the absence (`nil`) as the empty set. The tag is inert metadata — nothing in the current lowering reads it. It is the runtime identity a future trait-enforcement check keys on (the nominal assertion `value` *has* trait `T`).

### 3.3 `impl`

Inherent and trait-impl methods attach to the target type's table as plain functions taking `self` first:

```
⟦ impl S { fn m(self, p) -> τ { body } } ⟧ =
  function S.m(self, p) ⟦ body ⟧ₛ end
```

Because the constructor sets `S.__index = S` and `setmetatable`s each instance (§3.1), a value of type `S` finds `m` through its metatable. A method call `x.m(a)` lowers to the Lua colon call `x:m(a)`, which passes the receiver as `self` (§4.7) — dispatch happens at runtime via the metatable, so the emitter needs no type information at the call site. An associated function (no `self`) is still a plain entry on the table and is called as `S.f(a)`.

### 3.4 `trait`

A trait without default-bodied methods produces no output; its required methods are realised by each `impl Trait for Type`. A trait with defaults emits a table holding them, bound like any other item (`local`, or exported when `pub`), so a `use` of the trait reaches it (§3.7). An `impl Trait for Type` that omits a default assigns the trait's function onto the type's table:

```
⟦ trait Tr { fn m(self) { body } } ⟧ = local Tr = {}
                                        function Tr.m(self) ⟦ body ⟧ₛ end
⟦ impl Tr for S {} ⟧                  = S.m = Tr.m
```

The body is emitted once, in the trait's module, and shared by every implementor; it never names `Self` at runtime. When the impl precedes the trait's binding in the module, the assignment is emitted right after that binding. A blanket impl (§3.5) is expanded to a concrete `impl` for each implementing type before lowering, so its methods land on that type's table like any other impl — no runtime trait machinery is involved.

### 3.5 `const`

```
⟦ const C: τ = e ⟧ₛ = local C = ⟦ e ⟧ₑ      (pub: global C = ⟦ e ⟧ₑ)
```

### 3.6 `let`

```
⟦ let x = e ⟧ₛ        = local x = ⟦ e ⟧ₑ
⟦ let p = e ⟧ₛ        = (destructuring; see §5)
```

### 3.7 `use`

```
⟦ use a::b::c ⟧       = local c = require("a.b").c
⟦ use a::b::c as d ⟧  = local d = require("a.b").c
```

The path up to the final segment is the module; the final segment is the item selected from it. `require` uses the dotted module convention. A `use` of a method-bearing type needs no per-type wiring in the consumer: the value carries its metatable across the boundary (§7).

**Type import shadowed by a value.** A `use` of a type-only item (a struct with no value-namespace binding, `STATIC.md` §2) may share its binding with a value the module also binds — a value `use`, or a module-level `const` / `fn`. The two coexist statically, but the struct's constructor is still a runtime table, so the type import binds a fresh local `t` instead: `__type_<binding>`, suffixed `_1`, `_2`, … until no identifier in the module's source spells it and no fresh local already allocated in the module takes it, so no user binding can capture it and no two shadowed types share one. The value keeps the plain name. Struct literal, struct update, and `S::assoc` references to the type read `t`; every other reference to the name reads the value.

```
⟦ use a::b::c ⟧          = local t = require("a.b").c     when c is type-only and the module's value namespace binds c
⟦ c { f: e, … } ⟧ₑ       = t.new({ f = ⟦e⟧, … })
⟦ c::m ⟧ₑ                = t.m
```

### 3.8 `type` alias

No output. Type aliases are erased; they exist only in `STATIC.md`.

### 3.9 `extern`

An `extern { … }` block declares the target's surface (`EXTERN.md` is the authoritative lowering reference). The neoc value of an extern type **is** the target value — there is no wrapper. The block's own declarations (`struct`, `impl`, bodyless `fn`) emit no code; only a `mod` emits an import preamble, and references lower directly.

**Top-level extern `fn` (host global).** A bodyless `fn` declared directly in the block produces no output; the binding is expected in the runtime's environment, and a call lowers to a bare call.

**`mod` import.** A `mod`'s target path is its name (nested joined by `/`), or its `#[extern(path = "…")]`. It lowers to `require`. A `mod::item` reference reads the imported handle. An `#[extern(ambient)]` `mod` emits **no** import — its path is a member-access chain rooted at the target global.

```
⟦ extern { #[extern(path="vnd:rusqlite")] mod m { fn open(…) -> τ; } } ⟧
  = local m = require("vnd:rusqlite")          -- preamble
⟦ m::open(a…) ⟧ₑ  = m.open(⟦a⟧…)

⟦ #[extern(ambient, path="globalThis.crypto")] mod c { … } ⟧  = (no import)
⟦ c::digest(a…) ⟧ₑ  = globalThis.crypto.digest(⟦a⟧…)
```

**Field, method, struct literal, constructor.** Directly to the target value:

```
⟦ x.f ⟧ₑ       = x.f                        (field read)
⟦ x.f = v ⟧ₛ   = x.f = ⟦v⟧                  (field write)
⟦ x.m(a…) ⟧ₑ     = x:m(⟦a⟧…)                (Lua colon call — UserData convention)
⟦ T { f: v } ⟧ₑ  = { f = ⟦v⟧ }              — bare target object, no tag
⟦ T::f(a…) ⟧ₑ    = T.f(⟦a⟧…)                (associated / static call)
⟦ T::new(a…) ⟧ₑ  = T.new(⟦a⟧…)              — #[extern(constructor)]
```

An `#[extern(name = "…")]` overrides the target identifier of an `fn`/`struct`; the neoc-side name is unaffected.

**`Option<T>` at the boundary.** An argument for an `Option<T>` parameter or field is unwrapped to `nil` / its payload, and an `Option<T>` result or field read is wrapped from `nil` / the value (`EXTERN.md` §Option marshalling). The two conversions are module-local helpers, emitted only in a module that marshals, the wrapping one reading the `Some`/`None` of the prelude (§7):

```
local function __option_to_host(v)       -- None → nil, Some(v) → v
  if v.__tag == "Some" then return v[1] end
  return nil
end
local function __option_from_host(v)     -- nil → None, v → Some(v)
  if v == nil then return None end
  return Some(v)
end
```

**Cross-file `use` of an extern item.** A `pub` extern item `use`d from another file lowers to its **native** import in the *consumer*, not a `require` of the defining file — an extern emits no runtime value there (§3.7 does not apply). The consumer reconstructs the native import and the construction/method/associated lowering above from the imported item's `#[extern(…)]` metadata, identical to how the defining module would. `EXTERN.md` §Cross-file `use` is the authoritative reference.

```
⟦ use db::T ⟧            where  #[extern(path="p")] mod m { struct T; … }
  = local m = require("p")     -- preamble, in the consumer
  (no require of "db")
```

## 4. Expressions

### 4.1 Literals and paths

Number, string, and bool literals lower to their Lua spelling (`STRING_LITERAL` escapes are re-encoded to Lua string escapes). `()` lowers to `nil` in value position. A path to a value (`x`, `module::f`) lowers to the resolved Lua name.

### 4.2 Operators

```
⟦ e₁ + e₂ ⟧ₑ  = (⟦e₁⟧ + ⟦e₂⟧)     when e₁ : number
⟦ e₁ + e₂ ⟧ₑ  = (⟦e₁⟧ .. ⟦e₂⟧)    when e₁ : string
⟦ e₁ - e₂ ⟧ₑ  = (⟦e₁⟧ - ⟦e₂⟧)      (likewise * / %)
⟦ e₁ == e₂ ⟧ₑ = (⟦e₁⟧ == ⟦e₂⟧)     for number/string/bool; structural for Eq types (§6.2)
⟦ e₁ != e₂ ⟧ₑ = (⟦e₁⟧ ~= ⟦e₂⟧)
⟦ e₁ && e₂ ⟧ₑ = (⟦e₁⟧ and ⟦e₂⟧)
⟦ e₁ || e₂ ⟧ₑ = (⟦e₁⟧ or ⟦e₂⟧)
⟦ !e ⟧ₑ       = (not ⟦e⟧)
⟦ -e ⟧ₑ       = (-⟦e⟧)
```

`+` selects Lua `+` or `..` by the static type of its operands (`STATIC.md` T-ARITH / T-CONCAT) — the choice is fixed at compile time, never a runtime check. `!=` lowers to Lua `~=`. Compound assignment `p op= e` (`STATIC.md` §4.1) lowers to `⟦p⟧ = ⟦p⟧ op ⟦e⟧`, with `..` for `op` when `p` is a `string` (`s += e` → `s = (s .. ⟦e⟧)`). A field base, indexed list, or index of `p` that is not a name or a literal is bound once — a field read included, since an `extern` struct's field may be a host getter — to a fresh local first, inside a `do … end`, so `p` is evaluated once:

```
⟦ b[i] op= e ⟧ₛ = do local ⟪b⟫ = ⟦b⟧ₑ ; local ⟪i⟫ = ⟦i⟧ₑ ; ⟪b⟫[⟪i⟫ + 1] = ⟪b⟫[⟪i⟫ + 1] op ⟦e⟧ₑ end
⟦ b.f op= e ⟧ₛ  = do local ⟪b⟫ = ⟦b⟧ₑ ; ⟪b⟫.f = ⟪b⟫.f op ⟦e⟧ₑ end
```

### 4.3 Range expressions

`a..b` and `a..=b` lower to a small range value the prelude understands; in a `for` head (§4.9) they lower directly to numeric `for` bounds without allocating.

### 4.4 Struct, enum, and list construction

```
⟦ S { f: e, … } ⟧ₑ      = S.new({ f = ⟦e⟧, … })
⟦ S { f, .. base } ⟧ₑ   = S.new({ f = f, <copied fields from ⟦base⟧> })
⟦ Self { … } ⟧ₑ         = ⟦ S { … } ⟧ₑ   (inside `impl S`; also with `.. base` — STATIC.md §3.3)
⟦ N(a) ⟧ₑ               = N.new(⟦a⟧)     (tuple/newtype struct: a call constructs through .new)
⟦ E::V ⟧ₑ               = { __enum = "E", __tag = "V" }
⟦ E::V(a, …) ⟧ₑ         = { __enum = "E", __tag = "V", ⟦a⟧, … }
⟦ E::V { x: a, … } ⟧ₑ   = { __enum = "E", __tag = "V", x = ⟦a⟧, … }
⟦ [e₁, …] ⟧ₑ            = { ⟦e₁⟧, … }     (a sequence: a 1-based Lua table — §4.7, §7)
```

Prelude constructors `Ok`/`Err`/`Some`/`None` follow §7. Each component is validated against its declared field, slot, or element type at construction — a drift point (§9.3).

A list literal `[e₁, …]` builds a sequence value, evaluating its elements left to right. neoc indexes from 0 and a Lua table is 1-based; the offset surfaces at indexing (§4.7), not construction. Each `eᵢ` is validated against the list's element type `[τ]` at construction, a drift point (§9.3); reads (§4.7) and element writes `e[i] = v` (§7) then operate through the same offset convention.

### 4.5 `match`

`match` in expression position lowers to an IIFE that tests arms in order and returns the first match's body. Pattern tests and bindings come from §5.

```
⟦ match s { p₁ [if g₁] => e₁, … } ⟧ₑ =
  (function()
    local __s = ⟦s⟧
    if <test p₁ against __s> [and ⟦g₁⟧] then <bind p₁> return ⟦e₁⟧ₑ end
    …
  end)()
```

The scrutinee is evaluated exactly once (bound to `__s`). Exhaustiveness is guaranteed statically (`STATIC.md` §7), so the generated chain needs no fallback error arm. In statement position the wrapping IIFE and `return`s are dropped and each body lowers with `⟦ ⟧ₛ`.

### 4.6 `if`

```
⟦ if c { a } else { b } ⟧ₑ =
  (function() if ⟦c⟧ then return ⟦a⟧ₑ else return ⟦b⟧ₑ end end)()

⟦ if c { a } ⟧ₛ            = if ⟦c⟧ then ⟦a⟧ₛ end
⟦ if c { a } else { b } ⟧ₛ = if ⟦c⟧ then ⟦a⟧ₛ else ⟦b⟧ₛ end
```

In statement position no IIFE is needed; in expression position the IIFE carries the value out.

### 4.7 Field access, method calls, indexing

```
⟦ e.f ⟧ₑ        = ⟦e⟧.f
⟦ e.0 ⟧ₑ        = ⟦e⟧[1]                  (newtype field)
⟦ e.m(a, …) ⟧ₑ  = ⟦e⟧:m(⟦a⟧, …)           (Lua colon call: receiver passed as self)
⟦ f(a, …) ⟧ₑ    = f(⟦a⟧, …)
⟦ e[i] ⟧ₑ       = ⟦e⟧[⟦i⟧ + 1]            (Lua lists are 1-indexed)
```

Method dispatch goes through the receiver's metatable. The colon call `⟦e⟧:m(…)` looks `m` up on the value's metatable (set by its constructor, §3.1) and passes the receiver as the implicit `self`, so the call site needs no static knowledge of the receiver's type. A foreign (`extern`) receiver resolves `m` the same way, so neoc and host receivers are uniform. List indexing adjusts neoc's 0-based index to Lua's 1-based tables.

### 4.8 Block expressions

```
⟦ { s₁; …; sₙ; e } ⟧ₑ = (function() ⟦s₁⟧ₛ … ⟦sₙ⟧ₛ return ⟦e⟧ₑ end)()
⟦ { s₁; …; sₙ } ⟧ₛ    = do ⟦s₁⟧ₛ … ⟦sₙ⟧ₛ end
```

A value-producing block is an IIFE; a statement block is a Lua `do … end`.

### 4.9 Closures, loops, control flow

```
⟦ |x, y| e ⟧ₑ            = function(x, y) return ⟦e⟧ₑ end
⟦ |x| { … } ⟧ₑ          = function(x) ⟦…⟧ₛ end
⟦ for x in a..b { … } ⟧ₛ = for x = ⟦a⟧, ⟦b⟧ - 1 do ⟦…⟧ₛ end      (..= omits the -1)
⟦ for x in xs { … } ⟧ₛ   = for _, x in ipairs(⟦xs⟧) do ⟦…⟧ₛ end
⟦ while c { … } ⟧ₛ       = while ⟦c⟧ do ⟦…⟧ₛ end
⟦ loop { … } ⟧ₛ          = while true do ⟦…⟧ₛ end
⟦ break ⟧ₛ               = break
⟦ continue ⟧ₛ            = (Lua 5.1 has no continue; lowered via a per-loop goto-free guard — see §8)
⟦ return e ⟧ₛ            = return ⟦e⟧ₑ
```

### 4.10 The `?` operator

`e?` must return the carrier from the **enclosing function** on the short-circuit, so the guard cannot live in an IIFE — an IIFE's `return` would only exit the wrapper. The guard is hoisted as statements into the surrounding body and the operator's value is the unwrapped payload:

```
⟦ e? ⟧ₑ  hoists, before the statement that uses the value:
  local ⟪t⟫ = ⟦e⟧
  if ⟪t⟫.__tag == "Err" or ⟪t⟫.__tag == "None" then return ⟪t⟫ end
and yields the expression  ⟪t⟫[1]
```

`⟪t⟫` is a fresh temporary. On the short-circuiting variant (`Err` for `Result`, `None` for `Option`) the hoisted `return ⟪t⟫` exits the enclosing function with the carrier unchanged; otherwise the expression evaluates to the wrapped value at `[1]`. Testing both tags lets the lowering serve `Result` and `Option` alike without the carrier's static type — a value carries exactly one tag, so only the matching test can fire. The enclosing function's return type makes this well-formed (`STATIC.md` §6).

### 4.11 Loop-carried string appends

Lua interns every string, so `s = s .. x` in a loop hashes and copies the whole intermediate result on every pass. A loop that only *appends* to a local string therefore lowers the appends to pushes on an accumulator table and folds it with one `table.concat` after the loop. This is an optimisation: the program's observable behaviour is that of the plain `..` lowering (§4.2), and any shape outside the rule below keeps that lowering.

An *append* to `s` is an assignment `s = s + e₁ + … + eₙ` or `s += e` where `+` is string concatenation (`STATIC.md` T-CONCAT) and `s` is the leftmost operand. The rule applies to a loop `L` (`for`, `while`, `loop`) and a variable `s` when all of these hold:

1. `s` is introduced by a `let` with an initialiser, as a simple binding, and `L` is in that `let`'s scope (in the same block after the `let`, or nested in a later statement).
2. Within `L` — head, condition, and body — every occurrence of `s` is an append target, or the whole operand of a `return`. An occurrence anywhere else (a read, a prepend `s = e + s`, another assignment, an operand of an appended `eᵢ`, a struct-literal shorthand `S { s }`) disables the rule.
3. `L` contains at least one append to `s`, none of them inside a closure or nested function, and binds no name `s` of its own (`let`, pattern, closure parameter, nested function).
4. No closure or nested function anywhere in `s`'s scope mentions `s`: such a function could read the string while `L` still holds it in the accumulator.
5. `L` is not inside a block, `if`, or `match` used for its value. Those lower to an IIFE (§4.5, §4.6, §4.8), where a `return` — or the one `?` hoists (§4.10) — leaves only the IIFE and would skip the fold after `L`.

Where several nested loops satisfy the rule, only the outermost is rewritten; its appends include those of the inner loops. A loop that fails the rule may still contain a nested loop that satisfies it.

```
⟦ L ⟧ₛ   with s accumulating =
  do
    local ⟪acc⟫ = { (s .. "") }
    ⟦ L ⟧ₛ                     -- appends and returns lowered as below
    s = table.concat(⟪acc⟫)
  end

⟦ s = s + e₁ + … + eₙ ⟧ₛ  inside L = ⟪acc⟫[#⟪acc⟫ + 1] = ⟪e₁⟫  …  ⟪acc⟫[#⟪acc⟫ + 1] = ⟪eₙ⟫
⟦ s += e ⟧ₛ               inside L = ⟪acc⟫[#⟪acc⟫ + 1] = ⟪e⟫

⟪e⟫ = ⟦e⟧ₑ             when e is a string literal
⟪e⟫ = (⟦e⟧ₑ .. "")      otherwise
⟦ return s ⟧ₛ             inside L = return table.concat(⟪acc⟫)
```

`⟪acc⟫` is a fresh local. The accumulator is seeded with the string's current value, so a prior value is kept and a loop that never runs leaves `s` unchanged. The fold runs after the loop however it was left: a `break` only leaves the loop, so the folded string holds exactly the appends made before it; a `return` of any other value leaves the function, and `s` is then unreachable (condition 4). `eᵢ` is evaluated and pushed in order, so evaluation order is that of the chained `..`. A well-typed pushed value is a `string`, and `table.concat` joins strings exactly as `..` does. A host can still break an `ext fn`'s `string` contract; `.. ""` then raises at the append, as plain `..` does, where `table.concat` would skip a `nil` or fail only at the fold. For a string, Lua returns the left operand of `.. ""` unchanged. The seed is checked the same way, so a non-string `s` raises on entry to `L`.

## 5. Patterns

Patterns lower only inside `match` (§4.5) and `let` destructuring. Each pattern compiles to a **test** (a boolean Lua expression over the scrutinee) and a set of **bindings** (locals introduced when the test passes).

```
_              test: true                          bind: —
x              test: true                          bind: local x = __s
42 / "s" / true test: __s == 42                    bind: —
S { f: p }     test: __s.__struct == "S" and <test p over __s.f>   bind: <p over __s.f>
S { f: p, .. } test: as S { f: p }; the .. rest adds no test       bind: <p over __s.f>; the .. binds nothing
E::V           test: __s.__tag == "V"                bind: —
E::V(p₁, …)    test: __s.__tag == "V" and <tests>    bind: <pᵢ over __s[i]>
p₁ | p₂        test: <test p₁> or <test p₂>        bind: shared names
```

A guard `if g` is appended to the arm's test with `and ⟦g⟧`, evaluated only after the pattern test passes so guard code sees the pattern's bindings.

A struct pattern may end in a `..` rest (`S { f: p, .. }`), matching every field the pattern does not name. The rest lowers to nothing — no test, no binding — so such a pattern tests and binds exactly its named fields, just as `S { f: p }` does (`SYNTAX.md` §7 gives the grammar).

## 6. Built-in attribute derives

A derive attaches a method to the type's table, the same slot an `impl` method uses (§3.3).

### 6.1 `#[derive(Clone)]`

Emits a `clone` returning a shallow copy carrying the same `__struct`/`__enum` tag (`function S.clone(self)`), re-setting the metatable on the new value — for a struct and for an enum variant alike, so the clone keeps its methods and `__traits`. Fields are copied, not cloned: an aggregate field is shared with the original.

### 6.2 `#[derive(Eq)]`

Emits an `eq(a, b)` comparing tag and every field structurally (`function S.eq(a, b)` — both operands explicit, no `self`). A `number`/`string`/`bool` field is compared with `==`; any other field (a nested struct or enum, `Option<T>`, `[T]`) goes through a module-local `__eq` helper that recurses — a value whose type has `eq` through that method, any other table key by key, so enums compare tag then payload and lists element-wise with equal length. The helper is emitted only in a module that needs it. The `==` operator on `Eq` types lowers to a call to this method rather than the target's native `==` (§4.2).

### 6.3 `#[derive(Debug)]`

Emits a `debug` (`function S.debug(self)`) producing a developer-facing string showing the type name and field values — the analogue of Rust's `{:?}`. A `number`/`bool` field renders with `tostring`; any other field renders through a module-local `__debug` helper (emitted only when needed) that recurses: a `string` double-quoted with `\`, `"`, newline, tab and carriage return escaped as `\\`, `\"`, `\n`, `\t`, `\r` (`"a"`, so `["b, c"]` and `["b", "c"]` stay distinct), a nested struct or enum through its own `debug`, `Option<T>` as `Some(1)` / `None`, `Result<T, E>` as `Ok(1)` / `Err("bad")`, `[T]` as `[1, 2]` / `[]`, an enum payload as `A(Inner { v = 2 })`. Every field's type must satisfy `Debug` (`STATIC.md` §9); a value reaching the helper through a type parameter whose type has no `debug` renders by shape alone: a struct as its name with elided fields, `Plain { .. }`; a named-payload variant likewise, `V { .. }` (named keys carry no order); a tuple-payload variant positionally, `P(2)` (slots are ordered); a unit variant bare, `U`. A table address never appears.

### 6.4 `#[derive(Display)]`

Emits a `display` (`function S.display(self)`) producing the user-facing string — the analogue of Rust's `{}`. Each field renders with `tostring`. Requires every field to itself satisfy `Display` (`STATIC.md` §9); `[T]`, `Option<T>`, and `Result<T, E>` do not, so a field of one is rejected rather than rendered as a table address.

### 6.5 `#[test]`

A `#[test]` function lowers normally and additionally registers itself on a module-level test table the runtime walks:

```
⟦ #[test] fn t() { … } ⟧ = local function t() … end ; __tests["t"] = t
```

The shape of `__tests` is part of the runtime contract (§7).

## 7. Runtime conventions

The shapes the emitted Lua relies on. These are part of the spec: a passing `CONFORMANCE/` test depends on them, and the embedding runtime must agree.

- **Struct**: `{ __struct = "TypeName", <fields> }`, with the type table as its metatable (`Type.__index = Type`) so methods resolve. Newtype field at `[1]`.
- **Enum**: `{ __enum = "TypeName", __tag = "VariantName", <payload> }`. Tuple payload at `[1..]`, struct payload by field name.
- **`Result<T, E>`**: the prelude enum, lowered per §3.2 — method-bearing, so each constructor wraps the table with the `Result` metatable: `Ok(v)` is `setmetatable({ __enum = "Result", __tag = "Ok", v }, Result)`, `Err(e)` is `setmetatable({ __enum = "Result", __tag = "Err", e }, Result)`.
- **`Option<T>`**: likewise method-bearing under the `Option` metatable — `Some(v)` is `setmetatable({ __enum = "Option", __tag = "Some", v }, Option)`, `None` is `setmetatable({ __enum = "Option", __tag = "None" }, Option)`.
- **Method dispatch**: `value:method(…)` (Lua colon call), resolved through the value's metatable. The method lives on the type table as `Type.method(self, …)`.
- **Indexing**: neoc is 0-based; lists lower to 1-based Lua tables with a `+ 1` adjustment at every index site.
- **Tests**: `__tests` maps test names to zero-argument functions.

The Result/Option prelude is emitted only when the module references them. The two enums are **method-bearing** (§3.2): each carries a shared metatable (`Option`/`Result`, with `__index` self-set), every constructor `setmetatable`s its value onto that table, and the inherent operations attach as `self`-first functions. `match` and `?` are unaffected — they read `__tag`/`__enum`/payload, none of which a metatable shadows.

```
local Option = {}
Option.__index = Option
local Result = {}
Result.__index = Result
local Ok   = function(v) return setmetatable({ __enum = "Result", __tag = "Ok", v }, Result) end
local Err  = function(e) return setmetatable({ __enum = "Result", __tag = "Err", e }, Result) end
local Some = function(v) return setmetatable({ __enum = "Option", __tag = "Some", v }, Option) end
local None = setmetatable({ __enum = "Option", __tag = "None" }, Option)
function Option.is_some(self) return self.__tag == "Some" end
function Option.is_none(self) return self.__tag == "None" end
function Option.unwrap(self)
  if self.__tag == "Some" then return self[1] end
  error("called unwrap on a None value", 0)
end
function Option.unwrap_or(self, default)
  if self.__tag == "Some" then return self[1] else return default end
end
function Option.map(self, f)
  if self.__tag == "Some" then return Some(f(self[1])) else return self end
end
function Option.ok_or(self, err)
  if self.__tag == "Some" then return Ok(self[1]) else return Err(err) end
end
function Result.is_ok(self) return self.__tag == "Ok" end
function Result.is_err(self) return self.__tag == "Err" end
function Result.unwrap(self)
  if self.__tag == "Ok" then return self[1] end
  error("called unwrap on an Err value", 0)
end
function Result.unwrap_err(self)
  if self.__tag == "Err" then return self[1] end
  error("called unwrap_err on an Ok value", 0)
end
function Result.map(self, f)
  if self.__tag == "Ok" then return Ok(f(self[1])) else return self end
end
function Result.map_err(self, f)
  if self.__tag == "Err" then return Err(f(self[1])) else return self end
end
```

The inherent methods (snake_case, host-agnostic; typed in `STATIC.md` §3.6):

- **`Option<T>`**: `is_some`, `is_none`, `unwrap`, `unwrap_or`, `map`, `ok_or`
- **`Result<T, E>`**: `is_ok`, `is_err`, `unwrap`, `unwrap_err`, `map`, `map_err`

`map`/`map_err`/`ok_or` carry a method-level generic (`map<U>`, `map_err<F>`, `ok_or<E>`) bound from the closure or argument at the call site. On the inert variant (`None.map`, `Err.map`, `Ok.map_err`) the value flows through unchanged. `unwrap`/`unwrap_err` abort with a neoc-shaped message — `called unwrap on a None value`, `called unwrap on an Err value`, `called unwrap_err on an Ok value` — carrying no host phrasing: `error(msg, 0)` (level 0 suppresses the host `file:line:` prefix).

## 8. Lua 5.1 deference

Everything not specified here defers to the Lua 5.1 Reference Manual (§2 Basic Concepts, §3 The Language):

- **Numbers**: Lua 5.1 has one `number` type (IEEE-754 double). neoc's `number` is exactly this; integer-vs-float is a literal-syntax distinction with no runtime existence.
- **Strings, tables, truthiness, coercion, arithmetic, comparison**: as Lua 5.1 defines them.
- **`continue`**: Lua 5.1 has no `continue` statement. neoc's `continue` lowers to a loop-body restructuring that skips to the next iteration without `goto` (which 5.1 also lacks) — the canonical encoding wraps the remainder of the loop body in a guard. The exact shape is pinned by the conformance cases under `CONFORMANCE/dynamic/`.
- **`nil`**: neoc has no `nil` and no null. `()` lowers to `nil` only as the value of effectful expressions; it is never observable as a neoc value, because `Option<T>` is how absence is modelled.

## 9. Runtime type enforcement

Generated code enforces declared types at runtime — **deep, always-on, at drift points**. A `string` field rejects a non-string at construction and at any later mutation; the full shape is validated recursively. This section defines *what* a check lowers to. *Where* checks go — which positions require one and which are statically guaranteed not to — is `STATIC.md` §10. As with the rest of this file, the lowering is to Lua 5.1; the `lua` target meets the same observable contract through `CONFORMANCE/` (§9.4).

### 9.1 Validators

One reusable validator per declared type, emitted once at module top and shared by every checked site — never inlined per site. A validator takes the value and a path string, raises a structured error (§9.2) on the first mismatch, and returns the value on success. Composite validators recurse, so a single call validates the full depth.

Primitive validators, emitted when referenced:

```
local function __check_number(v, path) if type(v) ~= "number"  then __type_error(path, "number", v) end return v end
local function __check_string(v, path) if type(v) ~= "string"  then __type_error(path, "string", v) end return v end
local function __check_bool(v, path)   if type(v) ~= "boolean" then __type_error(path, "bool",   v) end return v end
```

Combinators for the structural types take the element validator as an argument, so they compose without inlining:

```
-- [τ] — a sequence; each element validated against τ. neoc indexes from 0,
-- so the reported index is the Lua index minus one (§4.7). A host-supplied
-- table (ingress, STATIC §10.2) need not be a sequence, so sequence-ness is
-- verified, not assumed: count every key, then require 1..n present. n keys
-- filling 1..n leaves no room for a hole or a stray key, so a dict
-- ({name=…}: n≥1 but v[1]==nil) and a holed table ({[1],[3]}: v[2]==nil) are
-- rejected rather than read as a short or empty list off #v.
local function __check_list(v, path, elem)
  if type(v) ~= "table" or v.__struct or v.__enum then __type_error(path, "list", v) end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  for i = 1, n do
    if v[i] == nil then __type_error(path, "list", v) end
    elem(v[i], path .. "[" .. (i - 1) .. "]")
  end
  return v
end

-- Option<τ>
local function __check_option(v, path, some)
  if type(v) ~= "table" or v.__enum ~= "Option" then __type_error(path, "Option", v) end
  if v.__tag == "Some" then some(v[1], path .. ".0")
  elseif v.__tag ~= "None" then __type_error(path, "Option", v) end
  return v
end

-- Result<τ, ε>
local function __check_result(v, path, ok, err)
  if type(v) ~= "table" or v.__enum ~= "Result" then __type_error(path, "Result", v) end
  if     v.__tag == "Ok"  then ok(v[1],  path .. ".0")
  elseif v.__tag == "Err" then err(v[1], path .. ".0")
  else __type_error(path, "Result", v) end
  return v
end
```

One validator per user-declared type. It checks the runtime tag, then each field/payload against its declared type. The validator locals are **forward-declared up front and then assigned** so mutually recursive types resolve — `local function __check_X` would scope the name to its own body only, leaving a sibling reference (`__check_Address` below) bound to a nil global and failing at the first mutual call. A single `local` declaration puts every name in scope before any body is assigned:

```
-- struct User    { name: string, addresses: [Address] }
-- struct Address { zip: string, resident: Option<User> }   -- closes the cycle
local __check_User, __check_Address   -- in scope for every body below

function __check_User(v, path)
  if type(v) ~= "table" or v.__struct ~= "User" then __type_error(path, "User", v) end
  __check_string(v.name, path .. ".name")
  __check_list(v.addresses, path .. ".addresses", __check_Address)   -- resolves
  return v
end

function __check_Address(v, path) … end   -- references __check_User likewise
```

- **Newtype** `struct N(τ)`: validate `v[1]` against `τ` at path `⟪path⟫ .. ".0"`.
- **Enum** `E`: check `v.__enum == "E"`, then dispatch on `v.__tag` and validate that variant's payload — tuple slots at `v[1], v[2], …`, struct-variant fields by name. An unrecognised `__tag` is an error.
- **`Option<τ>` / `Result<τ, ε>`**: the combinators above, composed with the payload validators.
- **`fn(…) -> _`**: checked as callable (`type(v) == "function"`); a signature is not runtime-observable.
- **Opaque extern `struct`**: no inspectable shape — the validator is the identity (`STATIC.md` §10). Recursion stops at the opaque boundary.
- **Bare type parameter** with no concrete witness at the position: validated only to the depth its static type reveals; the deep check lands where the parameter is instantiated (`STATIC.md` §10).
- **`()`**: lowers to `nil` and is never observable as a neoc value (§8). It has no validator, and a position declared `-> ()` — including an `extern`/ingress result — emits no check.

### 9.2 Structured errors

A mismatch raises through the target's idiom — Lua `error` — a path-aware message `⟪path⟫: expected ⟪type⟫, got ⟪actual⟫`:

```
local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
```

`actual` is the value's neoc-visible shape — a tagged table reports its `__struct`/`__enum`. The example `User.addresses[2].zip: expected string, got number` is produced by `__check_User` recursing into `addresses` (a `[Address]`), element `[2]`, then `Address`'s `zip` field. (`number`, not a Lua-specific name — neoc's `number` is the runtime numeric type, §8.)

### 9.3 Drift-point lowerings

Each drift point (`STATIC.md` §10) passes the incoming value through its type's validator; a stable position lowers unchanged. `⟦check τ⟧` denotes `τ`'s validator from §9.1.

**Construction** — the constructor validates each declared field; variant and list literals validate each payload/element. Extends §3.1 and §4.4:

```
⟦ struct S { f₁: τ₁, …, fₙ: τₙ } ⟧ =
  local S = {}
  S.__index = S
  function S.new(fields)
    ⟦check τ₁⟧(fields.f₁, "S.f₁")    (* one per field, deep *)
    …
    ⟦check τₙ⟧(fields.fₙ, "S.fₙ")
    fields.__struct = "S"
    return setmetatable(fields, S)
  end
```

A newtype/tuple `N(e)` validates `e` against the wrapped type inside `N.new` (extending §4.4); an enum-variant construction `E::V(e₁, …)` validates each `eᵢ` against `V`'s declared slot type; a list literal `[e₁, …]` of type `[τ]` validates each `eᵢ` against `τ`.

**Mutation** — assignment to a typed lvalue validates the new value before storing it. The value is bound once so it is not evaluated twice:

```
⟦ e.f = v ⟧ₛ    = local ⟪t⟫ = ⟦v⟧ₑ ; ⟦check τ_f⟧(⟪t⟫, "S.f") ; ⟦e⟧ₑ.f = ⟪t⟫
⟦ xs[i] = v ⟧ₛ  = local ⟪t⟫ = ⟦v⟧ₑ ; ⟦check τ_e⟧(⟪t⟫, "xs[i]") ; ⟦xs⟧ₑ[⟦i⟧ₑ + 1] = ⟪t⟫
```

`τ_f` is the declared type of field `f` on the lvalue's type; `τ_e` is the list element type. Both are supplied by the drift analysis. Compound assignment (`e.f += v`, …) desugars to `e.f = e.f + v` and lowers through the same barrier. A metatable `__newindex` guard is **not** used: in Lua 5.1 `__newindex` fires only for an *absent* key, so it cannot intercept reassignment of an already-set field. The barrier is the assignment lowering itself, where the field's static type is known.

**Dynamic ingress** — a value crossing from untyped (host) into a typed position is validated at the boundary:

```
⟦ f(a…) ⟧ₑ   where  extern fn f(…) -> τ   = ⟦check τ⟧(f(⟦a⟧…), "f()")
```

The same wrap applies to a value produced by a host decode or coercion declared to yield `τ`, and to runtime-supplied entry arguments (validated against the entry's declared parameter types). The host produces the value; neoc validates it before any typed use.

**Untyped boundary params** — a function reachable from untyped host code validates each typed parameter at entry; an internal-only function does not (`STATIC.md` §10):

```
⟦ pub fn g(p: τ, …) { body } ⟧ =
  function g(p, …)
    ⟦check τ⟧(p, "g.p")    (* one per typed parameter *)
    …
    ⟦ body ⟧ₛ
  end
```

### 9.4 Cross-target obligation

This file is the Lua lowering; the validator *mechanism* on another target is that target's choice — a write-trap or accessor native to it may realise the mutation barrier in place of an explicit lowered call. What is fixed, and pinned per target by `CONFORMANCE/`, is the *what* and *where*: the identical wrong-typed value is rejected at the identical drift point, raising through the target's idiom with a matching path-aware message and a non-zero exit.
