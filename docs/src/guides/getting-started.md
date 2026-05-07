# Getting started

This guide walks through installing neoc and running a first Luau script. It assumes a working command line and basic familiarity with Lua syntax.

## Installation

Pre-built binaries for Linux, macOS, and Windows are published on [GitHub Releases](https://github.com/flying-dice/neoc/releases). The install scripts download the appropriate binary for the host platform and place it on the path.

### On Linux or macOS

```bash
curl -fsSL https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.sh | bash
```

### On Windows

```powershell
irm https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.ps1 | iex
```

### Environment variables

The install scripts respect the following environment variables.

`NEOC_INSTALL_DIR`
:   The directory to install the binary into. Defaults to `~/.neoc/bin`.

`NEOC_VERSION`
:   The release tag to install. Defaults to `latest`.

## Running a script

A Luau script is a plain text file with a `.luau` extension. The neoc binary takes one or more file paths or glob patterns and runs each matching file as an independent script.

### A first script

The following script prints a greeting.

```lua
print("hello, neoc")
```

Save it as `hello.luau` and run it with the binary.

```bash
neoc hello.luau
```

The output is the literal string `hello, neoc` followed by a newline.

### Running multiple scripts

The binary accepts multiple paths and glob patterns, expanding them before execution.

```bash
neoc 'tests/**/*.test.luau'
```

This invocation runs every file matching the glob pattern as an independent script. The shell quoting prevents the shell from expanding the glob; neoc's internal expansion picks up the pattern instead.

## What runs where

Each script executes inside its own Luau virtual machine. The runtime maintains a pool of these virtual machines as *workers* and dispatches scripts across them. Globals, locals, and userdata constructed inside a worker are private to that worker; cross-worker state is exposed only through the explicit `std:workers` surface.

For an explanation of why the engine uses this model and what it implies for script authors, see [The sandboxing model](/guides/sandboxing-model).

## See also

- [The sandboxing model](/guides/sandboxing-model) — How the curated module surface replaces stripped Lua functionality.
- [The module system](/guides/module-system) — How `require("ns:name")` resolves.
- [`std:io`](/std/reference/io) — Standard input and output.
- [`std:workers`](/std/reference/workers) — The worker pool and cross-worker shared state.
