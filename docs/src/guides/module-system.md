# The module system

How neoc resolves `require`, the three namespaces, and their rules.

## How `require` resolves

Standard Lua `require` is replaced. Accepts `"namespace:name"` — colon-separated pair where namespace is `std`, `lib`, or `vnd`.

```lua
local fs   = require("std:fs")
local test = require("lib:test")
local json = require("vnd:serde_json")
```

Resolver is a flat lookup table built once at engine boot. No path search, no file I/O, no `package.path` fallback. Exact name match or failure.

### Failure mode

Unregistered name raises immediately:

```text
require: unknown module "vnd:hyperz" — expected std:<name>, lib:<name>, or vnd:<name>
```

Surfaces at point of call, not later on missing function. Typos visible immediately.

### Identity

`require` returns the module table itself, not a copy. Multiple callers receive the same table. Writes mutate for all callers. Modules typically frozen at registration.

## The three namespaces

### `std:*` — standard-library mirrors

Mirrors Rust's standard library. Naming and shape chosen so Rust-fluent readers recognise without explanation: `std:collections` → `std::collections`, `std:fs` → `std::fs`, `std:net` → `std::net`.

Exceptions: `std:workers` (worker pool substrate) admitted when broadly needed with no clean home elsewhere.

Catalogue: [`std:*`](/std/).

### `lib:*` — blessed gap-fillers

Not direct mirrors of a Rust crate. Two kinds: *gap-fillers* (`lib:base64`) and *project-native* modules (`lib:json`, `lib:test`).

Qualifies when: well-defined purpose, stable surface, genuinely useful to most scripts.

Catalogue: [`lib:*`](/lib/).

### `vnd:*` — vendored crates

Each module wraps exactly one upstream Rust crate. Module name matches crate name verbatim.

```lua
local hyper      = require("vnd:hyper")
local serde_json = require("vnd:serde_json")
```

No renaming, no aliasing, no merging. Non-negotiable: scripts reach for a crate by its real name and find real documentation without indirection.

Catalogue: [`vnd:*`](/vnd/).

## Module construction

Every module exports `pub fn module(lua: &Lua) -> mlua::Result<Table>`. Called once at registration, result stored in lookup map. Scripts never see construction — only the table via `require`.

For contributors writing modules, not script authors.

## See also

- [The sandboxing model](/guides/sandboxing-model)
- [Error conventions](/guides/error-conventions)
- [`std:*`](/std/)
- [`lib:*`](/lib/)
- [`vnd:*`](/vnd/)
