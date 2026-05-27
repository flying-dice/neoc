# Dynamic semantics

Defines what every meaningful neoc program *does* when executed, expressed as a translation to Lua 5.1. neoc is a transpiler; its dynamic semantics are the dynamic semantics of Lua 5.1 applied to the lowering of each construct.

This file is a **lowering table**: one entry per neoc construct, giving the Lua 5.1 source it produces. Once a program is lowered, its runtime behaviour is whatever Lua 5.1 would do with that source — see the Lua 5.1 Reference Manual for the underlying semantics.

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

## 3. Items

### 3.1 `struct`

A struct value is a Lua table tagged with its type name. The type's own table doubles as the method table and the shared metatable: `S.__index = S` makes instances find their methods (§3.3), and the constructor stamps each new value with `setmetatable`. Named struct:

```
⟦ struct S { f₁: τ₁, …, fₙ: τₙ } ⟧ =
  local S = {}
  S.__index = S
  function S.new(fields)
    fields._struct = "S"
    return setmetatable(fields, S)
  end
```

`S.new` is an internal constructor; `S { … }` literals (§4) call it. The `_struct` tag is the value's runtime identity, read by `match` (§4.5) and `Eq`/`Debug` (§6); the metatable carries dispatch (§4.7).

- **Unit struct** `struct S;` lowers to a single shared value `local S = { _struct = "S" }`. Methods (§3.3) attach to `S` itself, so `S:m(…)` resolves directly with no separate metatable.
- **Newtype** `struct N(τ)` lowers like a named struct with one field stored at key `[1]`; `N(x)` constructs `setmetatable({ _struct = "N", [1] = x }, N)` and `.0` reads `[1]`.

Methods are not emitted here; they come from `impl` (§3.3).

### 3.2 `enum`

An enum value is a tagged table carrying its variant name and payload:

```
⟦ enum E { … } ⟧ = (* no standalone output for a plain enum; constructors are
                      inlined at use sites *)
```

A variant constructor lowers at each use site (§4.4):

- **Unit variant** `E::V` → `{ _enum = "E", tag = "V" }`
- **Tuple variant** `E::V(a, b)` → `{ _enum = "E", tag = "V", ⟦a⟧, ⟦b⟧ }` (payload at `[1]`, `[2]`, …)
- **Struct variant** `E::V { x: a }` → `{ _enum = "E", tag = "V", x = ⟦a⟧ }`

`tag` is read by `match`; `_enum` guards against matching a same-tagged variant of a different enum.

**Enums with methods.** When an enum has any `impl` block or `#[derive(...)]`, it emits a method table doubling as the shared metatable — exactly like a struct (§3.1):

```
⟦ enum E { … } with methods ⟧ =
  local E = {}
  E.__index = E
```

Every variant construction is then `setmetatable`d onto `E`, so a value finds its methods through the metatable and a method call lowers to a Lua colon call (§3.3, §4.7):

- **Unit variant** `E::V` → `setmetatable({ _enum = "E", tag = "V" }, E)`
- **Tuple variant** `E::V(a, b)` → `setmetatable({ _enum = "E", tag = "V", ⟦a⟧, ⟦b⟧ }, E)`
- **Struct variant** `E::V { x: a }` → `setmetatable({ _enum = "E", tag = "V", x = ⟦a⟧ }, E)`

`impl` methods (§3.3) and derived methods (§6) attach to `E` as for a struct. The metatable changes nothing `match` or `?` rely on — they read `.tag`/`._enum`, which a metatable does not shadow. A plain enum with no methods keeps the bare tagged table (no metatable). The `Result`/`Option` prelude enums (§7) are always plain.

### 3.3 `impl`

Inherent and trait-impl methods attach to the target type's table as plain functions taking `self` first:

```
⟦ impl S { fn m(self, p) -> τ { body } } ⟧ =
  function S.m(self, p) ⟦ body ⟧ₛ end
```

Because the constructor sets `S.__index = S` and `setmetatable`s each instance (§3.1), a value of type `S` finds `m` through its metatable. A method call `x.m(a)` lowers to the Lua colon call `x:m(a)`, which passes the receiver as `self` (§4.7) — dispatch happens at runtime via the metatable, so the emitter needs no type information at the call site. An associated function (no `self`) is still a plain entry on the table and is called as `S.f(a)`.

