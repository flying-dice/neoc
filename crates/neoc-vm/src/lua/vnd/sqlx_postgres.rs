//! PostgreSQL — `lib.sqlx_postgres`, backed by sqlx (pure-Rust driver).
//!
//! Connection strings are standard Postgres URLs:
//! `postgres://user:pass@host:5432/dbname`. **TLS is not enabled in this
//! build** — connect to plaintext servers only (`vnd:hyper` is the only
//! module with a TLS stack).
//!
//! Postgres uses positional placeholders `$1, $2, …` (not `?`):
//!
//! ```lua
//! local db, err = lib.sqlx_postgres.open("postgres://postgres@127.0.0.1/test")
//! db:execute("CREATE TABLE users (id INT PRIMARY KEY, name TEXT)")
//! db:execute("INSERT INTO users (id, name) VALUES ($1, $2)", 1, "alice")
//! local rows = db:query("SELECT id, name FROM users")
//! db:close()
//! ```
//!
//! Strings bind as `TEXT` when valid UTF-8 and as `BYTEA` otherwise. Result
//! columns decode as: `BOOL` → boolean, `INT2`/`INT4`/`INT8` and
//! `FLOAT4`/`FLOAT8` → number, `BYTEA` → string, `TEXT`/`VARCHAR`/`CHAR(n)`/
//! `NAME`/`citext` → string. Any other column type raises
//! `unsupported column type <T>; cast to text in SQL` — e.g.
//! `SELECT created_at::text`, `SELECT data::text` for `JSON`/`JSONB`,
//! `SELECT id::text` for `UUID`.
//!
//! Postgres is strictly typed at bind time — to insert SQL `NULL` into a
//! non-text column, prefer the `NULL` literal in SQL over passing `nil` (we
//! bind `nil` as `Option::<i64>::None`, which the server rejects against e.g.
//! `TEXT` columns). All other casts work as usual: `INSERT … VALUES ($1::int)`.

use std::str::FromStr;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Table, UserData, UserDataMethods, Value};
use sqlx::AssertSqlSafe;
use sqlx::pool::PoolOptions;
use sqlx::postgres::{PgArguments, PgConnectOptions, PgPool, PgRow, Postgres};
use sqlx::query::Query;
use sqlx::{Column, Row, TypeInfo, ValueRef};
use tracing::instrument;

use super::split_sql_args;
use crate::lua::facade::Module;
use crate::lua::lifecycle::{Role, async_function, async_method};
use crate::lua::sandbox::{Fallible, from_err, i64_to_lua, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:sqlx_postgres` surface through the facade.
///
/// `open` is set verbatim with [`crate::lua::facade::Module::set`] rather than
/// `try_async_func` because the hand-written async function carries its own
/// `#[instrument(name = "sqlx_postgres.open", target = "lua::lib::sqlx_postgres")]`
/// attribute and bakes the `sqlx_postgres.open` error prefix into its pre-built
/// `Fallible` return — routing through the facade would add a second span
/// and double the prefix. Its signature is read by `neoc_macros`
/// from the implementation. The `Database` userdata type
/// keeps its own `impl UserData` (the async methods and `sqlx_postgres.*` error
/// prefixes unchanged) and is recorded via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:sqlx_postgres")?;

    m.declared(
        open::lua(),
        async_function(lua, Role::Wait, |lua, url: String| open(lua, url))?,
    )?;

    m.userdata("Database", |d| {
        Database::lua(d);
    });

    Ok(m)
}

#[derive(Clone)]
pub struct Database {
    pool: Arc<PgPool>,
}

/// Open a Postgres connection pool. `url` is a standard Postgres URL: `postgres://user:pass@host:5432/dbname`.
#[neoc_macros::lua_fn(takes("url: string"))]
#[instrument(
    name = "sqlx_postgres.open",
    target = "lua::lib::sqlx_postgres",
    skip_all
)]
async fn open(_: Lua, url: String) -> mlua::Result<Fallible<Database>> {
    let opts = match PgConnectOptions::from_str(&url) {
        Ok(o) => o,
        Err(e) => return from_err("sqlx_postgres.open", e),
    };
    match PoolOptions::<Postgres>::new().connect_with(opts).await {
        Ok(pool) => ok(Database {
            pool: Arc::new(pool),
        }),
        Err(e) => from_err("sqlx_postgres.open", e),
    }
}

