# Getting started

Install neoc and run a first Luau script. Assumes a working CLI and basic Lua familiarity.

## Installation

Pre-built binaries on [GitHub Releases](https://github.com/flying-dice/neoc/releases).

### Linux / macOS

```sh
curl -fsSL https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.sh | bash
```

Downloads binary, places in `$NEOC_INSTALL_DIR` (default `~/.neoc/bin`). Add to `$PATH` if needed.

### Windows

```powershell
irm https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.ps1 | iex
```

### From source

```sh
cargo install --path .
```

### Environment variables

`NEOC_INSTALL_DIR`
:   Install directory. Default: `~/.neoc/bin`.

## Running a script

Luau scripts are `.luau` files. The binary takes file paths or glob patterns, runs each match independently.

### First script

```lua
print("hello, neoc")
```

```bash
neoc run hello.luau
```

Output: `hello, neoc\n`

### Multiple scripts

```bash
neoc run 'tests/**/*.test.luau'
```

Shell quoting prevents shell expansion; neoc expands the glob internally.

## Execution model

Each script runs in its own Luau VM. Runtime maintains a worker pool, dispatches scripts across it. Globals, locals, and userdata are private to each worker. Cross-worker state only via explicit `std:workers` surface.

See [The sandboxing model](/guides/sandboxing-model) for rationale and implications.

## See also

- [The sandboxing model](/guides/sandboxing-model)
- [The module system](/guides/module-system)
- [`std:io`](/std/reference/io)
- [`std:workers`](/std/reference/workers)
