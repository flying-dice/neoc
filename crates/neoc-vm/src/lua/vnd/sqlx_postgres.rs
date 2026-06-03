//! PostgreSQL — `lib.sqlx_postgres`, backed by sqlx (pure-Rust driver).
//!
//! Connection strings are standard Postgres URLs:
//! `postgres://user:pass@host:5432/dbname`. **TLS is not enabled in this
//! build** — connect to plaintext servers only (consistent with `lib.hyper`
//! being HTTP-only).
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
//! Postgres is strictly typed at bind time — to insert SQL `NULL` into a
//! non-text column, prefer the `NULL` literal in SQL over passing `nil` (we
//! bind `nil` as `Option::<i64>::None`, which the server rejects against
//! e.g. `TEXT` columns). All other casts work as usual: `INSERT … VALUES
//! ($1::int)`.

use std::str::FromStr;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Table, UserData, UserDataMethods, Value};
use sqlx::pool::PoolOptions;
use sqlx::postgres::{PgArguments, PgConnectOptions, PgPool, PgRow, Postgres};
use sqlx::query::Query;
use sqlx::{Column, Row, TypeInfo, ValueRef};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:sqlx_postgres` surface through the facade.
///
/// `open` is set verbatim with [`crate::lua::facade::Module::set`] rather than
/// `try_async_func` because the hand-written async function carries its own
/// `#[instrument(name = "sqlx_postgres.open", target = "luau::lib::sqlx_postgres")]`
/// attribute and bakes the `sqlx_postgres.open` error prefix into its pre-built
/// `(Value, Value)` return — routing through the facade would add a second span
/// and double the prefix. Its signature is recorded for the header via
/// [`crate::lua::facade::Module::record_fn`]. The `Database` userdata type keeps
/// its own `impl UserData` (the async methods and `sqlx_postgres.*` error
/// prefixes unchanged) and is recorded for the header via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:sqlx_postgres")?;

    m.set(
        "open",
        lua.create_async_function(|lua, url: String| open(lua, url))?,
    )?;
    m.record_fn(
        "open",
        &[("url", "string")],
        "Result<Database, string>",
        "Open a Postgres connection pool. `url` is a standard Postgres URL: `postgres://user:pass@host:5432/dbname`.",
    );

    m.userdata("Database", |d| {
        d.method(
            "execute",
            &[("sql", "string"), ("params", "LuaValue")],
            "Result<number, string>",
            "Execute a SQL statement, binding trailing arguments to `$1, $2, …` placeholders; returns rows affected.",
        )
        .method(
            "query",
            &[("sql", "string"), ("params", "LuaValue")],
            "Result<LuaValue, string>",
            "Execute a SELECT and return all rows as a 1-indexed table of column-keyed row tables.",
        )
        .method(
            "query_one",
            &[("sql", "string"), ("params", "LuaValue")],
            "Result<LuaValue, string>",
            "Execute a SELECT and return the first matching row, or nil when none matched.",
        )
        .method(
            "close",
            &[],
            "()",
            "Close all connections in the pool; subsequent method calls on this handle error.",
        );
    });

    Ok(m)
}

#[derive(Clone)]
pub struct Database {
    pool: Arc<PgPool>,
}

#[instrument(
    name = "sqlx_postgres.open",
    target = "luau::lib::sqlx_postgres",
    skip_all
)]
async fn open(lua: Lua, url: String) -> mlua::Result<(Value, Value)> {
    let opts = match PgConnectOptions::from_str(&url) {
        Ok(o) => o,
        Err(e) => return from_err(&lua, "sqlx_postgres.open", e),
    };
    match PoolOptions::<Postgres>::new().connect_with(opts).await {
        Ok(pool) => ok(
            &lua,
            Database {
                pool: Arc::new(pool),
            },
        ),
        Err(e) => from_err(&lua, "sqlx_postgres.open", e),
    }
}

impl UserData for Database {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("execute", |lua, this, args: MultiValue| {
            let pool = this.pool.clone();
            async move {
                let op = "sqlx_postgres.execute";
                let (sql, params) = match prepare(args, &pool) {
                    Ok(v) => v,
                    Err(e) => return from_err(&lua, op, e),
                };
                let q = match build_query(&sql, params) {
                    Ok(q) => q,
                    Err(e) => return from_err(&lua, op, e),
                };
                match q.execute(&*pool).await {
                    Ok(r) => ok(&lua, r.rows_affected() as i64),
                    Err(e) => from_err(&lua, op, e),
                }
            }
        });

