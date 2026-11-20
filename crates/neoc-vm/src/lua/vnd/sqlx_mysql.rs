//! MySQL — `lib.sqlx_mysql`, backed by sqlx (pure-Rust driver).
//!
//! Connection strings are standard MySQL URLs:
//! `mysql://user:pass@host:3306/dbname`. **TLS is not enabled in this build**
//! — connect to plaintext servers only (`vnd:hyper` is the only module with a
//! TLS stack). MySQL 8.0+ servers usually require TLS by default; configure
//! the server to allow plaintext or run via a sidecar.
//!
//! ```lua
//! local db, err = lib.sqlx_mysql.open("mysql://root@127.0.0.1/test")
//! db:execute("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(64))")
//! db:execute("INSERT INTO users (id, name) VALUES (?, ?)", 1, "alice")
//! local rows = db:query("SELECT id, name FROM users")
//!
//! -- Hot paths: prepare once, reuse:
//! local insert = db:prepare("INSERT INTO users (id, name) VALUES (?, ?)")
//! insert:execute(1, "alice")
//! db:close()
//! ```
//!
//! Methods are async — calls yield the calling coroutine until the query
//! completes (genuine network IO underneath, worth yielding for, unlike
//! `lib.rusqlite` which is sync C calls). Connections are pooled (default
//! `max_connections = 10`), so the same `Database` handle is safe to share
//! across coroutines that fetch concurrently.

use std::str::FromStr;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Table, UserData, UserDataMethods, Value};
use sqlx::AssertSqlSafe;
use sqlx::mysql::{MySql, MySqlArguments, MySqlConnectOptions, MySqlPool, MySqlRow, MySqlValueRef};
use sqlx::pool::PoolOptions;
use sqlx::query::Query;
use sqlx::{Column, ConnectOptions, Decode, Row, TypeInfo, ValueRef};
use tracing::instrument;
use tracing::log::LevelFilter;

use super::split_sql_args;
use crate::lua::facade::Module;
use crate::lua::lifecycle::{Role, async_function, async_method};
use crate::lua::sandbox::{from_err, i64_to_lua, ok};

/// sqlx caches prepared statements per connection up to this many entries.
/// Default is 100; we bump it to cover apps that issue more distinct SQL
/// strings than that without paying re-prepare cost.
const STATEMENT_CACHE_CAPACITY: usize = 1024;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:sqlx_mysql` surface through the facade.
///
/// `open` is set verbatim with [`crate::lua::facade::Module::set`] rather than
/// `try_async_func` because the hand-written async function carries its own
/// `#[instrument(name = "sqlx_mysql.open", target = "lua::lib::sqlx_mysql")]`
/// attribute and bakes the `sqlx_mysql.open` error prefix into its pre-built
/// `(Value, Value)` return — routing through the facade would add a second span
/// and double the prefix. Its signature is recorded via
/// [`crate::lua::facade::Module::record_fn`]. The `Database` and `Statement`
/// userdata types keep their own `impl UserData` (the `db_op!` / `stmt_op!`
/// async methods, error prefixes, and `mysql.statement.*` op tags unchanged)
/// and are recorded via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:sqlx_mysql")?;

    m.set(
        "open",
        async_function(lua, Role::Wait, |lua, url: String| open(lua, url))?,
    )?;
    m.record_fn(
        "open",
        &[("url", "string")],
        "Result<Database, string>",
        "Open a MySQL connection pool. `url` is a standard MySQL URL: `mysql://user:pass@host:3306/dbname`.",
    );

    m.userdata("Database", |d| {
        d.method(
            "execute",
            &[("sql", "string"), ("...", "LuaValue")],
            "Result<number, string>",
            "Execute a SQL statement, binding trailing arguments to `?` placeholders; returns rows affected.",
        )
        .method(
            "query",
            &[("sql", "string"), ("...", "LuaValue")],
            "Result<LuaValue, string>",
            "Execute a SELECT and return all rows as a 1-indexed table of column-keyed row tables.",
        )
        .method(
            "query_one",
            &[("sql", "string"), ("...", "LuaValue")],
            "Result<LuaValue, string>",
            "Execute a SELECT and return the first matching row, or nil when none matched.",
        )
        .method(
            "prepare",
            &[("sql", "string")],
            "Statement",
            "Prepare a SQL statement for repeated execution, sharing this connection pool.",
        )
        .method(
            "close",
            &[],
            "()",
            "Close all connections in the pool; subsequent method calls on this handle error.",
        );
    });

    m.userdata("Statement", |s| {
        s.method(
            "execute",
            &[("...", "LuaValue")],
            "Result<number, string>",
            "Execute the prepared statement with positional parameters; returns rows affected.",
        )
        .method(
            "query",
            &[("...", "LuaValue")],
            "Result<LuaValue, string>",
            "Execute the prepared SELECT and return all rows.",
        )
        .method(
            "query_one",
            &[("...", "LuaValue")],
            "Result<LuaValue, string>",
            "Execute the prepared SELECT and return the first matching row, or nil when none matched.",
        );
    });

    Ok(m)
}

