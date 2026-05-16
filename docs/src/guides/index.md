# Guides

How neoc works and how to use it. Read in order when learning the runtime.

For module-specific surface, see reference pages under [`std:*`](/std/), [`lib:*`](/lib/), or [`vnd:*`](/vnd/).

## Getting oriented

[Getting started](/guides/getting-started)
:   Install neoc, run a first script, understand invocation.

[The sandboxing model](/guides/sandboxing-model)
:   What's stripped from Lua, why, and what replaces it.

[The module system](/guides/module-system)
:   How `require("ns:name")` resolves. Three namespaces and their rules.

[Error conventions](/guides/error-conventions)
:   `(value, err)` tuples vs raises. Error prefix format.

## See also

- [`std:*`](/std/) — Standard-library mirrors
- [`lib:*`](/lib/) — Project-native modules
- [`vnd:*`](/vnd/) — Vendored crates
- [RFCs](/rfcs/) — Cross-cutting design proposals
