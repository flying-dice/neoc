//! SQLite — `lib.rusqlite`, backed by rusqlite (sync, vendored libsqlite3).
//!
//! ```lua
//! local db, err = lib.rusqlite.open(":memory:")
//! db:execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
//! local n, err = db:execute("INSERT INTO users (name) VALUES (?)", "alice")
//! local rows, err = db:query("SELECT id, name FROM users")
//! -- rows[1].id == 1, rows[1].name == "alice"
//! local row, err = db:query_one("SELECT * FROM users WHERE id = ?", 1)
//!
//! -- For hot paths, prepare once and reuse:
//! local insert = db:prepare("INSERT INTO users (name) VALUES (?)")
//! insert:execute("alice")
//! insert:execute("bob")
//! db:close()
//! ```
//!
//! ## One statement per call
//!
//! `execute`, `query` and `query_one` (on a `Database` and on a `Statement`)
//! take exactly one SQL statement. A second statement is an error
//! (`rusqlite.execute: Multiple statements provided`), as is SQL that is empty
//! or only a comment (`SQL is empty or comment only`). rusqlite 0.32 silently
//! ran the first statement and dropped the rest. There is no `execute_batch`:
//! run a script one statement per call.
//!
//! ## Why sync (and not async)
//!
//! libsqlite3 is a synchronous C library — every call returns instantly with
//! no IO yield, even for file-backed databases (it just hits the kernel page
//! cache). Wrapping that in `add_async_method` would add per-call cost for
//! the future state machine, the Lua coroutine yield/resume, and Tokio task
//! coordination — measurable µs of pure ceremony for a query that takes
//! sub-µs to actually execute. So all methods here are sync (`add_method`),
//! and call rusqlite directly. The previous sqlx-async backend benchmarked
//! at roughly a quarter of what the sync path can do for in-memory
//! workloads.
//!
//! Trade-off: a slow query (giant SELECT, fsync-heavy commit on a slow
//! disk) holds the calling Tokio worker for its full duration. For typical
//! application use this is fine; if you ever hit a multi-millisecond query
//! on a high-concurrency path, structure the code so it runs in a
//! `tokio::task::spawn_blocking` block at the call site.
//!
//! ## Connection model
//!
//! Each `Database` wraps a single `rusqlite::Connection` behind a `parking_lot::Mutex`. mlua's `send`
//! feature requires `UserData: Send + Sync`, and `rusqlite::Connection` is `Send` but not `Sync`, so the
//! mutex supplies the `Sync` half safely. Every method here is `add_method` (sync) and Lua coroutines on
//! one engine never run concurrently, so the lock is uncontended in practice: it is held for one
//! statement and never across a yield. SQLite's own per-connection mutex is off (`OPEN_NO_MUTEX`)
//! because this one serialises access already.
//!
//! Statement caching uses rusqlite's built-in `prepare_cached` so
//! re-issuing the same SQL skips the parse.
//!
//! ## `Statement`
//!
//! `Statement` is a thin wrapper that holds the SQL string by `Arc` and
//! shares the database's connection. Calls funnel through the same
//! `prepare_cached` path as `db:execute(sql, ...)`, so the perf delta
//! between the two is whether the Lua → Rust crossing has to re-allocate
//! the SQL string. Use `prepare()` for hot paths.

use std::sync::Arc;

use mlua::{Lua, MultiValue, Table, UserData, UserDataMethods, Value};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use rusqlite::OpenFlags;
use rusqlite::types::{ToSql, ToSqlOutput, Value as SqlValue, ValueRef};
use tracing::instrument;

use super::split_sql_args;
use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, i64_to_lua, ok};

/// A rusqlite connection behind a mutex. `None` once `:close()` has dropped it.
struct SharedConn(Mutex<Option<rusqlite::Connection>>);

impl SharedConn {
    fn new(conn: rusqlite::Connection) -> Self {
        Self(Mutex::new(Some(conn)))
    }

    /// Lock the connection, or report that the database was closed.
    fn lock(&self) -> Result<MappedMutexGuard<'_, rusqlite::Connection>, &'static str> {
        MutexGuard::try_map(self.0.lock(), Option::as_mut).map_err(|_| "database is closed")
    }

