# `vnd:*`: Vendored crate modules

Each module wraps exactly one Rust crate. Module name matches crate name verbatim. No renaming, no aliasing, no merging. Non-negotiable naming rule.

## Reference

### HTTP and networking

- [`vnd:hyper`](/vnd/reference/hyper) — HTTP client/server

### Data formats

- [`vnd:serde_json`](/vnd/reference/serde_json) — JSON parsing/serialisation
- [`vnd:serde_yaml`](/vnd/reference/serde_yaml) — YAML parsing/serialisation
- [`vnd:toml`](/vnd/reference/toml) — TOML parsing/serialisation
- [`vnd:quick_xml`](/vnd/reference/quick_xml) — XML parsing/serialisation
- [`vnd:jsonschema`](/vnd/reference/jsonschema) — JSON Schema validation

### Databases

- [`vnd:rusqlite`](/vnd/reference/rusqlite) — SQLite (bundled)
- [`vnd:sqlx_mysql`](/vnd/reference/sqlx_mysql) — MySQL via sqlx
- [`vnd:sqlx_postgres`](/vnd/reference/sqlx_postgres) — PostgreSQL via sqlx

### Test infrastructure

- [`vnd:testcontainers_modules`](/vnd/reference/testcontainers_modules) — Test containers

## See also

- [The module system](/guides/module-system)
- [`std:*`](/std/)
- [`lib:*`](/lib/)
