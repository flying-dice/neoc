# `lib:json` module

**Stable**

The **`lib:json`** module exposes structural handles for working with JSON documents in Luau. It provides two userdata types — Object and Array — and a `null` sentinel distinct from Lua `nil`. Documents are constructed with [`json.object`](#json-object) or [`json.array`](#json-array), or received populated from a parser such as [`vnd:serde_json`](/vnd/reference/serde_json).

| Property | Value |
|---|---|
| Namespace | `lib` |
| Source | [`src/lua/lib/json.rs`](https://github.com/flying-dice/neoc/blob/main/src/lua/lib/json.rs) |
| Tests | [`tests/lib/json.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/json.test.luau) |
| Stability | Stable |
| Sharable across workers | No — Object, Array, and [`json.null`](#json-null) do not implement the Sendable protocol and cannot cross worker boundaries |

## Syntax

```lua
local json = require("lib:json")
```

The returned table exposes module functions and the `null` sentinel.

## Description

`lib:json` is *explicit* about access. There is no `obj.field` sugar, no `arr[1]` indexing, and no implicit `#arr` length. Reads go through `:get(...)`, writes through `:set(...)` or `:push(...)`, presence checks through `:has(...)`, and length through `:len()`. The shape keeps document semantics legible to readers and consistent under Luau strict mode, which cannot statically check dynamic JSON shapes.

Object, Array, and the [`json.null`](#json-null) sentinel are *not* Sendable and cannot appear in the `shared` table of [`thread.spawn`](/std/reference/thread). Each is bound to the Lua VM that constructed or parsed it. Scripts that need to pass JSON data to a spawned worker should serialise it to a string with [`vnd:serde_json`](/vnd/reference/serde_json) and pass the string via the `args` field.

## Module functions

#### json.object

```lua
json.object(): Object
```

Returns an empty mutable Object handle.

#### json.array

```lua
json.array(): Array
```

Returns an empty mutable Array handle.

#### json.null

```lua
json.null: any
```

A unique userdata sentinel representing JSON `null`, distinct from Lua `nil` (which represents "key absent"). Comparable by `==`. Always the same value within and across workers — `serde_json.from_str("null") == json.null` holds. Not sharable across worker boundaries; it does not implement the Sendable protocol.

## Object methods

`Object:get(...path)`
:   Walks `path` through nested Objects and Arrays. Each segment is a string (object key) or integer (1-based array index). Returns a Lua scalar for terminal leaves, [`json.null`](#json-null) for explicit JSON nulls, a fresh Object or Array handle for nested containers, or `nil` if any segment is missing. With no arguments, returns the receiver.

`Object:set(key, value)`
:   Assigns `value` at `key`. Accepts Lua scalars, plain Lua tables (recursively converted), other Object or Array handles, and [`json.null`](#json-null). Replacing a container preserves identity for handles already pointing into the old subtree at the same path — they reflect the new contents.

`Object:has(key)`
:   `true` if `key` is present (including when its value is [`json.null`](#json-null)), `false` otherwise. Distinguishes "key absent" from "key explicitly null".

`Object:keys()`
:   Returns an array of the Object's keys. Order is not guaranteed; sort explicitly if needed.

`Object:len()`
:   Returns the number of entries in the Object.

`Object:to_table()`
:   Returns a deep Lua-table copy of the Object's current contents. Mutations to the returned table do not flow back into the document.

## Array methods

`Array:get(...path)`
:   Same path-walking semantics as [`Object:get`](#object-methods). The leading segment is a 1-based integer index.

`Array:set(index, value)`
:   Assigns `value` at the 1-based `index`. Accepts the same value shapes as [`Object:set`](#object-methods).

`Array:push(value)`
:   Appends `value` to the end of the array.

`Array:len()`
:   Returns the number of elements in the array.

## Errors

`lib:json` does not raise on missing-path reads. `:get(...)` returns `nil` when any segment fails to resolve. Type mismatches at write time follow Luau's normal error path — passing a function to `:set` raises synchronously.

[`json.null`](#json-null) is comparable with `==` but is *not* sharable across worker boundaries.

## Examples

### Parsing, navigating, and mutating

The following example parses a JSON string, reads nested fields, mutates the document, and serialises the result.

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

### Distinguishing null from absent

The following example shows how `:has` and `:get` together distinguish a key whose value is explicitly `null` from a key that is missing entirely.

```lua
local json       = require("lib:json")
local serde_json = require("vnd:serde_json")

local v = serde_json.from_str('{"explicit":null}')

print(v:has("explicit"))               -- true
print(v:get("explicit") == json.null)  -- true
print(v:has("missing"))                -- false
print(v:get("missing"))                -- nil
```

### A nested handle observing parent mutation

A handle obtained from `:get` continues to reflect the live document after the parent replaces the subtree.

```lua
local serde_json = require("vnd:serde_json")

local doc    = serde_json.from_str('{"nested":{"x":1}}')
local nested = doc:get("nested")

doc:set("nested", { x = 99, y = 100 })

print(nested:get("x"))   -- 99
print(nested:get("y"))   -- 100
```

### Building a document from scratch

The following example constructs an Array, then an Object holding the array and other values, then writes [`json.null`](#json-null) into a third field.

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

The following scenarios must hold. They are exercised by the integration tests under [`tests/lib/json.test.luau`](https://github.com/flying-dice/neoc/blob/main/tests/lib/json.test.luau).

1. **Constructors.** [`json.object`](#json-object) and [`json.array`](#json-array) return empty handles whose `:len()` is `0`.
2. **Round-trip with parser.** `serde_json.from_str('{"a":1}')` returns an Object handle for which `:get("a")` is `1`.
3. **Variadic path walk.** For `'{"user":{"tags":["x","y"]}}'`, `:get("user", "tags", 2)` returns `"y"`. Any missing segment yields `nil`.
4. **Null versus absent.** For `'{"explicit":null}'`, `:has("explicit")` is `true` and `:get("explicit") == json.null`. `:has("missing")` is `false` and `:get("missing")` is `nil`.
5. **Scalar JSON values.** `serde_json.from_str('"hi"')` returns the Lua string `"hi"`. `'42'` returns `42`. `'true'` returns `true`. `'null'` returns [`json.null`](#json-null).
6. **Nested handle observes mutation.** A handle obtained via `doc:get("nested")` continues to reflect the live document after `doc:set("nested", ...)` replaces the subtree.
7. **Array push, set, len.** `arr:push(x)` increases `:len()` by one. `arr:get(n)` returns the element at the 1-based index `n`. `arr:set(n, y)` overwrites that element.
8. **Object keys.** `obj:keys()` returns an array containing every key present, with `#obj:keys() == obj:len()`.
9. **`to_table` is a snapshot.** Mutating the table returned by `:to_table()` does not affect the underlying document.
10. **Not sharable across workers.** Passing an Object, Array, or [`json.null`](#json-null) in the `shared` table of [`thread.spawn`](/std/reference/thread) raises an error. Cross-worker JSON data must be serialised to a string via [`vnd:serde_json`](/vnd/reference/serde_json) and passed through `args`.

## See also

- [`vnd:serde_json`](/vnd/reference/serde_json) — Parser and serialiser that produces and consumes `lib:json` handles.
- [`std:workers`](/std/reference/workers) — Process-level coordination (shutdown signal).
- [`std:thread`](/std/reference/thread) — Worker spawning and the Sendable protocol for cross-worker state.
- [`vnd:jsonschema`](/vnd/reference/jsonschema) — Schema validation for JSON documents.
- [The module system](/guides/module-system) — How `require("lib:json")` resolves.
