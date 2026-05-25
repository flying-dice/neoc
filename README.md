# neoc

> *Neocybex (n.) — the modern, working tongue of the machines, descended from Old Cybertronian.*

**A fast, lightweight [Luau](https://luau.org/) runtime _and_ the compiler for the neoc language. Rust's standard library, blessed gap-fillers, and vendored crates. One trusted supply chain.**

A toolkit AI agents use to build things. Install one program — everything needed, no setup, no mid-task downloads.

One of each, on purpose. Agents can't pick wrong when the right answer is the only answer. Same idea on security — audited set bundled in, nothing pulled from outside at runtime.

## One repo, one binary

This monorepo ships a single `neoc` binary that does both jobs:

| Subcommand | Job |
|---|---|
| `neoc run <script>` | run a Luau script in the sandboxed runtime |
| `neoc emit-headers --out <dir>` | emit `ext` headers describing the runtime surface |
| `neoc build <src>` | compile `.neoc` → Lua 5.1 (or JS via `--target`) |
| `neoc check <src>` | parse + type-check, no output |
| `neoc fmt <src>` | format `.neoc` source |
| `neoc lsp` | language server over stdio |

The companion grammar lives in a second repo, `tree-sitter-neoc`, consumed here as the `neoc-grammar` submodule.

## Documentation style

The Transformers learned from [caveman humans](https://github.com/JuliusBrussee/caveman). Short words. Few words. Maximum signal. Modern humans talk too much — the machines found a better way. These docs follow that principle.

## The Seekers

The agents who ship, design, review, and maintain neoc.

<table>
<tr>
<td align="center" width="25%">
<img src="media/starscream_real.png" width="140" alt="Starscream" /><br/>
<b>Starscream</b><br/>
<sub>Implementation lead</sub>
</td>
<td align="center" width="25%">
<img src="media/thundercracker_real.png" width="140" alt="Thundercracker" /><br/>
<b>Thundercracker</b><br/>
<sub>Design &amp; documentation</sub>
</td>
<td align="center" width="25%">
<img src="media/skywarp_real.png" width="140" alt="Skywarp" /><br/>
<b>Skywarp</b><br/>
<sub>Refactors &amp; precision fixes</sub>
</td>
<td align="center" width="25%">
<img src="media/shockwave_real.png" width="140" alt="Shockwave" /><br/>
<b>Shockwave</b><br/>
<sub>Code review</sub>
</td>
</tr>
</table>

## Quickstart

Pre-built binaries on [GitLab Releases](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/releases).

```sh
# Linux x86_64
curl -fsSL https://gitlab.beluga-sirius.ts.net/api/v4/projects/13/packages/generic/neoc/v0.1.0/install.sh | sh

# Windows x86_64 — download directly
# https://gitlab.beluga-sirius.ts.net/api/v4/projects/13/packages/generic/neoc/v0.1.0/neoc-x86_64-pc-windows-gnu.exe

# From source
cargo install --path .
```

`NEOC_INSTALL_DIR` (default `~/.neoc/bin`) controls binary placement.

```lua
-- fetch.luau
local fs    = require("std:fs")
local hyper = require("vnd:hyper")
local json  = require("vnd:serde_json")

local resp = assert(hyper.get("https://gitlab.beluga-sirius.ts.net/api/v4/projects/flying-dice%2Fneoc"))
local data = assert(json.from_str(resp.body))

fs.write("repo.json", json.to_string_pretty(data))
print("saved " .. data:get("path_with_namespace"))
```

```sh
neoc run fetch.luau
```

## What's inside

Three layers, one binary — `std:*` mirrors Rust's standard library, `lib:*` are curated helpers and gap-fillers, `vnd:*` are handpicked third-party libraries. Browse [crates/neoc-vm/src/lua/](crates/neoc-vm/src/lua) for the live catalogue.

`require("namespace:name")` is the only way in. No ambient globals, no path search. Unknown names error with the catalogue.

## Running

```sh
neoc run script.luau                   # one script
neoc run 'tests/**/*.luau'             # globs expand
neoc run one.luau two.luau             # several in sequence
```

`.test.luau` / `.spec.luau` suffix triggers built-in test runner.

## Compiling neoc

```sh
neoc build main.neoc                   # → Lua 5.1 (default target)
neoc build main.neoc --target js       # → JavaScript (ES2022)
neoc check main.neoc                   # parse + type-check only
neoc fmt main.neoc                     # canonical formatting
neoc lsp                               # language server (stdio)
```

`.neoc` compiles to Lua 5.1 by default — the lowest common denominator, so the output runs unchanged on Luau, Lua 5.4, LuaJIT, and this runtime. The codegen layer is decoupled from the language surface, so a JavaScript (ES2022) backend ships alongside it, selected with `--target`.

## Workspace layout

```
crates/neoc            # the `neoc` binary: unified CLI + folded compiler subcommands
crates/neoc-vm         # the Luau runtime: engine, script execution, .neoc headers
crates/neoc-grammar    # vendors tree-sitter-neoc (submodule); cc-builds the parser
crates/neoc-ast        # typed AST — pure data, span-free
crates/neoc-parser     # CST → AST adapter
crates/neoc-types      # name resolution, inference, well-formedness
crates/neoc-macros     # built-in derives (Clone/Eq/Debug/Display)
crates/neoc-codegen-lua # AST → Lua 5.1
crates/neoc-codegen-js  # AST → JavaScript (ES2022)
crates/neoc-compiler   # public lib API: transpile to any target (the runtime embeds this)
crates/neoc-fmt        # formatter
crates/neoc-lsp        # language server
crates/neoc-wasm       # browser entry point
```

## Where it fits

| Shines | Doesn't |
|---|---|
| Embedding in Rust — engines are `Send`, many VMs across threads | Tight pure-Lua compute loops (no V8-class JIT) |
| Snappy startup, low memory — short-lived invocations | Large data through Lua boundary — every translation costs |
| Orchestrating Rust work — HTTP, parsers, regex, I/O, SQL | Arbitrary third-party packages (dep tree closed by design) |
| One trusted supply chain — auditable, vendored | Drop-in replacement for existing runtime |

## Going further

- [STYLEGUIDE.md](STYLEGUIDE.md) — module conventions, return-type rules, dependency policy
- [CONTRIBUTING.md](CONTRIBUTING.md) — adding a module, tests, benchmarks, commit style
- [CLAUDE.md](CLAUDE.md) — instructions for Claude Code
- [docs/styleguide/documentation-styleguide.md](docs/styleguide/documentation-styleguide.md) — documentation conventions
