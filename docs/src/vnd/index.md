# `vnd:*`: Vendored crate modules

The `vnd:*` namespace exposes individual Rust crates to Lua. Each module wraps exactly one upstream crate, and the module name matches the crate name verbatim. There is no renaming, no aliasing, no merging of crates into composite modules — `require("vnd:hyper")` always means *the* `hyper` crate, and never anything else.

This naming rule is non-negotiable. It is what lets scripts reach for an upstream crate by its real name, find its real surface (modulo Lua adaptations), and locate its real documentation without indirection.

## Reference

### HTTP and networking

- [`vnd:hyper`](/vnd/reference/hyper) — HTTP client and server backed by the `hyper` crate.

### Data formats

- [`vnd:serde_json`](/vnd/reference/serde_json) — JSON parsing and serialisation.
- [`vnd:serde_yaml`](/vnd/reference/serde_yaml) — YAML parsing and serialisation.
- [`vnd:toml`](/vnd/reference/toml) — TOML parsing and serialisation.
- [`vnd:quick_xml`](/vnd/reference/quick_xml) — XML parsing and serialisation via `quick-xml`.
- [`vnd:jsonschema`](/vnd/reference/jsonschema) — JSON Schema validation.

### Databases

- [`vnd:rusqlite`](/vnd/reference/rusqlite) — SQLite via `rusqlite` (bundled).
- [`vnd:sqlx_mysql`](/vnd/reference/sqlx_mysql) — MySQL via `sqlx`.
- [`vnd:sqlx_postgres`](/vnd/reference/sqlx_postgres) — PostgreSQL via `sqlx`.

### Test infrastructure

- [`vnd:testcontainers_modules`](/vnd/reference/testcontainers_modules) — Test containers for integration tests.

## See also

- [The module system](/guides/module-system) — How `require("vnd:name")` resolves and the naming rule.
- [`std:*`](/std/) — Standard-library mirrors.
- [`lib:*`](/lib/) — Blessed gap-fillers and project-native modules.