### 3.4 `trait`

A trait produces no standalone output. Its required methods are realised by each `impl Trait for Type`. A default-bodied trait method, when not overridden, is emitted onto the implementing type's table at the impl site, with `Self` replaced by the implementing type name.

### 3.5 `const`

```
⟦ const C: τ = e ⟧ₛ = local C = ⟦ e ⟧ₑ      (pub: global C = ⟦ e ⟧ₑ)
```

### 3.6 `let`

```
⟦ let x = e ⟧ₛ        = local x = ⟦ e ⟧ₑ
⟦ let mut x = e ⟧ₛ    = local x = ⟦ e ⟧ₑ      (mut is a static-only distinction; no runtime effect)
⟦ let p = e ⟧ₛ        = (destructuring; see §5)
```

### 3.7 `use`

```
⟦ use a::b::c ⟧       = local c = require("a.b").c
⟦ use a::b::c as d ⟧  = local d = require("a.b").c
```

The path up to the final segment is the module; the final segment is the item selected from it. `require` uses Lua's dotted module convention.

### 3.8 `type` alias

No output. Type aliases are erased; they exist only in `STATIC.md`.

### 3.9 `ext`

**No output.** An `ext fn`, `ext type`, or `ext const` produces no Lua. The named binding is expected to be present in the runtime's environment when the chunk executes. The compiler emits no shim, stub, or resolution code; resolution is the host's responsibility.

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

`+` selects Lua `+` or `..` by the static type of its operands (`STATIC.md` T-ARITH / T-CONCAT) — the choice is fixed at compile time, never a runtime check. `!=` lowers to Lua `~=`.

### 4.3 Range expressions

`a..b` and `a..=b` lower to a small range value the prelude understands; in a `for` head (§4.9) they lower directly to numeric `for` bounds without allocating.

### 4.4 Struct and enum construction

```
⟦ S { f: e, … } ⟧ₑ      = S.new({ f = ⟦e⟧, … })
⟦ S { f, .. base } ⟧ₑ   = S.new({ f = f, <copied fields from ⟦base⟧> })
⟦ N(a) ⟧ₑ               = N.new(⟦a⟧)     (tuple/newtype struct: a call constructs through .new)
⟦ E::V ⟧ₑ               = { _enum = "E", tag = "V" }
⟦ E::V(a, …) ⟧ₑ         = { _enum = "E", tag = "V", ⟦a⟧, … }
⟦ E::V { x: a, … } ⟧ₑ   = { _enum = "E", tag = "V", x = ⟦a⟧, … }
```

Prelude constructors `Ok`/`Err`/`Some`/`None` follow §7. Each component is validated against its declared field, slot, or element type at construction — a drift point (§9.3).

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
⟦ e.0 ⟧ₑ        = ⟦e⟧[1]                 (newtype field)
⟦ e.m(a, …) ⟧ₑ  = ⟦e⟧:m(⟦a⟧, …)          (Lua colon call: receiver passed as self)
⟦ f(a, …) ⟧ₑ    = f(⟦a⟧, …)
⟦ e[i] ⟧ₑ       = ⟦e⟧[⟦i⟧ + 1]            (lists are 1-indexed in Lua; neoc indexes from 0)
```

Method dispatch goes through the receiver's metatable. The colon call `⟦e⟧:m(…)` looks `m` up on the value's metatable (set by its constructor, §3.1) and passes the receiver as the implicit `self`, so the call site needs no static knowledge of the receiver's type. List indexing adjusts neoc's 0-based index to Lua's 1-based tables.

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
  if ⟪t⟫.tag == "Err" or ⟪t⟫.tag == "None" then return ⟪t⟫ end
and yields the expression  ⟪t⟫[1]
```

