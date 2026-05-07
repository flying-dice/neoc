# `std:*`: Standard-library modules

The `std:*` namespace exposes modules that mirror the Rust standard library. Each module's public surface is shaped so a reader fluent in Rust's `std::*` recognises the methods, semantics, and error behaviour without explanation.

This namespace also contains a small number of modules that have no `std::` equivalent but cover capabilities that scripts cannot reasonably do without — `std:workers`, the worker-pool substrate, is the principal example. New modules are added to `std:*` only when they are both broadly needed and cleanly mappable to a Rust standard concept.

## Reference

- [`std:collections`](/std/reference/collections) — Concurrent and shareable collection types.
- [`std:fs`](/std/reference/fs) — Filesystem reads, writes, and metadata.
- [`std:io`](/std/reference/io) — Standard input and output handles.
- [`std:net`](/std/reference/net) — TCP listeners and sockets.
- [`std:thread`](/std/reference/thread) — Thread-local primitives and sleep.
- [`std:workers`](/std/reference/workers) — The worker pool, cross-worker shared state, and the shutdown signal.

## See also

- [The module system](/guides/module-system) — How `require("std:name")` resolves.
- [The sandboxing model](/guides/sandboxing-model) — What the standard surface replaces from Lua's stripped `os`, `io`, and `package`.
- [`lib:*`](/lib/) — Project-native and gap-filling modules.
- [`vnd:*`](/vnd/) — Vendored crates.
