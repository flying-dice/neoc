# `std:*`: Standard-library modules

Mirrors Rust's standard library. Surface shaped so Rust-fluent readers recognise methods and semantics immediately.

Also contains modules with no `std::` equivalent but broadly needed — `std:workers` (worker-pool substrate) is the principal example.

## Reference

- [`std:collections`](/std/reference/collections) — Concurrent, shareable collection types
- [`std:fs`](/std/reference/fs) — Filesystem reads, writes, metadata
- [`std:io`](/std/reference/io) — Standard input/output handles
- [`std:net`](/std/reference/net) — TCP listeners and sockets
- [`std:thread`](/std/reference/thread) — Thread-local primitives and sleep
- [`std:workers`](/std/reference/workers) — Worker pool, cross-worker state, shutdown signal

## See also

- [The module system](/guides/module-system)
- [The sandboxing model](/guides/sandboxing-model)
- [`lib:*`](/lib/)
- [`vnd:*`](/vnd/)