#[derive(Clone)]
pub struct Database {
    pool: Arc<MySqlPool>,
}

#[instrument(name = "sqlx_mysql.open", target = "lua::lib::sqlx_mysql", skip_all)]
async fn open(lua: Lua, url: String) -> mlua::Result<(Value, Value)> {
    let opts = match MySqlConnectOptions::from_str(&url) {
        Ok(o) => o,
        Err(e) => return from_err(&lua, "sqlx_mysql.open", e),
    };
    // sqlx logs every statement at INFO by default, which dominates trace
    // overhead on hot paths — silence here. Bump statement cache so reused
    // SQL doesn't re-prepare beyond the default 100-entry window.
    let opts = opts
        .statement_cache_capacity(STATEMENT_CACHE_CAPACITY)
        .log_statements(LevelFilter::Off)
        .log_slow_statements(LevelFilter::Off, std::time::Duration::from_secs(1));
    match PoolOptions::<MySql>::new().connect_with(opts).await {
        Ok(pool) => ok(
            &lua,
            Database {
                pool: Arc::new(pool),
            },
        ),
        Err(e) => from_err(&lua, "sqlx_mysql.open", e),
    }
}

macro_rules! db_op {
    ($methods:expr, $name:literal, $op:literal, $run:ident) => {
        async_method(
            $methods,
            $name,
            Role::Wait,
            |lua, this, args: MultiValue| {
                let pool = this.pool.clone();
                async move {
                    let (sql, params) = match split_sql_args(args) {
                        Ok(v) => v,
                        Err(e) => return from_err(&lua, $op, e),
                    };
                    $run(&lua, &pool, $op, &sql, params).await
                }
            },
        );
    };
}

macro_rules! stmt_op {
    ($methods:expr, $name:literal, $op:literal, $run:ident) => {
        async_method(
            $methods,
            $name,
            Role::Wait,
            |lua, this, args: MultiValue| {
                let sql = this.sql.clone();
                let pool = this.pool.clone();
                async move {
                    let params = args.into_iter().collect();
                    $run(&lua, &pool, $op, &sql, params).await
                }
            },
        );
    };
}

impl UserData for Database {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        db_op!(methods, "execute", "sqlx_mysql.execute", run_execute);
        db_op!(methods, "query", "sqlx_mysql.query", run_query);
        db_op!(methods, "query_one", "sqlx_mysql.query_one", run_query_one);

        methods.add_method("prepare", |_, this, sql: String| {
            Ok(Statement {
                sql: Arc::new(sql),
                pool: this.pool.clone(),
            })
        });

        async_method(methods, "close", Role::Wait, |_, this, _: ()| {
            let pool = this.pool.clone();
            async move {
                pool.close().await;
                Ok(())
            }
        });
    }
}

/// Pre-bound SQL — created via `db:prepare(sql)`. Holds the SQL by `Arc`
/// and shares the database's pool. sqlx caches prepared statements per
/// connection, so the practical win on top of plain `db:execute(sql, ...)`
/// is skipping the Lua → Rust SQL string allocation per call.
#[derive(Clone)]
pub struct Statement {
    sql: Arc<String>,
    pool: Arc<MySqlPool>,
}

impl UserData for Statement {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        stmt_op!(methods, "execute", "mysql.statement.execute", run_execute);
        stmt_op!(methods, "query", "mysql.statement.query", run_query);
        stmt_op!(
            methods,
            "query_one",
            "mysql.statement.query_one",
            run_query_one
        );
    }
}

// ---------------------------------------------------------------------------
// shared execution helpers
// ---------------------------------------------------------------------------

/// Reject a closed pool and bind `params` into a ready query. Shared prologue
/// for the three `run_*` helpers; errors stringify so each maps them under its
/// own `op`.
fn prepare_query<'q>(
    pool: &MySqlPool,
    sql: &'q str,
    params: Vec<Value>,
) -> Result<Query<'q, MySql, MySqlArguments>, String> {
    if pool.is_closed() {
        return Err("database is closed".to_string());
    }
    build_query(sql, params)
}