#[neoc_macros::lua_userdata("vnd:sqlx_postgres", "Database")]
impl UserData for Database {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Execute a SQL statement, binding trailing arguments to `$1, $2, …` placeholders; returns rows affected.
        #[lua(takes("sql: string", "...: LuaValue"))]
        async_method(
            methods,
            "execute",
            Role::Wait,
            |_, this, args: MultiValue| {
                let pool = this.pool.clone();
                async move {
                    let op = "sqlx_postgres.execute";
                    let (sql, params) = match prepare(args, &pool) {
                        Ok(v) => v,
                        Err(e) => return from_err(op, e),
                    };
                    let q = match build_query(&sql, params) {
                        Ok(q) => q,
                        Err(e) => return from_err(op, e),
                    };
                    match q.execute(&*pool).await {
                        Ok(r) => ok(r.rows_affected() as i64),
                        Err(e) => from_err(op, e),
                    }
                }
            },
        );

        /// Execute a SELECT and return all rows as a 1-indexed table of column-keyed row tables. A column of an unsupported type raises; cast it to text in SQL.
        #[lua(takes("sql: string", "...: LuaValue"))]
        async_method(
            methods,
            "query",
            Role::Wait,
            |lua, this, args: MultiValue| {
                let pool = this.pool.clone();
                async move {
                    let op = "sqlx_postgres.query";
                    let (sql, params) = match prepare(args, &pool) {
                        Ok(v) => v,
                        Err(e) => return from_err(op, e),
                    };
                    let q = match build_query(&sql, params) {
                        Ok(q) => q,
                        Err(e) => return from_err(op, e),
                    };
                    match q.fetch_all(&*pool).await {
                        Ok(rows) => {
                            let out = lua.create_table()?;
                            for (i, row) in rows.iter().enumerate() {
                                out.set(i + 1, row_to_table(&lua, row)?)?;
                            }
                            ok(out)
                        }
                        Err(e) => from_err(op, e),
                    }
                }
            },
        );

        /// Execute a SELECT and return the first matching row, or nil when none matched. A column of an unsupported type raises; cast it to text in SQL.
        #[lua(takes("sql: string", "...: LuaValue"))]
        async_method(
            methods,
            "query_one",
            Role::Wait,
            |lua, this, args: MultiValue| {
                let pool = this.pool.clone();
                async move {
                    let op = "sqlx_postgres.query_one";
                    let (sql, params) = match prepare(args, &pool) {
                        Ok(v) => v,
                        Err(e) => return from_err(op, e),
                    };
                    let q = match build_query(&sql, params) {
                        Ok(q) => q,
                        Err(e) => return from_err(op, e),
                    };
                    match q.fetch_optional(&*pool).await {
                        Ok(Some(row)) => ok(Some(row_to_table(&lua, &row)?)),
                        Ok(None) => ok(None),
                        Err(e) => from_err(op, e),
                    }
                }
            },
        );

        /// Close all connections in the pool; subsequent method calls on this handle error.
        async_method(methods, "close", Role::Wait, |_, this, _: ()| {
            let pool = this.pool.clone();
            async move {
                pool.close().await;
                Ok(())
            }
        });
    }
}

fn prepare(args: MultiValue, pool: &PgPool) -> Result<(String, Vec<Value>), String> {
    let (sql, params) = split_sql_args(args)?;
    if pool.is_closed() {
        return Err("database is closed".into());
    }
    Ok((sql, params))
}

// `AssertSqlSafe`: running caller-supplied SQL text is this module's purpose;
// values reach the database only through bound parameters (sqlx 0.9 requires
// the explicit opt-in for non-literal SQL).
fn build_query<'q>(
    sql: &'q str,
    params: Vec<Value>,
) -> Result<Query<'q, Postgres, PgArguments>, String> {
    params
        .into_iter()
        .try_fold(sqlx::query(AssertSqlSafe(sql)), bind_param)
}

// ---------------------------------------------------------------------------
// Lua → sqlx
// ---------------------------------------------------------------------------

