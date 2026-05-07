//! `vnd:*` modules — vendored crates.
//!
//! The module name **must** match the upstream crate name so the import is
//! unambiguous: `vnd:hyper` wraps the `hyper` crate, `vnd:serde_json` wraps
//! `serde_json`, and so on. Modules expose a `pub fn module(lua: &Lua) ->
//! Result<Table>` and are wired into the `require` registry by
//! `crate::lua::modules::install`.

pub mod hyper;
pub mod jsonschema;
pub mod quick_xml;
pub mod rusqlite;
pub mod serde_json;
pub mod serde_yaml;
pub mod sqlx_mysql;
pub mod sqlx_postgres;
pub mod testcontainers_modules;
pub mod toml;
