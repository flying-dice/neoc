# neoc

A fast, lightweight [Luau](https://luau.org/) runtime (Roblox's Lua 5.1 derivative, embedded through mlua). One binary: Lua scripts plus a curated set of Rust-backed modules, with nothing to install at run time.

`neoc` runs Lua in a VM with the unsafe globals stripped (`loadstring`, `os`, `debug`, `getfenv`, `setfenv`, ...). Scripts reach the outside world only through registered modules. This is a curated surface, not a security boundary.

neoc used to be a language that compiled to Lua. That is gone; the runtime is the product. The last state of the language is preserved under the `neoc-lang-final` tag.

## Install

```sh
# From source (any platform with the pinned Rust toolchain)
cargo install --locked --path crates/neoc
```

Releases ship Linux x86_64 and Windows x86_64 binaries. The install scripts download them from GitHub Releases:

```sh
# Linux x86_64
curl -fsSL https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.sh | bash

# Windows x86_64 (PowerShell)
irm https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.ps1 | iex
```

While the repository is private, the raw script URL, the GitHub API and the release downloads need authentication, so the scripts only work once the repository is public (or with a download you authenticate yourself). `NEOC_INSTALL_DIR` (default `~/.neoc/bin`) sets where the binary goes. `NEOC_VERSION=vX.Y.Z` installs a specific release.

## Run

```sh
neoc run script.lua                   # one script
neoc run one.lua two.lua              # several, in sequence
neoc run 'tests/**/*.lua'             # globs expand
neoc run -e 'print(1 + 2)'            # inline snippet
neoc run script.lua -- a b            # arguments reach the script as _NEOC_ARGS
```

```lua
-- notes.lua
local fs   = require("std:fs")
local json = require("vnd:serde_json")

local doc = assert(json.from_str('{"name": "neoc", "tags": ["lua", "rust"]}'))

fs.write("notes.json", json.to_string_pretty(doc))
print("saved " .. doc:get("name"))
```

## Modules

`require("namespace:name")` is the only way in. There are no ambient globals and no path search; an unknown name errors with the catalogue.

| Namespace | What | Modules |
|---|---|---|
| `std:` | mirrors Rust's standard library | `collections`, `env`, `fs`, `io`, `net`, `task`, `thread`, `time`, `workers` |
| `lib:` | curated helpers and gap-fillers | `archive`, `args`, `base64`, `cache`, `chrono`, `compile`, `compress`, `crypto`, `csv`, `diff`, `glob`, `hash`, `hex`, `hmac`, `html`, `http`, `ini`, `json`, `log`, `mime`, `pkey`, `random`, `regex`, `semver`, `tempfile`, `template`, `test`, `text`, `unicode`, `url`, `vcr` |
| `vnd:` | one vendored crate each, named after it | `hyper`, `jsonschema`, `quick_xml`, `rusqlite`, `serde_json`, `serde_yaml`, `sqlx_mysql`, `sqlx_postgres`, `testcontainers_modules`, `toml` |

The source in [`crates/neoc-vm/src/lua/`](crates/neoc-vm/src/lua) is the catalogue.

- `lib:test` is the test runner: a script named `*.test.lua` or `*.spec.lua` runs under it when passed to `neoc run`.
- `lib:compile` bundles a script and the runtime into a standalone executable.

## Language

Scripts are Lua 5.1 syntax running on Luau: the language and the base library are Luau's, not LuaJIT's, which neoc ran before NEOC-137. What a script written for LuaJIT can hit:

**Syntax LuaJIT accepted and Luau rejects.** The bitwise and logical operators `&`, `|`, `<<`, `>>`, `!=`, `&&`, `??` and `?.`, and `a ~= b` as a statement (xor-assign); `goto` and labels; hex floats (`0x.8`, `0xA.8p1`) and the `LL`/`ULL` suffixes; non-ASCII identifiers (`local café`: Luau identifiers are ASCII). A leading `#!` line and a UTF-8 byte-order mark are not Luau syntax either, but neoc's loader drops both before it loads a script (line numbers stay), so a script with a shebang runs as before. Luau accepts more than LuaJIT did (`//`, `+=`, `continue`, type annotations, `for k, v in t` over a table), so a script is still valid Lua 5.1 only if it avoids those.

**Library differences.**
- `bit` is `bit32` (unsigned results; a shift of 32 or more gives 0).
- `string.format("%s", v)` takes strings and numbers only: `nil`, booleans, tables and `__tostring` objects raise `invalid argument #2 to 'format' (string expected, got table)`. Luau's `%*` formats any value through `tostring` and its `__tostring`, and is the replacement for `%s` with a value that is not a string. `%a` is not a format (`invalid option '%a'`), and `%q` writes `\000` for a NUL.
- `tostring` of an object whose `__tostring` returns something that is not a string raises (`'__tostring' must return a string`; LuaJIT gave `nil`). `assert(false, <table>)` needs a string message.
- `string.rep` ignores a separator. `string.find("abc", "", 10)` is `nil` (LuaJIT: `4`). A `gsub` replacement string with a lone `%`, or a `%` followed by anything other than a digit or `%` (`"a%"`, `"100%"`, `"%-"`), raises `invalid use of '%' in replacement string` (`"%%"` is a literal `%` and works).
- `#` of a table with holes is the border Luau finds: `#{1, nil, 3}` and `select('#', unpack({1, nil, 3}))` are `3` (LuaJIT: `1`), so `table.sort({3, nil, 1})` raises `attempt to compare nil < number`. `#t` honours `__len`. `unpack({}, 1, 100000)` returns 100000 values (LuaJIT: `too many results to unpack`).
- A numeric `for` with a step of 0 runs zero times (LuaJIT looped for ever). There are no proper tail calls: `f(1000000)` that tail-calls itself overflows the stack. Recursion through a metamethod, a `string.gsub` callback or a `table.sort` comparator stops at about 200 levels with `C stack overflow`.
- `tostring(1e15)` is `1000000000000000` (shortest round-trip, not `%.14g`); `tonumber("0b11")` is `nil`; `_VERSION` is `Luau 0.740`; `tostring(print)` is an address. `math.random(0)` and `math.random(5, 1)` raise `invalid argument ... (interval is empty)` (LuaJIT answered a number); `math.random(1e300)` answers an integer.
- Error messages use Luau's wording (`attempt to call a nil value`, `attempt to perform arithmetic (add) on number and nil`, `invalid argument #1 to 'select'`) and name no variable. A failing `xpcall` handler is not run again: `xpcall` answers `error in error handling`.
- There is no `coroutine.close` in `neoc run` (Luau has one, and it would kill a task's thread); a script environment has one that closes only coroutines the script made. `typeof`, `rawlen` and Luau's extra `table`, `string` and `math` functions are there.

A wait (`task.wait`, I/O) suspends inside `pcall`, `xpcall`, `__call` and `for ... in` iterator functions, and is refused with "attempt to yield across metamethod/C-call boundary" in every other metamethod, in `tostring`, and in `string.gsub` and `table.sort` callbacks.

## Editor types

This repository contains [Luau](https://luau.org) declarations for every module in [`types/neoc.d.luau`](types/neoc.d.luau), one definitions file generated from the modules' own signatures: a table type per module (`lib_json` for `lib:json`), a type per resource type (`lib_json_Object`), and an overloaded `require` that gives `require("lib:json")` the type of that module. [luau-lsp](https://github.com/JohnnyMorganz/luau-lsp) gives completion, hover docs and type checks once it loads that file; a `require` of a name that is not a module is a type error, as it is at run time. The declarations are a definitions file, not a module: nothing loads them at run time.

```sh
luau-lsp analyze --definitions path/to/neoc/types/neoc.d.luau script.luau   # type-check from the command line
```

In VS Code with the [Luau Language Server](https://marketplace.visualstudio.com/items?itemName=JohnnyMorganz.luau-lsp) extension, these workspace settings (`.vscode/settings.json`) load the file:

```jsonc
{
  "luau-lsp.platform.type": "standard",
  "luau-lsp.types.definitionFiles": ["path/to/neoc/types/neoc.d.luau"]
}
```

`standard` keeps the extension from loading Roblox's API. Other editors start the server as `luau-lsp lsp --definitions=path/to/neoc/types/neoc.d.luau`. Put `--!strict` at the top of a script, or `{ "languageMode": "strict" }` in a [`.luaurc`](.luaurc) (this repository has one), to have strict mode flag an optional result used without a check. The resource types are exported: `local o: lib_json_Object = json.object()`. The declarations live in the source tree, not in the release binaries or the Docker image. `cargo run -p neoc-vm --example luau_decls` regenerates them.

The declarations describe Luau. The engine neoc runs scripts on today is LuaJIT, which does not parse Luau-only syntax (annotations, `continue`, compound assignment, string interpolation); the engine switch is tracked as NEOC-137. The calls and results are typed either way.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) covers building, testing, adding a module and releasing. [CLAUDE.md](CLAUDE.md) holds the module design rules.