fn bind_param<'q>(
    q: Query<'q, Postgres, PgArguments>,
    v: Value,
) -> Result<Query<'q, Postgres, PgArguments>, String> {
    Ok(match v {
        Value::Nil => q.bind(Option::<i64>::None),
        Value::Boolean(b) => q.bind(b),
        Value::Integer(i) => q.bind(i),
        Value::Number(f) => q.bind(f),
        // Documented contract (module docs): utf8 → TEXT, otherwise BYTEA.
        Value::String(s) => {
            let bytes = s.as_bytes();
            match std::str::from_utf8(&bytes) {
                Ok(text) => q.bind(text.to_owned()),
                Err(_) => q.bind(bytes.to_vec()),
            }
        }
        other => return Err(format!("cannot bind {} as parameter", other.type_name())),
    })
}

fn row_to_table(lua: &Lua, row: &PgRow) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for col in row.columns() {
        let name = col.name();
        let raw = row
            .try_get_raw(col.ordinal())
            .map_err(mlua::Error::external)?;
        let val = if raw.is_null() {
            Value::Nil
        } else {
            decode_cell(lua, row, col.ordinal(), raw.type_info().name())?
        };
        t.set(name, val)?;
    }
    Ok(t)
}

fn decode_cell(lua: &Lua, row: &PgRow, idx: usize, type_name: &str) -> mlua::Result<Value> {
    match decode_known(lua, row, idx, type_name)? {
        Some(v) => Ok(v),
        None if is_text_type(type_name) => decode_text(lua, row, idx),
        None => Err(mlua::Error::external(format!(
            "sqlx_postgres.decode: unsupported column type {type_name}; cast to text in SQL"
        ))),
    }
}

/// Decode a column whose Postgres type maps to a fixed Lua scalar. Returns
/// `None` for any type not in this table; the caller then decodes it as text if
/// [`is_text_type`] allows, otherwise raises. Postgres is strict about widths
/// (INT4 via i32, FLOAT4 via f32, …); a mismatch raises a decode error rather
/// than coercing.
fn decode_known(
    lua: &Lua,
    row: &PgRow,
    idx: usize,
    type_name: &str,
) -> mlua::Result<Option<Value>> {
    let v = match type_name {
        "BOOL" => {
            let b: bool = row.try_get(idx).map_err(mlua::Error::external)?;
            Value::Boolean(b)
        }
        "INT2" => {
            let n: i16 = row.try_get(idx).map_err(mlua::Error::external)?;
            i64_to_lua(i64::from(n))
        }
        "INT4" => {
            let n: i32 = row.try_get(idx).map_err(mlua::Error::external)?;
            i64_to_lua(i64::from(n))
        }
        "INT8" => {
            let n: i64 = row.try_get(idx).map_err(mlua::Error::external)?;
            i64_to_lua(n)
        }
        "FLOAT4" => {
            let f: f32 = row.try_get(idx).map_err(mlua::Error::external)?;
            Value::Number(f as f64)
        }
        "FLOAT8" => {
            let f: f64 = row.try_get(idx).map_err(mlua::Error::external)?;
            Value::Number(f)
        }
        "BYTEA" => {
            let b: Vec<u8> = row.try_get(idx).map_err(mlua::Error::external)?;
            Value::String(lua.create_string(&b)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// Column types that decode as a Lua string: the TEXT family (`CHAR(n)`
/// reports as `CHAR`), `UNKNOWN` and `citext`. These are exactly the types
/// sqlx lets `String` decode. Every other type not handled by `decode_known`
/// (NUMERIC, JSON, JSONB, UUID, temporal types, arrays, …) raises; callers
/// cast to text in SQL.
fn is_text_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "UNKNOWN" | "citext"
    )
}

/// Decode a column accepted by [`is_text_type`] through `String`.
fn decode_text(lua: &Lua, row: &PgRow, idx: usize) -> mlua::Result<Value> {
    let v: String = row.try_get(idx).map_err(mlua::Error::external)?;
    Ok(Value::String(lua.create_string(&v)?))
}

#[cfg(test)]
mod tests {
    use super::is_text_type;

    #[test]
    fn text_family_decodes_as_string() {
        for t in ["TEXT", "VARCHAR", "CHAR", "NAME", "UNKNOWN", "citext"] {
            assert!(is_text_type(t), "{t}");
        }
    }

    #[test]
    fn other_types_are_rejected() {
        for t in [
            "NUMERIC",
            "TIMESTAMPTZ",
            "INTERVAL",
            "INT4[]",
            "MONEY",
            "JSON",
            "JSONB",
            "UUID",
        ] {
            assert!(!is_text_type(t), "{t}");
        }
    }
}