    /// Drop the connection (rusqlite closes it); later calls report "database is closed".
    fn close(&self) {
        drop(self.0.lock().take());
    }
}

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:rusqlite` surface through the facade.
///
/// `open` is set verbatim with [`crate::lua::facade::Module::set`] rather than
/// `try_func` because the hand-written function carries its own
/// `#[instrument(name = "rusqlite.open", target = "lua::lib::rusqlite")]`
/// attribute and bakes the `rusqlite.open` error prefix — routing through
/// `try_func` would add a second span and double the prefix. Its signature is
/// read by `neoc_macros` from the implementation. The `Database` and
/// `Statement` userdata types keep their own `impl UserData` (methods, error
/// prefixes, and the `rusqlite.statement.*` op tags unchanged) and are recorded
/// via [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:rusqlite")?;

    m.declared(
        open::lua(),
        lua.create_function(|lua, path: String| open(lua, path))?,
    )?;

    m.userdata("Database", |d| {
        Database::lua(d);
    });

    m.userdata("Statement", |s| {
        Statement::lua(s);
    });

    Ok(m)
}

#[derive(Clone)]
pub struct Database {
    /// `Arc` so `prepare()` can hand a `Statement` a co-owning reference; the
    /// connection inside is `None` once `:close()` ran. See module docs.
    conn: Arc<SharedConn>,
}

/// Open a SQLite database file. Pass `":memory:"` for an in-memory database.
#[neoc_macros::lua_fn(returns = "Result<Database, string>")]
#[instrument(name = "rusqlite.open", target = "lua::lib::rusqlite", skip_all)]
fn open(lua: &Lua, path: String) -> mlua::Result<(Value, Value)> {
    // OPEN_NO_MUTEX: `SharedConn` serialises access, SQLite's own per-connection
    // mutex would just be redundant work.
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    // `:memory:` needs no special-casing — sqlite treats it identically whether
    // the literal or a variable holding it is passed.
    let conn = rusqlite::Connection::open_with_flags(&path, flags);
    match conn {
        Ok(c) => ok(
            lua,
            Database {
                conn: Arc::new(SharedConn::new(c)),
            },
        ),
        Err(e) => from_err(lua, "rusqlite.open", e),
    }
}

#[neoc_macros::lua_userdata]
impl UserData for Database {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Execute a SQL statement, binding trailing arguments to `?` placeholders; returns rows affected.
        #[lua("Result<number, string>", takes("sql: string", "...: LuaValue"))]
        methods.add_method("execute", |lua, this, args: MultiValue| {
            db_dispatch(lua, &this.conn, "rusqlite.execute", args, run_execute)
        });

        /// Execute a SELECT and return all rows as a 1-indexed table of column-keyed row tables.
        #[lua("Result<LuaValue, string>", takes("sql: string", "...: LuaValue"))]
        methods.add_method("query", |lua, this, args: MultiValue| {
            db_dispatch(lua, &this.conn, "rusqlite.query", args, run_query)
        });

        /// Execute a SELECT and return the first matching row, or nil when none matched.
        #[lua("Result<LuaValue, string>", takes("sql: string", "...: LuaValue"))]
        methods.add_method("query_one", |lua, this, args: MultiValue| {
            db_dispatch(lua, &this.conn, "rusqlite.query_one", args, run_query_one)
        });

        /// Prepare a SQL statement for repeated execution, sharing this connection.
        #[lua("Statement")]
        methods.add_method("prepare", |_, this, sql: String| {
            Ok(Statement {
                sql: Arc::new(sql),
                conn: this.conn.clone(),
            })
        });

        /// Close the connection; subsequent method calls on this handle error.
        #[lua("()")]
        methods.add_method("close", |_, this, _: ()| {
            // Take the connection out so the next call lands on the
            // "database is closed" branch in the helpers below.
            this.conn.close();
            Ok(())
        });
    }
}

/// Pre-bound SQL — created via `db:prepare(sql)`. Holds the SQL by `Arc`
/// and shares the originating database's connection. Calls go through
/// rusqlite's `prepare_cached`, the same path `db:execute(sql, ...)` uses;
/// the win is skipping the Lua → Rust SQL string allocation per call.
#[derive(Clone)]
pub struct Statement {
    sql: Arc<String>,
    conn: Arc<SharedConn>,
}

