# `extern` Blocks

How neoc describes the surface of an external runtime (the **target**) to the
type checker, and how references to it lower to target code. This is the only
place declarations may appear without bodies; everywhere else, every `fn`,
method, and constructor has an implementation.

An `extern` block is authored directly — a *port* the host runtime implements —
or emitted by a generator from a foreign surface. The language rules here are
host-agnostic; the *contents* of any given block belong to whichever runtime is
bound.

---

## Block syntax

```
extern {
    <item>*
}
```

Zero or more items. Permitted: `mod`, `struct`, `impl`, `fn`. Visibility
modifiers apply as they would outside the block.

`extern` blocks appear at module scope. They are not items — they cannot be
`pub` and cannot be referenced. They are syntactic regions that change the
body-elision rule for their contents. **One target per compilation** — there is
no `extern "abi"` multi-target form.

---

## Body-elision rule

Inside an `extern` block (recursively, including nested `mod` and `impl`):

- **`fn`** has no body. `fn foo() -> T;` is valid; `fn foo() -> T { … }` is an error.
- **`struct`** declares its **fields** but no methods. A field is a *data
  property* (`changes: number`) or a *callback* (`fetch: fn(Request) -> Response`)
  of the target value — never an implementation. An empty `struct Foo;` is a pure
  opaque handle (no readable fields).
- **`impl`** contains only bodyless `fn`s (method signatures).
- **`mod`** contains only extern-permitted items.

Outside an `extern` block the inverse holds: bodies are required.

---

## Items

### `fn`

A bodyless function. Ordinary signature rules (typed params, optional return,
generics).

```
pub fn read_file(path: string) -> Promise<string>;
```

`async fn` is sugar for a function returning `Promise<T>`. `Promise` is a
prelude nominal type, not a builtin.

### `struct`

A nominal target handle. It may carry **fields** — the target value's data
properties and callbacks:

```
pub struct Changes {
    changes: number,
    lastInsertRowid: Unknown,
}

pub struct ServeOptions {
    port: number,
    fetch: fn(Request) -> Response,
}

pub struct Response;   // opaque handle: no readable fields
```

Fields are Rust-shaped: data fields and function-typed fields (neoc's single
`fn(A) -> B` covers what Rust splits across `fn` / `Fn` / `dyn`, since neoc has
no borrowing). Generic parameters are permitted: `pub struct Promise<T>;`.

A struct may have both fields and an `impl` — data/callbacks are fields, methods
are `impl` fns. That split *is* the property-vs-method distinction; no marker is
needed.

### `impl`

A bodyless inherent method block. Methods take `self` (no `&self`/`&mut self` —
no borrowing). A self-less method is an associated function (static on the
target). Multiple `impl` blocks merge. Trait impls are out of scope.

```
impl Database {
    #[extern(constructor)] pub fn new(filename: string);
    pub fn query(self, sql: string) -> Statement;
    pub fn run(self, sql: string) -> Changes;
}
```

### `mod`

A named scope representing a target module. Nesting maps to `/` in the target's
path unless overridden.

```
mod fs {
    mod promises {
        pub fn read_file(path: string) -> Promise<string>;
    }
}
```

`fs::promises` items import from target path `fs/promises`.

---

## Attributes

A closed set of bare-name attributes, valid only inside `extern`. Unknown bare
attributes and any path-based attribute (`#[a::b(…)]`, reserved for future
macros) are errors. The sole attribute is `#[extern(…)]`; params:

- **`ambient`** — available without import. `fn`/`struct` referenced from the
  target's global scope; `mod` contents accessed via a global path, no import
  emitted. Propagates to descendants; re-applying to a descendant is an error.
- **`path = "<string>"`** — sets a `mod`'s target path (scoped packages,
  kebab-case, protocol prefixes). Nested mods append with `/`. `mod` only.
- **`name = "<string>"`** — sets an `fn`/`struct`'s target identifier. The
  neoc-side name is unaffected. `fn`/`struct` only; not on constructors.
- **`constructor`** — marks an `fn` in an `impl` as a constructor; lowers to the
  target's constructor form. No explicit return type (it is the enclosing
  `Self`). Incompatible with `name`. An `impl` may declare several, each under
  its own name (`new`, `from_pem`, …) — the target has no overload resolution to
  disambiguate one name.

Params combine in one `#[extern(…)]` (`#[extern(ambient, path = "globalThis.crypto.subtle")]`).
Multiple `#[extern(…)]` on one item is an error.

---

## Lowering

Source-language intent; precise emission is codegen's. The neoc value of an
extern type **is** the target value — there is no wrapper indirection.

- **Path** — a `mod`'s target path is its name (nested joined by `/`), or its
  `#[extern(path)]`. An `ambient` mod emits no import; its path is a
  member-access chain rooted at the target global (`path` is the root expr, which
  may be dotted: `"globalThis.crypto.subtle"`).
- **Name** — an item's target identifier is its source name, or its
  `#[extern(name)]`.
- **Field read** `x.f` → target property read `x.f`.
- **Field write** `x.f = v` → target property write `x.f = v`.
- **Struct literal** `T { f: v }` → target object literal `{ f: v }` (so option
  bags build naturally). A function-typed field takes a neoc closure → a target
  function, no marshalling.
