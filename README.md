# neoc

> *Neocybex (n.) — the modern, working tongue of the machines, descended in use from Old Cybertronian.*

**A fast, lightweight [Luau](https://luau.org/) runtime. Rust's standard library, blessed gap-fillers, and vendored crates. One trusted supply chain.**

A toolkit AI agents can use to build things. Install one program and an agent has everything it needs — no setup, no extra downloads, nothing to chase mid-task. The same toolkit, every machine, every call.

Most software ecosystems are designed for humans. People have taste, opinions, and time to compare ten options. Agents don't. Faced with five ways to do the same thing, an agent will sometimes pick the wrong one — and pick differently the next time. neoc gives one of each, on purpose. The right answer is the only answer, so an agent can't pick wrong.

The same idea protects the security side. Open ecosystems pull in code from thousands of strangers; every piece is something you have to hope is safe. neoc bundles a small, audited set of tools directly into the program — nothing to install at runtime, nothing pulled in from outside.

## The Seekers

The agents who ship, design, review, and maintain neoc — the runtime that speaks it.

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

Pre-built binaries for Linux and Windows live on the [GitLab Releases](https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/releases) page.

```sh
# Linux x86_64
curl -fsSL https://gitlab.beluga-sirius.ts.net/api/v4/projects/13/packages/generic/neoc/v0.1.0/install.sh | sh

# Windows x86_64 — download directly
# https://gitlab.beluga-sirius.ts.net/api/v4/projects/13/packages/generic/neoc/v0.1.0/neoc-x86_64-pc-windows-gnu.exe

# From source
cargo install --path .
```

`NEOC_INSTALL_DIR` (default `~/.neoc/bin`) controls where the install script places the binary.

A first script — fetch a URL, parse JSON, write a file:

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
neoc fetch.luau
```

## What's inside

Three layers, one binary.

| Layer    | Purpose                                  | Modules |
|----------|------------------------------------------|---------|
| `std:*`  | Rust's standard library, 1:1 in Luau     | `collections`, `fs`, `io`, `net`, `thread`, `workers` |
| `lib:*`  | Curated helpers and gap-fillers we wrote | `base64`, `json`, `mime`, `test`, `vcr` |
| `vnd:*`  | Handpicked third-party libraries         | `hyper`, `rusqlite`, `sqlx_postgres`, `sqlx_mysql`, `serde_json`, `serde_yaml`, `quick_xml`, `toml`, `jsonschema`, `testcontainers_modules` |

`require("namespace:name")` is the only way in. No ambient globals, no path search, no file I/O — unknown names error with the catalogue, so typos surface immediately.

For native code, a `.dll` or `.so` built on [mlua](https://github.com/mlua-rs/mlua) plugs in alongside.

## Running

```sh
neoc script.luau                       # run one script
neoc 'tests/**/*.luau'                 # globs expand
neoc one.luau two.luau                 # several scripts in sequence
```

A `.test.luau` or `.spec.luau` suffix triggers the built-in test runner — pass/fail counts print after execution.

For long-running HTTP servers, the script binds a listener and uses `std:thread`'s `spawn({ source = "...", shared = { ... } })` to fan out workers that share the listener fd. See `examples/serve/handler.luau` for the canonical pattern, with `vnd:hyper`'s `serve` driving each worker.

## Where it fits

| Where it shines                                                                    | Where it doesn't                                                              |
|------------------------------------------------------------------------------------|-------------------------------------------------------------------------------|
| Embedding in Rust apps — engines are `Send`, many VMs across many threads          | Tight pure-Lua compute loops — Luau has codegen, not a V8-class JIT           |
| Snappy startup, low memory — short-lived invocations, sidecar processes            | Bouncing large data through the Lua boundary — every translation has a cost   |
| Orchestrating Rust-backed work — HTTP, parsers, regex, file I/O, SQL               | Pulling in arbitrary third-party packages — the dep tree is closed by design  |
| One trusted supply chain — auditable, vendored, no transitive surprises            | A drop-in replacement for an existing runtime                                 |

## Going further

- [STYLEGUIDE.md](STYLEGUIDE.md) — module conventions, return-type rules, dependency policy
- [CONTRIBUTING.md](CONTRIBUTING.md) — adding a module, tests, benchmarks
- [docs/styleguide/documentation-styleguide.md](docs/styleguide/documentation-styleguide.md) — documentation conventions

It isn't trying to replace anything. It's a different trade-off — one shaped around who's writing code now.