#[neoc_macros::lua_userdata]
impl UserData for Statement {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Execute the prepared statement with positional parameters; returns rows affected.
        #[lua("Result<number, string>", takes("...: LuaValue"))]
        methods.add_method("execute", |lua, this, args: MultiValue| {
            let params: Vec<Value> = args.into_iter().collect();
            run_execute(
                lua,
                &this.conn,
                "rusqlite.statement.execute",
                &this.sql,
                params,
            )
        });

        /// Execute the prepared SELECT and return all rows.
        #[lua("Result<LuaValue, string>", takes("...: LuaValue"))]
        methods.add_method("query", |lua, this, args: MultiValue| {
            let params: Vec<Value> = args.into_iter().collect();
            run_query(
                lua,
                &this.conn,
                "rusqlite.statement.query",
                &this.sql,
                params,
            )
        });

        /// Execute the prepared SELECT and return the first matching row, or nil when none matched.
        #[lua("Result<LuaValue, string>", takes("...: LuaValue"))]
        methods.add_method("query_one", |lua, this, args: MultiValue| {
            let params: Vec<Value> = args.into_iter().collect();
            run_query_one(
                lua,
                &this.conn,
                "rusqlite.statement.query_one",
                &this.sql,
                params,
            )
        });
    }
}

// ---------------------------------------------------------------------------
// shared execution helpers
// ---------------------------------------------------------------------------

