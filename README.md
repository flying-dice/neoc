# neoc and vos

One repository, two products on one Rust workspace:

- **neoc**: a fast, lightweight [Luau](https://luau.org/) runtime (Roblox's Lua 5.1 derivative, embedded through mlua). One
  binary: Luau scripts plus a curated set of Rust-backed modules, with nothing to install at run time. Scripts reach the outside
  world only through `require("std:...")`, `require("lib:...")` and `require("vnd:...")`; the unsafe globals (`loadstring`,
  `os`, `debug`, ...) are stripped. A curated surface, not a security boundary. Code: `crates/neoc` (the CLI),
  `crates/neoc-vm` (the runtime library).
- **vos** (Virtual OS): an iPadOS-style virtual desktop for agents, a self-hosted home lab. A Rust host (`crates/vos-*`) runs
  apps written in typed Luau (`apps/`) on `neoc-vm`, serves a Svelte PWA (`client/`, with the UI kit in `packages/ui`) and speaks
  JSON-RPC (`/rpc`), AG-UI (`/agui`) and MCP (`/mcp`). Every client pairs as a device first.

[CLAUDE.md](CLAUDE.md) holds the rules and the layout, [CONTRIBUTING.md](CONTRIBUTING.md) the workflow and the pre-PR checks,
[apps/README.md](apps/README.md) how to write a vos app.

## Build, run and test

Prerequisites: `rustup` (the toolchain is pinned in `rust-toolchain.toml`; building Luau needs a C++ compiler). For vos also
[Bun](https://bun.sh), `darklua` 0.18.0 (`cargo install darklua --version 0.18.0 --locked`), `luau-lsp` 1.70.1, `stylua`,
`cargo-nextest` and `cargo-deny`.

```sh
# neoc
cargo build                                  # both products
cargo run -p neoc -- run script.lua          # run a script (several, a glob, or -e 'print(1 + 2)'; args after -- reach it as _NEOC_ARGS)
cargo install --locked --path crates/neoc    # install the binary
cargo test -p neoc -p neoc-vm                # the neoc tests

# vos
bun install
bun run dev                                  # build the apps, the host on :7400 and the client on :5173; pair with the printed code
bun run build && bun run build:apps && bun run host   # the same without hot reload; `bun run host pair` prints a new code

# everything, before a PR
bun run gate                                 # neoc checks, then lint, typecheck, test and e2e (see CONTRIBUTING.md)
```

Releases ship Linux x86_64 and Windows x86_64 `neoc` binaries (`scripts/install.sh`, `scripts/install.ps1`; `NEOC_INSTALL_DIR`
and `NEOC_VERSION` choose where and which). While the repository is private the downloads need authentication.

## neoc

```lua
-- notes.lua
local fs   = require("std:fs")
local json = require("vnd:serde_json")
local null = require("lib:json").null -- the JSON null sentinel is lib:json's; vnd:serde_json has none

-- a parse result is a handle, `null` or a scalar: cast a container to the handle you expect
local doc = assert(json.from_str('{"name": "neoc", "tags": ["lua", "rust"], "owner": null}')) :: lib_json_Object

local text = assert(json.to_string_pretty(doc))
assert(fs.write("notes.json", text))
print("saved " .. tostring(doc:get("name")))
if doc:get("owner") == null then
  print("no owner yet")
end
```

A container in a parse result (or a `get` result) is narrowed with a cast, as above: luau-lsp narrows the handle types to
nothing on `type(v) == "userdata"`, and `type(v) == "table"` is false at run time. A JSON `null` is the sentinel
`require("lib:json").null` (`vnd:serde_json` has no `null` of its own): test for it with `==`, as above, not by type,
because its type, `lib_json_JsonNull`, is structurally empty and any table satisfies it. `types/fixtures/readme.luau`
holds this example, checked in strict mode.

`require("namespace:name")` is the only way in: no ambient globals, no path search; an unknown name errors with the catalogue.

| Namespace | What | Modules |
|---|---|---|
| `std:` | mirrors Rust's standard library | `collections`, `env`, `fs`, `io`, `net`, `path`, `task`, `thread`, `time`, `workers` |
| `lib:` | curated helpers and gap-fillers | `archive`, `args`, `base64`, `cache`, `chrono`, `compile`, `compress`, `crypto`, `csv`, `diff`, `glob`, `hash`, `hex`, `hmac`, `html`, `http`, `ini`, `json`, `log`, `mime`, `pkey`, `random`, `regex`, `semver`, `tempfile`, `template`, `test`, `text`, `unicode`, `url`, `vcr` |
| `vnd:` | one vendored crate each, named after it | `hyper`, `jsonschema`, `quick_xml`, `rusqlite`, `serde_json`, `serde_yaml`, `sqlx_mysql`, `sqlx_postgres`, `testcontainers_modules`, `toml` |

The source in [`crates/neoc-vm/src/lua/`](crates/neoc-vm/src/lua) is the catalogue. `lib:test` is the test runner (a script
named `*.test.lua` or `*.spec.lua` runs under it); `lib:compile` bundles a script and the runtime into a standalone executable.

### HTTP client defaults

`vnd:hyper`'s `get`, `post` and `request` are safe with no options (NEOC-259, after `URLSession` and Swift's async-http-client): a
request gives up after 10 s of connecting and after 60 s with no data moving (`connect_timeout`, `idle_timeout`; `timeout`
bounds the whole request and has no default), follows up to 16 redirects without handing `Authorization`, `Cookie`,
`Proxy-Authorization` or `Origin` to another origin (`follow_redirects`; a credential header of your own, such as `X-Api-Key`,
does go to every hop), and reads at most 64 MiB of body (`max_body`). It never decompresses. A header sent twice is one value
joined with `", "` (`set-cookie` with `"\n"`, so cookies can be split), and non-UTF-8 header bytes become U+FFFD, for a
`hyper.serve` handler's request too. A failure is `nil, "hyper.<fn>: <kind>: <message>"` and `hyper.error_kind(err)` returns the kind (`timed_out`,
`connect`, `tls`, `invalid_url`, `too_large`, `too_many_redirects`, ...). Where a redirect leads is not checked (a public URL can send
the request to the local network): `follow_redirects = false` returns the 3xx. The options and kinds are documented in
[`types/neoc.d.luau`](types/neoc.d.luau); vos apps have the same module, described in [`apps/README.md`](apps/README.md#web-requests).

### Language

Luau: Lua 5.1 syntax plus type annotations, `continue`, compound assignment, string interpolation and `if` expressions, with
Luau's base library (not LuaJIT's, which neoc ran before NEOC-137). What a script written for LuaJIT can hit:

