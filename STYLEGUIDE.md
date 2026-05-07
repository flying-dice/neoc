# Style Guide

Code conventions for this repository. Rules here are binding; discuss changes before deviating.

---

## Design principles

Every API decision must satisfy these four rules, in priority order:

1. **No surprises.** Behave like every other major runtime. If Rust, Go, Python, and Node agree on a convention, follow it. Platform-native path separators, standard error semantics, conventional return types. A developer should never need to unlearn habits from other runtimes to use neoc.

2. **No footguns.** Never ship an API that is unintuitive or likely to cause silent mistakes. If a design needs a warning paragraph to use correctly, redesign it. Obvious over clever.

3. **Mirror Rust first.** The primary API surface is `UserData` types that mirror `std::*` 1:1. Same method names, same semantics, same return types (adapted for Lua where the borrow checker doesn't apply). When facing a design question, the first answer is always "what does Rust's standard library do?"

4. **Supplement with scripting ergonomics.** Layer flat functions (Python/Node-style) on top of the UserData foundation for quick scripting use cases. These are convenience — they delegate to the UserData layer internally and never introduce their own design decisions.

**Precedence:** 1 > 2 > 3 > 4. If mirroring Rust would violate industry consensus, don't mirror it. If a Rust-faithful API is a footgun in scripting context, fix the footgun. The flat-function layer never overrides decisions made by principles 1–3.

These principles apply to all three namespaces (`std:*`, `lib:*`, `vnd:*`). A `vnd:*` module that wraps a crate still follows principles 1–2 — if the upstream crate has a surprising API, the wrapper smooths it.

---

## Module namespaces

There is **no ambient `lib` global**. Scripts reach modules through a custom `require` that takes a strict `namespace:name` spec — three namespaces, picked at registration time. This keeps the import surface explicit and the namespace a contract that travels with the file.

| Namespace | Contains | Examples |
|---|---|---|
| `std:*` | Mirrors the Rust standard library (and tightly-scoped extensions on top). Anything you'd reach for in `std::` lives here. | `std:collections`, `std:fs`, `std:io`, `std:net`, `std:thread`, `std:workers` (the tokio exception — runtime substrate, no `std::` equivalent) |
| `lib:*` | Blessed gap-fillers and our own custom modules. Either plugs a hole Rust std doesn't cover (`lib:base64`) or is something we wrote (`lib:test`, `lib:vcr`). | `lib:base64`, `lib:test`, `lib:vcr` |
| `vnd:*` | Vendored crates. **The module name must match the upstream crate name** so the import is unambiguous. No renaming, no aliasing. | `vnd:hyper`, `vnd:serde_json`, `vnd:rusqlite`, `vnd:toml`, `vnd:jsonschema` |

Picking the namespace:

- Is there a clear `std::*` analog? → `std:*`. Use the same name (`std::collections` → `std:collections`).
- Is it our code, or a small vetted helper crate filling a gap? → `lib:*`.
- Is it a vendored crate users would recognise from `Cargo.toml`? → `vnd:*` with the crate's exact name.

The registry is built once at engine boot in `src/lua/lib/mod.rs::install`. Adding a module means picking the right namespace and adding one line.

---

## Module API pattern — UserData + scripting

Every module that wraps a substantive type (a URL, a path, a connection, a reader) ships **two API surfaces**:

1. **UserData types** — the foundation. Wrap the backing Rust type as `UserData` with methods that mirror the Rust API 1:1 (same names, same semantics, adapted for Lua where the borrow checker doesn't apply). These are the primary API; they're what scripts use for anything beyond one-liners.

2. **Flat convenience functions** — ergonomic sugar. Stateless functions on the module table (Python/Node-style) for common one-shot operations. These delegate to the UserData layer internally and never introduce their own design decisions.

This is a direct consequence of design principles 3 and 4.

### Shape

```lua
local mod = require("std:example")

-- UserData surface: construct, query, mutate
local obj = mod.Type.new("input")   -- or Type.parse, Type.from, etc.
obj:method()                        -- mirrors Rust method
obj:set_field("value")              -- mutation where the Rust type is mutable
tostring(obj)                       -- __tostring for serialisation

-- Flat surface: one-shot convenience
mod.convenience_fn("input")         -- delegates to Type internally, returns string/table
```

### Rules

- The UserData type name matches the Rust type it wraps (`Url`, `Path`, `PathBuf`, `Map`).
- Constructor lives on a sub-table: `mod.Type.new()`, `mod.Type.parse()`, `mod.Type.from()`.
- Read methods on UserData return Lua-native values (strings, numbers, bools, tables). Methods that produce new instances of the same domain return UserData.
- Mutation methods follow Rust naming: `set_path`, `set_extension`, `push`, `pop`.
- Flat functions return plain strings or tables — never UserData. Exception: `mod.parse()` may return UserData as a convenience alias for `mod.Type.parse()`.
- Flat functions that overlap with UserData methods use the scripting-ecosystem convention (e.g., `.extname()` returns `".txt"` with dot like Node, while `Path:extension()` returns `"txt"` without dot like Rust).
- Modules that don't wrap a substantive type (e.g., `lib:base64` — encode/decode are stateless transforms) only need the flat surface. Don't invent a UserData type for the sake of the pattern.

---

## Rust

### mlua modules

Every library module exports a single `module` function that builds and returns the Lua table:

```rust
pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("encode", lua.create_function(encode)?)?;
    Ok(t)
}
```

The Rust functions and `UserData` impls are private; the returned table is the public surface. `mod.rs::install` registers the table under the appropriate namespace key (`std:foo`, `lib:foo`, `vnd:foo`), and `require` hands it back when scripts ask. There is no separate `install` entry point; modules don't set globals.

Behaviour reaches scripts in two shapes: free functions set on the table (`t.set("encode", ...)`), and methods on `UserData` handles returned from those functions (see _Resource handles_ below for when each applies).

### Return types for Lua-bound functions

**Infallible functions** return the value directly. Do not introduce a fake error slot.

```rust
// correct
fn encode(_: &Lua, input: mlua::String) -> mlua::Result<String> {
    Ok(STANDARD.encode(input.as_bytes()))
}
```

**Fallible functions** return `(Value, Value)` using the `ok` / `from_err` helpers from `crate::lua::sandbox`. The Lua caller receives `(result, nil)` on success and `(nil, "prefix: message")` on failure.

```rust
fn decode(lua: &Lua, input: mlua::String) -> mlua::Result<(Value, Value)> {
    match STANDARD.decode(input.as_bytes()) {
        Ok(b)  => ok(lua, lua.create_string(&b)?),
        Err(e) => from_err(lua, "base64.decode", e),
    }
}
```

Never use `ok` / `from_err` on a path that cannot fail. The `&Lua` parameter can be named `_` when it is not used.

### Error prefixes

The prefix passed to `from_err` must be `"module.function"` (the bare module name, no namespace). This is the string the caller uses to identify the error source, so keep it stable. `vnd:hyper`'s `get` errors with `hyper.get: ...`, not `vnd:hyper.get: ...` — the namespace is a registration-time concern, not a runtime identity.

### Resource handles

Modules that wrap OS resources expose them as `UserData` handles. Behaviour that operates on a single handle — reading, writing, closing — is bound as a method on the handle (`impl UserData for ReaderHandle { fn add_methods(...) { methods.add_method("read", ...) } }`), so call sites read like the Rust API: `reader:read(n)`, `server:close()`.

Operations that take multiple handles, or none, stay as free functions on the module table — e.g. `(require("std:io")).copy(reader, writer)`.

Handle types live in their natural home module, not a central `handles.rs`. Per ADR 0003 the canonical wrappers are `Box<dyn BufRead + Send>` and `Box<dyn Write + Send>`, defined in `lib.io` (mirroring how `std::io` owns those traits).

### Allowed dependencies

The choice of namespace already constrains what's appropriate:

- `std:*` modules may use the Rust standard library and crates listed on [blessed.rs](https://blessed.rs/crates).
- `lib:*` modules may use the Rust standard library, blessed.rs crates, and a focused vendored crate when the module's whole purpose is to wrap one. (`lib:base64` wrapping the `base64` crate is fine because the wrapper is itself the gap-filler; the wrapper must be thin.)
- `vnd:*` modules wrap exactly one upstream crate — the crate the namespace name refers to. Pulling in a second crate to "round out the API" turns it into a different module; split it.

A crate that is not in the standard library and not on blessed.rs (other than the single crate a `vnd:*` module is named after) requires explicit team sign-off before it appears in `Cargo.toml`.

This applies to transitive exposure too. If a crate's types, errors, or behaviour become observable through the Lua surface, that crate is part of the public contract of the module. Keep third-party surface area minimal.

### Constructor naming

Reach for a descriptive name before reaching for `new`. The signal that `new` is appropriate is when there is genuinely nothing interesting to say about construction — no parameter, no strategy being chosen.

| Situation | Use |
|---|---|
| Bare, default, nothing to configure | `new()` |
| Wrapping an existing value | `from_lua(lua)`, `from_file(p)` |
| Constructing from a config struct | `from_options(opts)` |
| Converting from raw data | `from_snapshot(b)` |
| Opinionated preset, no required args | `with_stdlib()`, `with_sandbox()` |
| Preset + one key parameter | `with_module(name, chunk)` |
| Natural type conversion | `impl From<T>` |
| Avoid | `new_with_*`, `new_from_*`, builder structs |

The gut-check: if you could replace `new` with a more descriptive name without it feeling redundant, you should. `LuaRuntime::new()` is fine. `LuaRuntime::new_with_stdlib()` is a smell — that should be `with_stdlib()`.

The moment `new` grows a meaningful parameter, it deserves a real name instead.

### No speculative abstractions

Add a helper only when three or more concrete call sites already exist. A two-line function repeated twice is clearer than an abstraction used twice.

---

## Luau

### Imports

Every script that uses our modules opens with `require` calls bound to local names. No ambient globals, no `lib.X` access. Group imports by namespace, alphabetical inside each group:

```lua
local fs   = require("std:fs")
local io   = require("std:io")
local net  = require("std:net")

local b64  = require("lib:base64")

local hyper = require("vnd:hyper")
local json  = require("vnd:serde_json")
```

Local binding name matches the module name (`net`, not `network`). Single source of truth: changing what `net` is means changing the require line.

### Test file structure

Tests live in `tests/` and are named `<module>.test.luau`. Each file opens with the test framework binding, then any module imports it needs:

```lua
local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local base64 = require("lib:base64")
```

The `_t` shim is only there because `test` would shadow the test function. Other modules bind directly under their natural name.

Group tests by function using nested `describe` blocks:

```lua
describe("base64", function()
    describe("encode", function()
        test("known value", function() ... end)
    end)
    describe("decode", function()
        test("known value", function() ... end)
        test("invalid input returns (nil, err)", function() ... end)
    end)
end)
```

The outer `describe` label is the module name (`"base64"`), not a path (`"lib.base64"`). The namespace is metadata — it doesn't appear in test descriptions.

### Capturing return values

**Infallible functions** return one value. Assign only that value:

```lua
local result = base64.encode("hello")
expect(result).toBe("aGVsbG8=")
```

**Fallible functions** return `(result, err)`. Always capture both:

```lua
local result, err = base64.decode("aGVsbG8=")
expect(err).toBeNil()
expect(result).toBe("hello")
```

Do not capture `err` from an infallible call to assert it is nil. That assertion is meaningless and obscures which functions can actually fail.

### Coverage requirement

Every exported function needs at minimum:
- One test for the happy path.
- One test per distinct error condition.
- An empty-string or zero-value test where applicable.

---

## CI and distribution

- **GitHub Actions** (`.github/workflows/ci.yml`) runs on every push to `main` and every PR. Tests, benchmarks, and release builds run across Linux, macOS, and Windows.
- **Install scripts** (`scripts/install.sh`, `scripts/install.ps1`) follow the Bun distribution pattern — `curl | bash` on Unix, `irm | iex` on Windows. They download pre-built binaries from GitHub Releases.
- Release artifacts are named `neoc-<target>` where `<target>` is the Rust target triple (e.g. `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`).

---

## Documentation

See [`docs/styleguide/documentation-styleguide.md`](docs/styleguide/documentation-styleguide.md) for documentation conventions (page types, voice, formatting, templates).