/// A `run_*` helper: prepare + run the SQL, shaping the result for its op.
type RunFn =
    fn(&Lua, &Arc<SharedConn>, &'static str, &str, Vec<Value>) -> mlua::Result<(Value, Value)>;

/// Split a `db:execute`/`query`/`query_one` call's args into SQL + params and
/// dispatch to the matching `run_*` helper. The three `Database` methods differ
/// only by op name and which helper shapes the result.
fn db_dispatch(
    lua: &Lua,
    conn: &Arc<SharedConn>,
    op: &'static str,
    args: MultiValue,
    run: RunFn,
) -> mlua::Result<(Value, Value)> {
    let (sql, params) = match split_sql_args(args) {
        Ok(v) => v,
        Err(e) => return from_err(lua, op, e),
    };
    run(lua, conn, op, &sql, params)
}

fn to_sql_refs(lua_params: &[LuaToSql]) -> Vec<&dyn ToSql> {
    // rusqlite's `params_from_iter` takes any iterator of &dyn ToSql; we
    // materialize them into a Vec<&dyn ToSql> first so the borrow lifetimes
    // line up cleanly with prepare_cached's borrow of conn.
    lua_params.iter().map(|p| p as &dyn ToSql).collect()
}

fn column_names(stmt: &rusqlite::Statement<'_>) -> Vec<String> {
    (0..stmt.column_count())
        .map(|i| stmt.column_name(i).unwrap_or("").to_owned())
        .collect()
}

fn materialize_row(
    row: &rusqlite::Row<'_>,
    col_names: &[String],
) -> rusqlite::Result<Vec<(String, SqlValue)>> {
    let mut out = Vec::with_capacity(col_names.len());
    for (i, name) in col_names.iter().enumerate() {
        let v = row.get_ref(i)?;
        out.push((name.clone(), value_ref_to_owned(v)?));
    }
    Ok(out)
}

fn run_execute(
    lua: &Lua,
    handle: &Arc<SharedConn>,
    op: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> mlua::Result<(Value, Value)> {
    let conn = match handle.lock() {
        Ok(c) => c,
        Err(e) => return from_err(lua, op, e),
    };
    let lua_params: Vec<LuaToSql> = params.into_iter().map(LuaToSql).collect();
    let result = (|| -> rusqlite::Result<u64> {
        let mut stmt = conn.prepare_cached(sql)?;
        let refs = to_sql_refs(&lua_params);
        let n = stmt.execute(rusqlite::params_from_iter(refs.iter().copied()))?;
        Ok(n as u64)
    })();
    match result {
        Ok(n) => ok(lua, n as i64),
        Err(e) => from_err(lua, op, e),
    }
}

fn run_query(
    lua: &Lua,
    handle: &Arc<SharedConn>,
    op: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> mlua::Result<(Value, Value)> {
    let conn = match handle.lock() {
        Ok(c) => c,
        Err(e) => return from_err(lua, op, e),
    };
    let lua_params: Vec<LuaToSql> = params.into_iter().map(LuaToSql).collect();
    let result = (|| -> rusqlite::Result<Vec<Vec<(String, SqlValue)>>> {
        let mut stmt = conn.prepare_cached(sql)?;
        let col_names = column_names(&stmt);
        let refs = to_sql_refs(&lua_params);
        stmt.query_map(rusqlite::params_from_iter(refs.iter().copied()), |row| {
            materialize_row(row, &col_names)
        })?
        .collect()
    })();
    match result {
        Ok(rows) => {
            let out = lua.create_table_with_capacity(rows.len(), 0)?;
            for (i, row) in rows.into_iter().enumerate() {
                out.set(i + 1, row_to_lua_table(lua, row)?)?;
            }
            ok(lua, out)
        }
        Err(e) => from_err(lua, op, e),
    }
}

fn run_query_one(
    lua: &Lua,
    handle: &Arc<SharedConn>,
    op: &'static str,
    sql: &str,
    params: Vec<Value>,
) -> mlua::Result<(Value, Value)> {
    let conn = match handle.lock() {
        Ok(c) => c,
        Err(e) => return from_err(lua, op, e),
    };
    let lua_params: Vec<LuaToSql> = params.into_iter().map(LuaToSql).collect();
    let result = (|| -> rusqlite::Result<Option<Vec<(String, SqlValue)>>> {
        let mut stmt = conn.prepare_cached(sql)?;
        let col_names = column_names(&stmt);
        let refs = to_sql_refs(&lua_params);
        let mut rows = stmt.query(rusqlite::params_from_iter(refs.iter().copied()))?;
        rows.next()?
            .map(|row| materialize_row(row, &col_names))
            .transpose()
    })();
    match result {
        Ok(Some(row)) => ok(lua, row_to_lua_table(lua, row)?),
        Ok(None) => ok(lua, Value::Nil),
        Err(e) => from_err(lua, op, e),
    }
}

// ---------------------------------------------------------------------------
// Lua ⇄ rusqlite type bridges
// ---------------------------------------------------------------------------

/// Wrapper that lets a Lua `Value` bind into rusqlite without copying for
/// numeric/nil cases; strings have to be owned because `s.as_bytes()`
/// borrows the Lua string and we don't keep that borrow live across the
/// statement execution.
struct LuaToSql(Value);

impl ToSql for LuaToSql {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match &self.0 {
            Value::Nil => Ok(ToSqlOutput::Owned(SqlValue::Null)),
            Value::Boolean(b) => Ok(ToSqlOutput::Owned(SqlValue::Integer(if *b {
                1
            } else {
                0
            }))),
            Value::Integer(i) => Ok(ToSqlOutput::Owned(SqlValue::Integer(*i))),
            Value::Number(f) => Ok(ToSqlOutput::Owned(SqlValue::Real(*f))),
            Value::String(s) => {
                let bytes = s.as_bytes();
                // valid utf-8 → TEXT; otherwise BLOB. Lua strings are
                // byte-clean so blobs round-trip through `Value::String`
                // on read as well.
                match std::str::from_utf8(&bytes) {
                    Ok(text) => Ok(ToSqlOutput::Owned(SqlValue::Text(text.to_owned()))),
                    Err(_) => Ok(ToSqlOutput::Owned(SqlValue::Blob(bytes.to_vec()))),
                }
            }
            other => Err(rusqlite::Error::ToSqlConversionFailure(
                format!("cannot bind Lua {} as SQL parameter", other.type_name()).into(),
            )),
        }
    }
}

fn value_ref_to_owned(v: ValueRef<'_>) -> rusqlite::Result<SqlValue> {
    Ok(match v {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(i) => SqlValue::Integer(i),
        ValueRef::Real(f) => SqlValue::Real(f),
        ValueRef::Text(t) => SqlValue::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => SqlValue::Blob(b.to_vec()),
    })
}

fn row_to_lua_table(lua: &Lua, row: Vec<(String, SqlValue)>) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(0, row.len())?;
    for (name, v) in row {
        let val = match v {
            SqlValue::Null => Value::Nil,
            SqlValue::Integer(i) => i64_to_lua(i),
            SqlValue::Real(f) => Value::Number(f),
            SqlValue::Text(s) => Value::String(lua.create_string(&s)?),
            SqlValue::Blob(b) => Value::String(lua.create_string(&b)?),
        };
        t.set(name, val)?;
    }
    Ok(t)
}