`⟪t⟫` is a fresh temporary. On the short-circuiting variant (`Err` for `Result`, `None` for `Option`) the hoisted `return ⟪t⟫` exits the enclosing function with the carrier unchanged; otherwise the expression evaluates to the wrapped value at `[1]`. Testing both tags lets the lowering serve `Result` and `Option` alike without the carrier's static type — a value carries exactly one tag, so only the matching test can fire. The enclosing function's return type makes this well-formed (`STATIC.md` §6).

## 5. Patterns

Patterns lower only inside `match` (§4.5) and `let` destructuring. Each pattern compiles to a **test** (a boolean Lua expression over the scrutinee) and a set of **bindings** (locals introduced when the test passes).

```
_              test: true                          bind: —
x              test: true                          bind: local x = __s
42 / "s" / true test: __s == 42                    bind: —
S { f: p }     test: __s._struct == "S" and <test p over __s.f>   bind: <p over __s.f>
E::V           test: __s.tag == "V"                bind: —
E::V(p₁, …)    test: __s.tag == "V" and <tests>    bind: <pᵢ over __s[i]>
p₁ | p₂        test: <test p₁> or <test p₂>        bind: shared names
```

A guard `if g` is appended to the arm's test with `and ⟦g⟧`, evaluated only after the pattern test passes so guard code sees the pattern's bindings.

## 6. Built-in attribute derives

### 6.1 `#[derive(Clone)]`

Emits `function S.clone(self)` returning a shallow copy carrying the same `_struct`/`_enum` tag.

### 6.2 `#[derive(Eq)]`

Emits `function S.eq(a, b)` comparing tag and every field structurally. The `==` operator on `Eq` types lowers to a call to this function rather than Lua `==` (§4.2).

### 6.3 `#[derive(Debug)]`

Emits `function S.debug(self)` producing a developer-facing string showing the type name and field values — the analogue of Rust's `{:?}`.

### 6.4 `#[derive(Display)]`

Emits `function S.display(self)` producing the user-facing string — the analogue of Rust's `{}`. Requires every field to itself satisfy `Display` (`STATIC.md` §9).

### 6.5 `#[test]`

A `#[test]` function lowers normally and additionally registers itself on a module-level test table the runtime walks:

```
⟦ #[test] fn t() { … } ⟧ = local function t() … end ; __tests["t"] = t
```

The shape of `__tests` is part of the runtime contract (§7).

## 7. Runtime conventions

The shapes the emitted Lua relies on. These are part of the spec: a passing `CONFORMANCE/` test depends on them, and the embedding runtime must agree.

- **Struct**: `{ _struct = "TypeName", <fields> }`, with the type table as its metatable (`Type.__index = Type`) so methods resolve. Newtype field at `[1]`.
- **Enum**: `{ _enum = "TypeName", tag = "VariantName", <payload> }`. Tuple payload at `[1..]`, struct payload by field name.
- **`Result<T, E>`**: the prelude enum, lowered per §3.2 — `Ok(v)` is `{ _enum = "Result", tag = "Ok", v }`, `Err(e)` is `{ _enum = "Result", tag = "Err", e }`.
- **`Option<T>`**: `Some(v)` is `{ _enum = "Option", tag = "Some", v }`, `None` is `{ _enum = "Option", tag = "None" }`.
- **Method dispatch**: `value:method(…)` (Lua colon call), resolved through the value's metatable. The method lives on the type table as `Type.method(self, …)`.
- **Indexing**: neoc is 0-based; lists lower to 1-based Lua tables with a `+ 1` adjustment at every index site.
- **Tests**: `__tests` maps test names to zero-argument functions.

The Result/Option prelude is emitted only when the module references them:

```
local Ok   = function(v) return { _enum = "Result", tag = "Ok", v } end
local Err  = function(e) return { _enum = "Result", tag = "Err", e } end
local Some = function(v) return { _enum = "Option", tag = "Some", v } end
local None = { _enum = "Option", tag = "None" }
```

## 8. Lua 5.1 deference

Everything not specified here defers to the Lua 5.1 Reference Manual (§2 Basic Concepts, §3 The Language):

