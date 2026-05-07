# The module system

This guide explains how neoc resolves `require` calls, the three namespaces a module may belong to, and the rules each namespace follows. Familiarity with Lua's standard `require` and basic notions of namespacing is assumed.

## How `require` resolves

The standard Lua `require` is replaced inside the engine. The replacement accepts a single argument of the form `"namespace:name"` — a colon-separated pair where the namespace is one of `std`, `lib`, or `vnd`, and the name is the module's identifier within that namespace.

```lua
local fs   = require("std:fs")
local test = require("lib:test")
local json = require("vnd:serde_json")
```

The resolver is a flat lookup table built once at engine boot. There is no path search, no file I/O, no fallback to the standard `package.path`. A `require` call either finds a registered module by exact name or it fails.

### Failure mode

A `require` call for an unregistered name raises an error that names the requested module and lists the three valid namespace prefixes:

```text
require: unknown module "vnd:hyperz" — expected std:<name>, lib:<name>, or vnd:<name>
```

The error surfaces at the point of the call, not later when a missing function is invoked. This makes typos in module names visible immediately.

### Identity of returned tables

`require` returns the module table itself, not a copy. Multiple callers — and multiple workers — receive handles to the same table. Writing into a module table observably mutates it for every caller. This matches Lua's standard `require` contract, with the caveat that modules in neoc are typically frozen at registration time and writes are rare.

## The three namespaces

Every module belongs to exactly one namespace, picked at registration. The choice encodes what kind of capability the module provides and what rules it follows.

### `std:*` — standard-library mirrors

The `std:*` namespace contains modules that mirror Rust's standard library. The naming and shape of each module's surface is chosen so a reader fluent in Rust's `std::*` recognises it without explanation: `std:collections` mirrors `std::collections`, `std:fs` mirrors `std::fs`, `std:net` mirrors `std::net`, and so on.

A small number of `std:*` modules cover capabilities that Rust's standard library does not — `std:workers`, the worker pool substrate, is the principal example. These are admitted to `std:*` only when they are broadly needed and have no clean home elsewhere.

For the catalogue, see [`std:*`](/std/).

### `lib:*` — blessed gap-fillers and project-native modules

The `lib:*` namespace contains modules that are not direct mirrors of a Rust crate or standard module. Two kinds qualify: *gap-fillers* that plug a hole the standard library does not cover (such as `lib:base64`), and *project-native* modules written specifically for neoc (such as `lib:json` and `lib:test`).

A module qualifies for `lib:*` when its purpose is well-defined, its surface is stable, and it is genuinely useful to most scripts. Speculative or experimental functionality belongs elsewhere until it has earned a place here.

For the catalogue, see [`lib:*`](/lib/).

### `vnd:*` — vendored crates

The `vnd:*` namespace exposes individual Rust crates to Lua. Each module wraps exactly one upstream crate, and the module name matches the crate name verbatim.

```lua
local hyper      = require("vnd:hyper")        -- the `hyper` crate
local serde_json = require("vnd:serde_json")   -- the `serde_json` crate
```

There is no renaming, no aliasing, and no merging of multiple crates into a composite module. This naming rule is non-negotiable: it is what lets scripts reach for an upstream crate by its real name and find its real documentation without indirection.

For the catalogue, see [`vnd:*`](/vnd/).

## Module construction

Internally, every module exports a single Rust function with the signature `pub fn module(lua: &Lua) -> mlua::Result<Table>`. The engine calls this function once at registration time and stores the resulting table in the resolver's lookup map under the module's qualified name. Scripts never see the construction step; they see only the resulting table when they call `require`.

This structure is documented for contributors writing new modules. Scripts do not interact with it.

## See also

- [The sandboxing model](/guides/sandboxing-model) — How the module system fits into the broader sandbox.
- [Error conventions](/guides/error-conventions) — How modules signal failures.
- [`std:*`](/std/) — Standard-library mirrors.
- [`lib:*`](/lib/) — Project-native modules.
- [`vnd:*`](/vnd/) — Vendored crates.