async fn run_execute(
    lua: &Lua,
    pool: &MySqlPool,
    op: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> mlua::Result<(Value, Value)> {
    let q = match prepare_query(pool, sql, params) {
        Ok(q) => q,
        Err(e) => return from_err(lua, op, e),
    };
    match q.execute(pool).await {
        Ok(r) => ok(lua, r.rows_affected() as i64),
        Err(e) => from_err(lua, op, e),
    }
}

async fn run_query(
    lua: &Lua,
    pool: &MySqlPool,
    op: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> mlua::Result<(Value, Value)> {
    let q = match prepare_query(pool, sql, params) {
        Ok(q) => q,
        Err(e) => return from_err(lua, op, e),
    };
    match q.fetch_all(pool).await {
        Ok(rows) => {
            let out = lua.create_table_with_capacity(rows.len(), 0)?;
            for (i, row) in rows.iter().enumerate() {
                out.set(i + 1, row_to_table(lua, row)?)?;
            }
            ok(lua, out)
        }
        Err(e) => from_err(lua, op, e),
    }
}

async fn run_query_one(
    lua: &Lua,
    pool: &MySqlPool,
    op: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> mlua::Result<(Value, Value)> {
    let q = match prepare_query(pool, sql, params) {
        Ok(q) => q,
        Err(e) => return from_err(lua, op, e),
    };
    match q.fetch_optional(pool).await {
        Ok(Some(row)) => ok(lua, row_to_table(lua, &row)?),
        Ok(None) => ok(lua, Value::Nil),
        Err(e) => from_err(lua, op, e),
    }
}

// `AssertSqlSafe`: running caller-supplied SQL text is this module's purpose;
// values reach the database only through bound parameters (sqlx 0.9 requires
// the explicit opt-in for non-literal SQL).
fn build_query<'q>(
    sql: &'q str,
    params: Vec<Value>,
) -> Result<Query<'q, MySql, MySqlArguments>, String> {
    let mut q = sqlx::query(AssertSqlSafe(sql));
    for p in params {
        q = bind_param(q, p)?;
    }
    Ok(q)
}

// ---------------------------------------------------------------------------
// Lua → sqlx
// ---------------------------------------------------------------------------

fn bind_param<'q>(
    q: Query<'q, MySql, MySqlArguments>,
    v: Value,
) -> Result<Query<'q, MySql, MySqlArguments>, String> {
    Ok(match v {
        Value::Nil => q.bind(Option::<i64>::None),
        Value::Boolean(b) => q.bind(b),
        Value::Integer(i) => q.bind(i),
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

/// Pre-classified MySQL column kind. We compute this from the type-info
/// name once so the per-cell match arm doesn't have to (a) inspect the
/// type info twice (the previous code did `try_get_raw` to check null,
/// then `try_get` again for the actual decode) and (b) keep the
/// `MySqlValueRef`'s borrow alive across a substring match.
#[derive(Clone, Copy)]
enum Kind {
    Nil,
    Bool,
    Integer,
    Real,
    Blob,
    Text,
}

fn classify(type_name: &str) -> Kind {
    if type_name.starts_with("BOOLEAN") {
        return Kind::Bool;
    }
    if matches!(type_name, "FLOAT" | "DOUBLE") {
        return Kind::Real;
    }
    if matches!(
        type_name,
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY"
    ) {
        return Kind::Blob;
    }
    // Integer types: TINYINT/SMALLINT/MEDIUMINT/INT/BIGINT, signed or
    // unsigned (e.g. "INT UNSIGNED"). Excludes INTERVAL just in case.
    if type_name.contains("INT") && !type_name.contains("INTERVAL") {
        return Kind::Integer;
    }
    // Default: text. Covers VARCHAR/TEXT/CHAR/JSON/DATE/TIME/DATETIME/
    // TIMESTAMP/DECIMAL/ENUM/SET — sqlx renders temporal/decimal as text
    // unless the relevant feature is on, which we don't enable.
    Kind::Text
}

fn row_to_table(lua: &Lua, row: &MySqlRow) -> mlua::Result<Table> {
    let cols = row.columns();
    let t = lua.create_table_with_capacity(0, cols.len())?;
    for col in cols {
        let raw: MySqlValueRef = row
            .try_get_raw(col.ordinal())
            .map_err(mlua::Error::external)?;
        let kind = if raw.is_null() {
            Kind::Nil
        } else {
            classify(raw.type_info().name())
        };
        t.set(col.name(), decode_cell(lua, kind, raw)?)?;
    }
    Ok(t)
}

/// Decode one cell's raw value into a Lua value per its classified `kind`.
fn decode_cell(lua: &Lua, kind: Kind, raw: MySqlValueRef) -> mlua::Result<Value> {
    let val = match kind {
        Kind::Nil => Value::Nil,
        Kind::Bool => {
            let v: bool = <bool as Decode<MySql>>::decode(raw).map_err(mlua::Error::external)?;
            Value::Boolean(v)
        }
        Kind::Integer => {
            let v: i64 = <i64 as Decode<MySql>>::decode(raw).map_err(mlua::Error::external)?;
            i64_to_lua(v)
        }
        Kind::Real => {
            let v: f64 = <f64 as Decode<MySql>>::decode(raw).map_err(mlua::Error::external)?;
            Value::Number(v)
        }
        Kind::Blob => {
            let v: &[u8] = <&[u8] as Decode<MySql>>::decode(raw).map_err(mlua::Error::external)?;
            Value::String(lua.create_string(v)?)
        }
        Kind::Text => {
            let v: &str = <&str as Decode<MySql>>::decode(raw).map_err(mlua::Error::external)?;
            Value::String(lua.create_string(v)?)
        }
    };
    Ok(val)
}
