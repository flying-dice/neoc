//! `vnd:*` modules — vendored crates.
//!
//! The module name **must** match the upstream crate name so the import is
//! unambiguous: `vnd:hyper` wraps the `hyper` crate, `vnd:serde_json` wraps
//! `serde_json`, and so on. Modules expose a `pub fn module(lua: &Lua) ->
//! Result<Table>` and are wired into the `require` registry by
//! `crate::lua::modules::install`.

pub mod bollard;
pub mod hyper;
pub mod jsonschema;
pub mod keyring;
pub mod portable_pty;
pub mod quick_xml;
pub mod rusqlite;
pub mod serde_json;
pub mod serde_yaml;
pub mod sqlx_mysql;
pub mod sqlx_postgres;
pub mod testcontainers_modules;
pub mod toml;

use mlua::{MultiValue, Value};

/// Splits `(sql, ...params)` call arguments into the SQL text and the bind
/// parameters. Shared by the database drivers (`rusqlite`, `sqlx_mysql`,
/// `sqlx_postgres`).
///
/// # Errors
///
/// Returns a message when the first argument is missing or is not a string.
pub(crate) fn split_sql_args(mut args: MultiValue) -> Result<(String, Vec<Value>), String> {
    let head = args
        .pop_front()
        .ok_or_else(|| "missing SQL string".to_string())?;
    let sql = match head {
        Value::String(s) => s.to_str().map_err(|e| e.to_string())?.to_owned(),
        other => {
            return Err(format!(
                "expected SQL string, got {} — accepted: string",
                other.type_name()
            ));
        }
    };
    Ok((sql, args.into_iter().collect()))
}