- **Numbers**: Lua 5.1 has one `number` type (IEEE-754 double). neoc's `number` is exactly this; integer-vs-float is a literal-syntax distinction with no runtime existence.
- **Strings, tables, truthiness, coercion, arithmetic, comparison**: as Lua 5.1 defines them.
- **`continue`**: Lua 5.1 has no `continue` statement. neoc's `continue` lowers to a loop-body restructuring that skips to the next iteration without `goto` (which 5.1 also lacks) — the canonical encoding wraps the remainder of the loop body in a guard. The exact shape is pinned by the conformance cases under `CONFORMANCE/dynamic/`.
- **`nil`**: neoc has no `nil` and no null. `()` lowers to `nil` only as the value of effectful expressions; it is never observable as a neoc value, because `Option<T>` is how absence is modelled.

## 9. Runtime type enforcement

Generated code enforces declared types at runtime — **deep, always-on, at drift points**. A `string` field rejects a non-string at construction and at any later mutation; the full shape is validated recursively. This section defines *what* a check lowers to. *Where* checks go — which positions require one and which are statically guaranteed not to — is `STATIC.md` §10. As with the rest of this file, the lowering is to Lua 5.1; every other target meets the same observable contract through `CONFORMANCE/` (§9.4).

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
-- so the reported index is the Lua index minus one (§4.7).
local function __check_list(v, path, elem)
  if type(v) ~= "table" or v._struct or v._enum then __type_error(path, "list", v) end
  for i = 1, #v do elem(v[i], path .. "[" .. (i - 1) .. "]") end
  return v
end

-- Option<τ>
local function __check_option(v, path, some)
  if type(v) ~= "table" or v._enum ~= "Option" then __type_error(path, "Option", v) end
  if v.tag == "Some" then some(v[1], path .. ".0")
  elseif v.tag ~= "None" then __type_error(path, "Option", v) end
  return v
end

-- Result<τ, ε>
local function __check_result(v, path, ok, err)
  if type(v) ~= "table" or v._enum ~= "Result" then __type_error(path, "Result", v) end
  if     v.tag == "Ok"  then ok(v[1],  path .. ".0")
  elseif v.tag == "Err" then err(v[1], path .. ".0")
  else __type_error(path, "Result", v) end
  return v
end
```

One validator per user-declared type. It checks the runtime tag, then each field/payload against its declared type. Validators are declared as locals up front so mutually recursive types resolve:

```
-- struct User { name: string, addresses: [Address] }
local function __check_User(v, path)
  if type(v) ~= "table" or v._struct ~= "User" then __type_error(path, "User", v) end
  __check_string(v.name, path .. ".name")
  __check_list(v.addresses, path .. ".addresses", __check_Address)
  return v
end
```

- **Newtype** `struct N(τ)`: validate `v[1]` against `τ` at path `⟪path⟫ .. ".0"`.
- **Enum** `E`: check `v._enum == "E"`, then dispatch on `v.tag` and validate that variant's payload — tuple slots at `v[1], v[2], …`, struct-variant fields by name. An unrecognised `tag` is an error.
- **`Option<τ>` / `Result<τ, ε>`**: the combinators above, composed with the payload validators.
- **`fn(…) -> _`**: checked as callable (`type(v) == "function"`); a signature is not runtime-observable.
- **Opaque `ext type`**: no inspectable shape — the validator is the identity (`STATIC.md` §10). Recursion stops at the opaque boundary.
- **Bare type parameter** with no concrete witness at the position: validated only to the depth its static type reveals; the deep check lands where the parameter is instantiated (`STATIC.md` §10).

### 9.2 Structured errors

A mismatch raises through the target's idiom — Lua `error` — a path-aware message `⟪path⟫: expected ⟪type⟫, got ⟪actual⟫`:

```
local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got._struct or got._enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
```

`actual` is the value's neoc-visible shape — a tagged table reports its `_struct`/`_enum`. The example `User.addresses[2].zip: expected string, got number` is produced by `__check_User` recursing into `addresses` (a `[Address]`), element `[2]`, then `Address`'s `zip` field. (`number`, not a Lua-specific name — neoc's `number` is the runtime numeric type, §8.)

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
    fields._struct = "S"
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
⟦ f(a…) ⟧ₑ   where  ext fn f(…) -> τ   = ⟦check τ⟧(f(⟦a⟧…), "f()")
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
