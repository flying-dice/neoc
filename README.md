# neoc

> *Neocybex (n.) — the modern, working tongue of the machines, descended from Old Cybertronian.*

**A fast, lightweight [LuaJIT](https://luajit.org/) runtime. Rust's standard library, blessed gap-fillers, and vendored crates. One trusted supply chain.**

A toolkit AI agents use to build things. Install one program — everything needed, no setup, no mid-task downloads.

One of each, on purpose. Agents can't pick wrong when the right answer is the only answer. Same idea on security — audited set bundled in, nothing pulled from outside at runtime.

## One binary

`neoc` runs Lua scripts in a VM with a curated module surface: unsafe Lua globals are stripped and only registered modules load. It is not a security boundary.

| Command | Job |
|---|---|
| `neoc run <script>...` | run Lua scripts; globs expand |
| `neoc run -e <source>` | run an inline Lua snippet |

neoc used to be a language that compiled to Lua. That is gone: the runtime is the product. The last state of the language is preserved under the `neoc-lang-final` tag.

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

Pre-built binaries on [GitHub Releases](https://github.com/flying-dice/neoc/releases).

```sh
# Linux / macOS (x86_64, arm64)
curl -fsSL https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.sh | bash

# Windows (PowerShell)
# irm https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.ps1 | iex

# From source
cargo install --path crates/neoc
```

`NEOC_INSTALL_DIR` (default `~/.neoc/bin`) controls binary placement.

```lua
-- notes.lua
local fs   = require("std:fs")
local json = require("vnd:serde_json")

local doc = assert(json.from_str('{"name": "neoc", "tags": ["lua", "rust"]}'))

fs.write("notes.json", json.to_string_pretty(doc))
print("saved " .. doc:get("name"))
```

```sh
neoc run notes.lua
```

## What's inside

Three layers, one binary — `std:*` mirrors Rust's standard library, `lib:*` are curated helpers and gap-fillers, `vnd:*` are handpicked third-party libraries. Browse [crates/neoc-vm/src/lua/](crates/neoc-vm/src/lua) for the live catalogue.

`require("namespace:name")` is the only way in. No ambient globals, no path search. Unknown names error with the catalogue.

## Running

```sh
neoc run script.lua                   # one script
neoc run 'tests/**/*.lua'             # globs expand
neoc run one.lua two.lua              # several in sequence
neoc run -e 'print(1 + 2)'            # inline snippet
neoc run script.lua -- a b            # arguments reach the script as _NEOC_ARGS
```

`.test.lua` / `.spec.lua` suffix triggers built-in test runner.

`lib:compile` bundles a script and the runtime into a standalone executable.

## Workspace layout

```
crates/neoc            # the `neoc` binary: the CLI over the runtime
crates/neoc-vm         # the LuaJIT runtime: engine, sandbox, std/lib/vnd modules
tooling/cli            # Gherkin spec for the CLI, run against the built binary
docs/                  # the documentation site
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