- Rejected syntax: the operators `&`, `|`, `<<`, `>>`, `!=`, `&&`, `??`, `?.`; `goto` and labels; hex floats and the `LL`/`ULL`
  suffixes; non-ASCII identifiers. A leading `#!` line and a byte-order mark are dropped by neoc's loader.
- `bit` is `bit32`. `string.format("%s", v)` takes strings and numbers only (`%*` formats any value through `tostring`); `%a`
  is not a format. `#t` of a table with holes is the border Luau finds. There are no proper tail calls, and recursion through
  a metamethod, a `string.gsub` callback or a `table.sort` comparator stops at about 200 levels (`C stack overflow`).
- Error messages use Luau's wording and name no variable. `coroutine.close` is absent in `neoc run`.
- A wait (`task.wait`, I/O) suspends inside `pcall`, `xpcall`, `__call` and `for ... in` iterator functions, and is refused with
  "attempt to yield across metamethod/C-call boundary" in other metamethods, in `tostring`, and in `string.gsub` and
  `table.sort` callbacks.
- [`buffer`](https://luau.org/library#buffer-library) is Luau's mutable block of bytes, for binary formats and protocols. Its limits are
  accepted: a buffer has a fixed size (it cannot grow), reads and writes are little-endian only, and there are no
  pack/unpack format strings (`string.pack` and `string.unpack` have those). A size that is negative, not a number or over
  Luau's limit of 1 GiB is an error (a numeric string converts and a fraction is truncated, as for any Lua number
  argument). One call can allocate up to 1 GiB, as building a long string can: neoc has no memory budgets. A serializer
  (`vnd:serde_json`, `vnd:toml`, a `std:thread` message, ...) refuses a buffer rather than turn it into an array of
  numbers: convert it first (`buffer.tostring`). A `vnd:hyper` request body can be a buffer, and `hyper.get`, `hyper.post`
  and `hyper.request` return an error naming any option they do not read. vos apps do not have `buffer` yet.

### Editor types

[`types/neoc.d.luau`](types/neoc.d.luau) holds Luau declarations of every module, generated from the modules' own signatures
(`cargo run -p neoc-vm --example luau_decls`; never edit it by hand). With [luau-lsp](https://github.com/JohnnyMorganz/luau-lsp)
they give completion, hover docs and type checks, and a `require` of a name that is not a module is a type error:

```sh
luau-lsp analyze --definitions:@neoc=types/neoc.d.luau script.luau
```

In VS Code (Luau Language Server extension): `"luau-lsp.platform.type": "standard"` and
`"luau-lsp.types.definitionFiles": { "@neoc": "path/to/neoc/types/neoc.d.luau" }`. Another editor starts the server as
`luau-lsp lsp --stdio --definitions:@neoc=path/to/neoc.d.luau`. `require` is typed for a string literal; put `--!strict` at the
top of a script to have optional results checked. The declarations cover the modules, not the base globals (those are luau-lsp's
own Luau ones).

## vos

`bun run dev` starts the host on :7400 and the client on :5173. The host prints a pairing code; open
<http://localhost:5173>, enter it, and open Discover to install apps. `apps/calculator` and `apps/agents` are the reference
apps. Data lives under `~/.vos` (`VOS_DATA` overrides it); settings come from `vos.toml` and `VOS_*` variables (the table is in
`crates/vos-host/src/config.rs`).