        methods.add_async_method("query", |lua, this, args: MultiValue| {
            let pool = this.pool.clone();
            async move {
                let op = "sqlx_postgres.query";
                let (sql, params) = match prepare(args, &pool) {
                    Ok(v) => v,
                    Err(e) => return from_err(&lua, op, e),
                };
                let q = match build_query(&sql, params) {
                    Ok(q) => q,
                    Err(e) => return from_err(&lua, op, e),
                };
                match q.fetch_all(&*pool).await {
                    Ok(rows) => {
                        let out = lua.create_table()?;
                        for (i, row) in rows.iter().enumerate() {
                            out.set(i + 1, row_to_table(&lua, row)?)?;
                        }
                        ok(&lua, out)
                    }
                    Err(e) => from_err(&lua, op, e),
                }
            }
        });

        methods.add_async_method("query_one", |lua, this, args: MultiValue| {
            let pool = this.pool.clone();
            async move {
                let op = "sqlx_postgres.query_one";
                let (sql, params) = match prepare(args, &pool) {
                    Ok(v) => v,
                    Err(e) => return from_err(&lua, op, e),
                };
                let q = match build_query(&sql, params) {
                    Ok(q) => q,
                    Err(e) => return from_err(&lua, op, e),
                };
                match q.fetch_optional(&*pool).await {
                    Ok(Some(row)) => ok(&lua, row_to_table(&lua, &row)?),
                    Ok(None) => ok(&lua, Value::Nil),
                    Err(e) => from_err(&lua, op, e),
                }
            }
        });

        methods.add_async_method("close", |_, this, _: ()| {
            let pool = this.pool.clone();
            async move {
                pool.close().await;
                Ok(())
            }
        });
    }
}

fn prepare(args: MultiValue, pool: &PgPool) -> Result<(String, Vec<Value>), String> {
    let (sql, params) = split_args(args)?;
    if pool.is_closed() {
        return Err("database is closed".into());
    }
    Ok((sql, params))
}

fn build_query<'q>(
    sql: &'q str,
    params: Vec<Value>,
) -> Result<Query<'q, Postgres, PgArguments>, String> {
    params.into_iter().try_fold(sqlx::query(sql), bind_param)
}

// ---------------------------------------------------------------------------
// Lua → sqlx
// ---------------------------------------------------------------------------

fn split_args(mut args: MultiValue) -> Result<(String, Vec<Value>), String> {
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

fn bind_param<'q>(
    q: Query<'q, Postgres, PgArguments>,
    v: Value,
) -> Result<Query<'q, Postgres, PgArguments>, String> {
    Ok(match v {
        Value::Nil => q.bind(Option::<i64>::None),
        Value::Boolean(b) => q.bind(b),
        Value::Integer(i) => q.bind(i as i64),
        Value::Number(f) => q.bind(f),
        // TODO: clean-code - 0.6 - KISS: implicit utf8→text / non-utf8→bytea fallback hides bind type from caller; consider explicit bytes type
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
        None => decode_text(lua, row, idx),
    }
}

/// Decode a column whose Postgres type maps to a fixed Lua scalar. Returns
/// `None` for any type not in this table — the caller falls back to a text
/// decode. Postgres is strict about widths (INT4 via i32, FLOAT4 via f32, …);
/// a mismatch raises a decode error rather than coercing.
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
            Value::Integer(n as i32)
        }
        "INT4" => {
            let n: i32 = row.try_get(idx).map_err(mlua::Error::external)?;
            Value::Integer(n)
        }
        "INT8" => {
            let n: i64 = row.try_get(idx).map_err(mlua::Error::external)?;
            match i32::try_from(n) {
                Ok(i) => Value::Integer(i),
                Err(_) => Value::Number(n as f64),
            }
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

/// Text fallback for any type `decode_known` doesn't recognise: the TEXT family
/// plus JSON/JSONB/UUID and temporal types decode through `String`. NUMERIC
/// needs the `bigdecimal`/`rust_decimal` sqlx feature and is not handled.
fn decode_text(lua: &Lua, row: &PgRow, idx: usize) -> mlua::Result<Value> {
    let v: String = row.try_get(idx).map_err(mlua::Error::external)?;
    Ok(Value::String(lua.create_string(&v)?))
}
