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

use crate::lua::sandbox::{from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(
        "open",
        lua.create_async_function(|lua, url: String| open(lua, url))?,
    )?;
    Ok(t)
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
                let (sql, params) = match split_args(args) {
                    Ok(v) => v,
                    Err(e) => return from_err(&lua, "sqlx_postgres.execute", e),
                };
                let mut q = sqlx::query(&sql);
                for p in params {
                    q = match bind_param(q, p) {
                        Ok(q) => q,
                        Err(e) => return from_err(&lua, "sqlx_postgres.execute", e),
                    };
                }
                if pool.is_closed() {
                    return from_err(&lua, "sqlx_postgres.execute", "database is closed");
                }
                match q.execute(&*pool).await {
                    Ok(r) => ok(&lua, r.rows_affected() as i64),
                    Err(e) => from_err(&lua, "sqlx_postgres.execute", e),
                }
            }
        });

        methods.add_async_method("query", |lua, this, args: MultiValue| {
            let pool = this.pool.clone();
            async move {
                let (sql, params) = match split_args(args) {
                    Ok(v) => v,
                    Err(e) => return from_err(&lua, "sqlx_postgres.query", e),
                };
                let mut q = sqlx::query(&sql);
                for p in params {
                    q = match bind_param(q, p) {
                        Ok(q) => q,
                        Err(e) => return from_err(&lua, "sqlx_postgres.query", e),
                    };
                }
                if pool.is_closed() {
                    return from_err(&lua, "sqlx_postgres.query", "database is closed");
                }
                match q.fetch_all(&*pool).await {
                    Ok(rows) => {
                        let out = lua.create_table()?;
                        for (i, row) in rows.iter().enumerate() {
                            out.set(i + 1, row_to_table(&lua, row)?)?;
                        }
                        ok(&lua, out)
                    }
                    Err(e) => from_err(&lua, "sqlx_postgres.query", e),
                }
            }
        });

        methods.add_async_method("query_one", |lua, this, args: MultiValue| {
            let pool = this.pool.clone();
            async move {
                let (sql, params) = match split_args(args) {
                    Ok(v) => v,
                    Err(e) => return from_err(&lua, "sqlx_postgres.query_one", e),
                };
                let mut q = sqlx::query(&sql);
                for p in params {
                    q = match bind_param(q, p) {
                        Ok(q) => q,
                        Err(e) => return from_err(&lua, "sqlx_postgres.query_one", e),
                    };
                }
                if pool.is_closed() {
                    return from_err(&lua, "sqlx_postgres.query_one", "database is closed");
                }
                match q.fetch_optional(&*pool).await {
                    Ok(Some(row)) => ok(&lua, row_to_table(&lua, &row)?),
                    Ok(None) => ok(&lua, Value::Nil),
                    Err(e) => from_err(&lua, "sqlx_postgres.query_one", e),
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
    // Postgres is strict about type widths — INT4 must decode through i32,
    // not i64, and likewise FLOAT4 through f32. Mismatches raise a runtime
    // decoding error rather than coercing.
    match type_name {
        "BOOL" => {
            let v: bool = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::Boolean(v))
        }
        "INT2" => {
            let v: i16 = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::Integer(v as i32))
        }
        "INT4" => {
            let v: i32 = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::Integer(v))
        }
        "INT8" => {
            let v: i64 = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(match i32::try_from(v) {
                Ok(i) => Value::Integer(i),
                Err(_) => Value::Number(v as f64),
            })
        }
        "FLOAT4" => {
            let v: f32 = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::Number(v as f64))
        }
        "FLOAT8" => {
            let v: f64 = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::Number(v))
        }
        "BYTEA" => {
            let v: Vec<u8> = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::String(lua.create_string(&v)?))
        }
        // TEXT family + JSON/JSONB/UUID + temporal types fall through to a
        // text decode. NUMERIC requires the `bigdecimal`/`rust_decimal`
        // sqlx feature and is not handled here.
        _ => {
            let v: String = row.try_get(idx).map_err(mlua::Error::external)?;
            Ok(Value::String(lua.create_string(&v)?))
        }
    }
}
