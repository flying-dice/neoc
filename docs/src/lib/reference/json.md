# `lib:json`

**Stable**

Structural handles for JSON documents. Two userdata types — Object and Array — plus a `null` sentinel distinct from Lua `nil`. Constructed via [`json.object`](#json-object)/[`json.array`](#json-array), or received populated from [`vnd:serde_json`](/vnd/reference/serde_json).

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/json.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/json.rs) |
| Tests | [`tests/lib/json.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/json.test.luau) |
| Stability | Stable |
| Sharable | No — Object, Array, `json.null` are not Sendable |

## Syntax

```lua
local json = require("lib:json")
```

## Description

Explicit access — no `obj.field` sugar, no `arr[1]` indexing. Reads via `:get(...)`, writes via `:set(...)`/`:push(...)`, presence via `:has(...)`, length via `:len()`. Keeps document semantics legible under Luau strict mode.

Not Sendable. Cannot appear in `shared` table of `thread.spawn`. Serialise to string via `vnd:serde_json` and pass through `args` for cross-worker transfer.

## Module functions

#### json.object

```lua
json.object(): Object
```

Returns empty mutable Object handle.

#### json.array

```lua
json.array(): Array
```

Returns empty mutable Array handle.

#### json.null

```lua
json.null: any
```

Unique userdata sentinel for JSON `null` (distinct from Lua `nil` = "key absent"). Comparable by `==`. Same value within and across workers — `serde_json.from_str("null") == json.null`. Not Sendable.

## Object methods

`Object:get(...path)`
:   Walks `path` through nested Objects/Arrays. Segments: string (key) or integer (1-based index). Returns: Lua scalar for leaves, `json.null` for explicit nulls, Object/Array handle for containers, `nil` if missing.

`Object:set(key, value)`
:   Assigns `value` at `key`. Accepts: scalars, plain tables (recursive convert), Object/Array handles, `json.null`.

`Object:has(key)`
:   `true` if key present (including when value is `json.null`).

`Object:keys()`
:   Array of keys. Order not guaranteed.

`Object:len()`
:   Number of entries.

`Object:to_table()`
:   Deep Lua-table copy. Mutations to returned table don't affect document.

## Array methods

`Array:get(...path)`
:   Same path-walking as Object. Leading segment is 1-based integer.

`Array:set(index, value)`
:   Assigns at 1-based index.

`Array:push(value)`
:   Appends to end.

`Array:len()`
:   Number of elements.

## Errors

`:get(...)` returns `nil` on missing path (no raise). Type mismatches at write time raise synchronously.

## Examples

### Parse, navigate, mutate

```lua
local json       = require("lib:json")
local serde_json = require("vnd:serde_json")

local doc = serde_json.from_str('{"name":"Alice","age":30,"tags":["a","b"]}')

print(doc:get("name"))           -- Alice
print(doc:get("tags", 2))        -- b
print(doc:has("missing"))        -- false

doc:set("city", "Springfield")
doc:get("tags"):push("c")
print(serde_json.to_string(doc)) -- {"age":30,"city":"Springfield",...}
```

### Null vs absent

```lua
local json       = require("lib:json")
local serde_json = require("vnd:serde_json")

local v = serde_json.from_str('{"explicit":null}')

print(v:has("explicit"))               -- true
print(v:get("explicit") == json.null)  -- true
print(v:has("missing"))                -- false
print(v:get("missing"))                -- nil
```

### Nested handle observes mutation

```lua
local serde_json = require("vnd:serde_json")

local doc    = serde_json.from_str('{"nested":{"x":1}}')
local nested = doc:get("nested")

doc:set("nested", { x = 99, y = 100 })

print(nested:get("x"))   -- 99
print(nested:get("y"))   -- 100
```

### Building from scratch

```lua
local json = require("lib:json")

local arr = json.array()
arr:push(1)
arr:push("two")
arr:push(true)

local obj = json.object()
obj:set("name", "Alice")
obj:set("scores", arr)
obj:set("missing", json.null)
```

## Acceptance

Exercised by [`tests/lib/json.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/json.test.luau):

1. **Constructors.** `json.object()`/`json.array()` → empty, `:len()` == 0.
2. **Round-trip.** `serde_json.from_str('{"a":1}')` → Object, `:get("a")` == 1.
3. **Variadic path walk.** `'{"user":{"tags":["x","y"]}}'` → `:get("user","tags",2)` == `"y"`. Missing → `nil`.
4. **Null vs absent.** `'{"explicit":null}'` → `:has("explicit")` true, `:get("explicit") == json.null`. `:has("missing")` false, `:get("missing")` nil.
5. **Scalar JSON values.** `'"hi"'` → `"hi"`. `'42'` → 42. `'true'` → true. `'null'` → `json.null`.
6. **Nested handle observes mutation.** Handle from `:get("nested")` reflects changes after `doc:set("nested", ...)`.
7. **Array ops.** `:push` increments `:len()`. `:get(n)` returns element. `:set(n, y)` overwrites.
8. **Object keys.** `#obj:keys() == obj:len()`.
9. **`to_table` snapshot.** Table mutations don't affect document.
10. **Not Sendable.** Object/Array/`json.null` in `shared` → raises.

## See also

- [`vnd:serde_json`](/vnd/reference/serde_json) — parser/serialiser
- [`std:thread`](/std/reference/thread) — Sendable protocol
- [`vnd:jsonschema`](/vnd/reference/jsonschema) — schema validation
- [The module system](/guides/module-system)
