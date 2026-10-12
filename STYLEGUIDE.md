# Style Guide

Code conventions. Binding — discuss changes before deviating.

---

## Design principles

Priority order:

1. **No surprises.** Match industry consensus. If Rust, Go, Python, Node agree → follow them.
2. **No footguns.** If a design needs a warning paragraph, redesign it.
3. **Mirror Rust first.** Primary API: `UserData` types mirroring `std::*` 1:1 (same methods, semantics).
4. **Supplement with scripting ergonomics.** Flat functions (Python/Node-style) layered on top. Delegate to UserData. Never introduce own design decisions.

Precedence: 1 > 2 > 3 > 4. Applies to all namespaces.

---

## Module namespaces

No ambient globals. `require("namespace:name")` — strict spec, three namespaces.

| Namespace | Contains | Examples |
|---|---|---|
| `std:*` | Rust std mirrors + tightly-scoped extensions | `collections`, `fs`, `io`, `net`, `thread`, `workers` |
| `lib:*` | Gap-fillers + custom modules | `base64`, `test`, `vcr` |
| `vnd:*` | Vendored crates. Name matches upstream. | `hyper`, `serde_json`, `rusqlite` |

Picking:
- Clear `std::*` analog → `std:*` (same name)
- Our code or small helper crate → `lib:*`
- Recognisable crate from `Cargo.toml` → `vnd:*` (exact name)

Registry built once at boot in `crates/neoc-vm/src/lua/lib/mod.rs::install`.

---

## Module API pattern — UserData + scripting

Two surfaces per module wrapping a substantive type:

1. **UserData** — foundation. Mirror Rust API 1:1.
2. **Flat functions** — ergonomic sugar. Delegate to UserData internally.

### Shape

```lua
local mod = require("std:example")

-- UserData: construct, query, mutate
local obj = mod.Type.new("input")
obj:method()
obj:set_field("value")
tostring(obj)

-- Flat: one-shot convenience
mod.convenience_fn("input")
```

### Rules

- UserData type name matches Rust type (`Url`, `Path`, `Map`).
- Constructor on sub-table: `mod.Type.new()`, `.parse()`, `.from()`.
- Read methods return Lua-native values. Domain instances return UserData.
- Mutation follows Rust naming: `set_path`, `push`, `pop`.
- Flat functions return strings/tables, never UserData (exception: `mod.parse()` → UserData alias).
- Flat functions use scripting convention (`.extname()` → `".txt"` with dot); UserData mirrors Rust (`:extension()` → `"txt"` without dot).
- Stateless modules (`lib:base64`) only need flat surface.

---

## Rust

### mlua modules

Single `module` function returning Lua table:

```rust
pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("encode", lua.create_function(encode)?)?;
    Ok(t)
}
```

Private Rust functions/UserData impls. Table is the public surface. No globals.

### Return types

**Infallible** — return value directly:

```rust
fn encode(_: &Lua, input: mlua::String) -> mlua::Result<String> {
    Ok(STANDARD.encode(input.as_bytes()))
}
```

**Fallible** — return `(Value, Value)` via `ok`/`from_err`:

```rust
fn decode(lua: &Lua, input: mlua::String) -> mlua::Result<(Value, Value)> {
    match STANDARD.decode(input.as_bytes()) {
        Ok(b)  => ok(lua, lua.create_string(&b)?),
        Err(e) => from_err(lua, "base64.decode", e),
    }
}
```

### Error prefixes

`"module.function"` format. Bare module name, no namespace. Stable string.

### Resource handles

OS resources → `UserData` handles with methods. Single-handle ops = methods. Multi-handle ops = free functions on module table.

### Allowed dependencies

- `std:*` — Rust std + [blessed.rs](https://blessed.rs/crates)
- `lib:*` — above + one focused vendored crate when that IS the module
- `vnd:*` — exactly one upstream crate (the named one)

Non-blessed crate requires team sign-off.

### Constructor naming

| Situation | Use |
|---|---|
| Bare, default | `new()` |
| Wrapping existing value | `from_lua()`, `from_file()` |
| Config struct | `from_options()` |
| Raw data | `from_snapshot()` |
| Opinionated preset | `with_stdlib()`, `with_sandbox()` |
| Avoid | `new_with_*`, `new_from_*`, builders |

If `new` could be more descriptive without feeling redundant → use the descriptive name.

### No speculative abstractions

Helper only when 3+ call sites exist.

---

## Lua

### Imports

```lua
local fs   = require("std:fs")
local io   = require("std:io")

local b64  = require("lib:base64")

local hyper = require("vnd:hyper")
```

Grouped by namespace, alphabetical. Local name matches module name.

### Test file structure

`crates/neoc/tests/<module>.test.lua`:

```lua
local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local base64 = require("lib:base64")

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

Outer `describe` = module name (no namespace).

### Capturing returns

Infallible — one value. Fallible — capture both `(result, err)`. Don't capture `err` from infallible calls.

### Coverage requirement

Per exported function:
- One happy-path test
- One test per distinct error condition
- Empty-string/zero-value test where applicable

---

## CI and distribution

- GitHub Actions on push to `main` and `develop`.
- Install scripts: `curl | bash` (Unix), `irm | iex` (Windows). Download from GitHub Releases.
- Artifacts: `neoc-<target>` (Rust target triple).

---

## Documentation

See [`docs/styleguide/documentation-styleguide.md`](docs/styleguide/documentation-styleguide.md).
