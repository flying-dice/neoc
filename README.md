# neoc

A fast, lightweight [LuaJIT](https://luajit.org/) runtime. One binary: Lua scripts plus a curated set of Rust-backed modules, with nothing to install at run time.

`neoc` runs Lua in a VM with the unsafe globals stripped (`load*`, `io`, `os`, `debug`, `ffi`, ...). Scripts reach the outside world only through registered modules. This is a curated surface, not a security boundary.

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

## Editor types

The runtime ships [LuaCATS](https://luals.github.io/wiki/annotations/) definitions for every module in [`types/luals/`](types/luals), one file per module named after its `require` key (`lib:json.lua`), generated from the modules' own signatures. Point [lua-language-server](https://luals.github.io/) at them for completion, hover docs and type checks of `require("lib:json")` and the rest:

```jsonc
// .luarc.json
{
  "runtime.version": "LuaJIT",
  "workspace.library": ["path/to/neoc/types/luals"]
}
```

This repository's own [`.luarc.json`](.luarc.json) does that. The file names contain a colon, which Windows file systems do not allow, so a Windows checkout needs WSL (or a copy of the folder made elsewhere). `cargo run -p neoc-vm --example luals` regenerates the folder.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) covers building, testing, adding a module and releasing. [CLAUDE.md](CLAUDE.md) holds the module design rules.