- **Method call** `x.m(args)` → target method call on the handle.
- **Associated call** `T::f(args)` → static/namespaced target call.
- **Constructor** `T::new(args)` → the target's constructor form. Luau has no
  `new` operator, so a constructor is the plain associated call `T.new(args)`
  (`DYNAMIC.md` §3.9) — it differs from an ordinary associated call only in that
  the checker types its return as the enclosing `Self`.
- **Async** — `async fn` / `Promise<T>` → an asynchronous target call; `.await`
  → the target's await form. *(Async is a prerequisite language feature; the
  first extern implementation targets the synchronous subset.)*

### Cross-file `use`

A `pub` extern item is `use`-able from another file exactly as a plain `pub`
item is (§Visibility). An extern item exports at its **file**-module level — the
enclosing extern `mod` is an import-path concern, not a `use`-namespace segment —
so `use db::Database` names the file `db` and the item `Database`, regardless of
which extern `mod` declares `Database`.

A `use`d extern item lowers to its **native** import in the *consumer*, not an
import from the defining file. An extern emits no runtime value in its defining
module, so there is nothing there to import; the consumer reconstructs the native
import from the item's `#[extern(…)]` metadata, identically to how the defining
module would. For `use db::Database` where `Database` is declared in a
`#[extern(path = "bun:sqlite")] mod sqlite`:

- the consumer emits the same native import the defining module would (the
  `bun:sqlite` require), and `Database::new(…)` lowers to its constructor form in
  the consumer, qualified through that `mod`'s binding
  (`sqlite.Database.new(…)`), with no reference to the defining file;
- `#[extern(ambient)]` (no import; global root) and `#[extern(name = "…")]` are
  honoured in the consumer too.

---

## `Unknown`

`Unknown` is a **language primitive** — the dynamic boundary type (`STATIC.md`
§Unknown). Member access, method call, index, and assignment on it yield
`Unknown`, and it flows to and from any type. It is the honest type for a foreign
value whose runtime shape neoc cannot name: a heterogeneous union, `any`,
`unknown`, or anything with no faithful neoc form. It is not a wrapper and
carries no runtime representation — it lowers to the bare target value.

---

## Visibility

Standard rules, unchanged. Default module-private; `pub`, `pub(crate)`,
`pub(super)`, `pub(in path)` all permitted on items. `extern` blocks themselves
take no visibility modifier.

---

## Errors

Hard errors, each with a corrective suggestion:

- a function body inside `extern`, or a bodyless function outside it;
- a method body inside `extern`;
- an unknown bare attribute, or unknown `#[extern(…)]` param;
- `#[extern(constructor)]` outside an `impl` inside `extern`, combined with
  `name`, or with an explicit return type;
- `#[extern(name)]` on a `mod`; `#[extern(path)]` on a non-`mod`;
- a path-based attribute;
- `#[extern(ambient)]` on a descendant of an ambient `mod`.

---

## Non-goals

- Structural typing / shape declarations — structs are nominal.
- Function overloading — overloaded host functions take distinct neoc names.
- Union types and variadic params as language features. At the foreign boundary
  a heterogeneous union is `Unknown`; a variadic host function is declared with
  the arity a call site needs, since neoc has no spread emission.
- Trait impls on extern types — inherent impls only.
- User-defined attributes — reserved for future proc-macro support.
- Multi-target dispatch (`extern "abi"`) — one target per compilation.

---

## Prelude

A prelude `extern` block would declare the host's standard types implicitly,
written in this same syntax with no privileged status. Not built: the compiler
ships no `extern` prelude, and its only implicit names are the `Result`/`Option`
language prelude (`DYNAMIC.md` §7) and the `Unknown` primitive. Naming the host's
types is an author's job until then, and the set is the host's — the earlier
JavaScript list (`Promise<T>`, `Array<T>`, `Object`, `Function`, `Error`) named
no type Luau has.

---

## Appendix: authoring at the foreign boundary

The governing principle when a host surface has no faithful neoc form —
**degrade toward the bare target value; never put a neoc *tagged* representation
(`Option`, `enum`) into a foreign call**, because the backend passes arguments
verbatim and a tag would leak into the call.

| host surface | neoc | rationale |
| --- | --- | --- |
| an omittable trailing argument | drop it, or declare the arity the call site needs | `Option<T>` leaks its tag and doesn't recover omission (`None` ≠ absent) |
| a field that may be absent | its base type `T` (`Unknown` if none) | a foreign field read is not a neoc `Option` |
| a variadic function | declare the arity a call site needs | a list `[T]` mis-shapes the call (`f({a,b})` vs `f(a,b)`); a faithful form needs **spread emission** at `ext` call sites, not built |
| a value of several unrelated types | `Unknown` | a real union type would need sum types neoc deliberately lacks |
| a value restricted to fixed strings | `string` | the value *is* a string at runtime; an `enum` would leak its tag. A faithful form needs **string-valued enum lowering** (unit-variant → its string), not built |
| a dynamic value with no faithful form | `Unknown` | the dynamic boundary primitive |
