# Guides

This section contains narrative documentation that explains how neoc works and how to use it. Guides are read in order when learning the runtime; each one builds on the ones above it.

For the full surface of a specific module, see the reference page under [`std:*`](/std/), [`lib:*`](/lib/), or [`vnd:*`](/vnd/) instead.

## Getting oriented

[Getting started](/guides/getting-started)
:   Install neoc, run a first script, and understand the basic invocation model.

[The sandboxing model](/guides/sandboxing-model)
:   What the engine strips from Lua, why those parts are stripped, and what the curated module surface replaces them with.

[The module system](/guides/module-system)
:   How `require("ns:name")` resolves, the three namespaces, and the rules each one follows.

[Error conventions](/guides/error-conventions)
:   When functions return `(value, err)` tuples and when they raise. How error prefixes are formatted.

## See also

- [`std:*`](/std/) — Standard-library mirrors.
- [`lib:*`](/lib/) — Project-native modules.
- [`vnd:*`](/vnd/) — Vendored crates.
- [RFCs](/rfcs/) — Cross-cutting design proposals.
