//! `app.db`: migrations, statements and transactions; `app.kv`: a small durable key-value store.
//!
//! ```lua
//! local db = app.db
//! db:migrate { "create table notes(...)" }
//! db:exec("insert into notes(body) values (:b)", { b = "hi" })     -- { changes, last_insert_id }
//! local rows = db:all("select * from notes where id > :n", { n = 3 })
//! local row  = db:one("select * from notes where id = :id", { id = 7 })
//! db:tx(function() db:exec(...); db:exec(...) end)                -- one commit
//! ```
//!
//! Every function takes the app's turn before its body runs (`#[host("Vm")]`, `lifecycle::Role::Vm`).

use super::{host, json::Json, shared};
use crate::{
    convert::json_to_params,
    tx::{answer, OpenTx, Reply},
    vm::Shared,
};
use mlua_ir::{Callback, Live, Object, Shape, Unknown};
use neoc_vm2::lifecycle::turn_id;
use neoc_vm2::mlua::{self, Lua, Value as LuaValue};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use vos_application::data::{ExecOutcome, Params, SqlMigration};

impl Shared {
    /// The gate every write passes: the call is not a `readOnly` tool (the host's read-only handle would
    /// refuse the statement too, this gives the app an error that says why).
    pub(crate) fn require_write(&self, what: &str) -> mlua::Result<()> {
        self.writable(what)
    }

    /// Queue a statement on the open transaction, when there is one: a transaction belongs to the turn that opened
    /// it, and a statement from any other call is refused, never run inside it. The statement is handed back when
    /// there is no transaction. Nothing is held across the wait for the answer.
    fn in_open_tx<A, T>(
        &self,
        statement: A,
        queue: impl FnOnce(&OpenTx, A) -> Reply<T>,
    ) -> mlua::Result<Result<Reply<T>, A>> {
        let open = self.tx.lock();
        let Some(tx) = open.as_ref() else { return Ok(Err(statement)) };
        if turn_id() != Some(tx.owner()) {
            return Err(mlua::Error::runtime("another call's db:tx is open: wait for it to finish"));
        }
        Ok(Ok(queue(tx, statement)))
    }

    /// A read: through the open transaction when there is one.
    async fn run_query(&self, sql: String, params: Params) -> mlua::Result<Vec<Value>> {
        match self.in_open_tx((sql, params), |tx, (sql, params)| tx.query(sql, params))? {
            Ok(reply) => await_reply(self, reply).await,
            Err((sql, params)) => host(self, self.database().query(sql, params)).await,
        }
    }

    /// A write: through the open transaction when there is one.
    async fn run_exec(&self, sql: String, params: Params) -> mlua::Result<ExecOutcome> {
        match self.in_open_tx((sql, params), |tx, (sql, params)| tx.exec(sql, params))? {
            Ok(reply) => await_reply(self, reply).await,
            Err((sql, params)) => host(self, self.database().exec(sql, params)).await,
        }
    }
}

/// Wait for what a transaction's task answers.
async fn await_reply<T>(sh: &Shared, reply: Reply<T>) -> mlua::Result<T> {
    host(sh, answer(reply)).await
}

mlua_ir::unknown_reason!(
    /// Why a bound value has no declared shape.
    SqlValue = "a value bound to a statement: whatever the column takes (a table is stored as JSON text)"
);
mlua_ir::unknown_reason!(
    /// Why a column's value has no declared shape.
    Column = "a column's value: its type is the table's, which the app's own migrations define"
);
mlua_ir::unknown_reason!(
    /// Why a transaction's result has no declared shape.
    TxResult = "whatever the body of the transaction returned"
);
mlua_ir::unknown_reason!(
    /// Why a stored value has no declared shape.
    Stored = "whatever JSON value the app stored under the key"
);

/// The named binds of a statement: `{ id = 7 }` binds `:id`.
pub type SqlParams = Json<BTreeMap<String, Unknown<SqlValue>>>;

/// A row: its columns by name.
pub type Row = Json<BTreeMap<String, Unknown<Column>>>;

/// What a write did.
#[derive(Clone, Copy, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct ExecResult {
    /// How many rows the statement changed.
    pub changes: u64,
    /// The row id of the last insert.
    pub last_insert_id: i64,
}

fn params(given: Option<SqlParams>) -> mlua::Result<Params> {
    given.map_or(Ok(Params::None), |given| json_to_params(given.value))
}

/// The app's database (SQLite), its own and no other app's.
#[derive(Clone, Copy, Debug, Object)]
#[shape(module = "vos:app")]
pub struct Db;

#[mlua_ir::object(module = "vos:app")]
impl Db {
    /// Bring the schema up to date: `steps` are SQL scripts in order, each run once, ever (`steps[3]` is version
    /// 3). Add a step to change the schema; never edit one that has run. Raises inside `db:tx`.
    #[host("Vm")]
    pub async fn migrate(&self, lua: Lua, steps: Vec<String>) -> mlua::Result<()> {
        let s = shared(&lua)?;
        s.require_write("db:migrate")?;
        if s.tx.lock().is_some() {
            // the transaction holds the database's write gate, and a migration waits for that same gate
            return Err(mlua::Error::runtime(
                "db:migrate cannot run inside db:tx: migrate before the transaction starts",
            ));
        }
        let mut migrations = Vec::with_capacity(steps.len());
        for (version, sql) in (1_u32..).zip(steps) {
            migrations.push(SqlMigration { version, name: format!("migration {version}"), sql });
        }
        host(&s, s.db.migrate(migrations)).await
    }

    /// Run a statement that writes. Raises when the statement fails, and inside a `readOnly` tool.
    #[host("Vm")]
    pub async fn exec(&self, lua: Lua, sql: String, params: Option<SqlParams>) -> mlua::Result<ExecResult> {
        let s = shared(&lua)?;
        s.require_write("db:exec")?;
        let out = s.run_exec(sql, self::params(params)?).await?;
        Ok(ExecResult { changes: out.changes, last_insert_id: out.last_insert_id })
    }

    /// Every row of a query.
    #[host("Vm")]
    pub async fn all(
        &self,
        lua: Lua,
        sql: String,
        params: Option<SqlParams>,
    ) -> mlua::Result<Json<Vec<Row>>> {
        let s = shared(&lua)?;
        let rows = s.run_query(sql, self::params(params)?).await?;
        Ok(Json::new(Value::Array(rows)))
    }

    /// The first row of a query, or `nil` when there is none.
    #[host("Vm")]
    pub async fn one(&self, lua: Lua, sql: String, params: Option<SqlParams>) -> mlua::Result<Option<Row>> {
        let s = shared(&lua)?;
        let mut rows = s.run_query(sql, self::params(params)?).await?;
        rows.truncate(1);
        Ok(rows.pop().map(Json::new))
    }

    /// Run `body` in a transaction: the statements inside commit together or not at all (an error raised in the
    /// body rolls them back and is raised again). Returns what the body returned. Cannot be nested.
    #[host("Vm")]
    pub async fn tx(&self, lua: Lua, body: Callback<fn() -> Live<TxResult>>) -> mlua::Result<Live<TxResult>> {
        let s = shared(&lua)?;
        // the call holds the turn here (`Role::Vm`): the checks below, the transaction and the `readOnly`
        // flag `database()` reads belong to it (a task started inside another `db:tx` waited for it)
        if s.tx.lock().is_some() {
            return Err(mlua::Error::runtime("db:tx cannot be nested"));
        }
        let begun = host(&s, s.database().begin()).await?;
        // the call that finishes beginning (the same one, unless the host call was left in flight) owns it
        let owner = turn_id().ok_or_else(|| mlua::Error::runtime("db:tx needs a call to belong to"))?;
        *s.tx.lock() = Some(OpenTx::start(&s.life, begun, owner));
        let mut guard = TxGuard { shared: &s, owner: Some(owner) };
        let result = body.function().call_async::<LuaValue>(()).await;
        // whatever the body left in flight is queued ahead of the end, in the transaction
        match (result, guard.release()) {
            (Ok(value), Some(tx)) => {
                await_reply(&s, tx.end(true)).await?;
                Ok(Live::new(value))
            }
            (Err(e), Some(tx)) => {
                if let Err(rollback) = await_reply(&s, tx.end(false)).await {
                    tracing::warn!(app = %s.app, error = %rollback, "rolling back a failed db:tx failed");
                }
                Err(e)
            }
            (Ok(_), None) => Err(mlua::Error::runtime(
                "transaction lost: the call that opened it ended before the body finished, so it was rolled back",
            )),
            (Err(e), None) => Err(e),
        }
    }
}

/// The open `db:tx` of a host call: if the call is dropped (abandoned) before it ends the transaction, the
/// transaction is rolled back, so nothing is left open that no call is finishing.
struct TxGuard<'a> {
    shared: &'a Shared,
    owner: Option<u64>,
}

impl TxGuard<'_> {
    /// The transaction, if it is still this call's (the turn that opened it may have ended and rolled it back).
    fn release(&mut self) -> Option<OpenTx> {
        let owner = self.owner.take()?;
        let mut slot = self.shared.tx.lock();
        if slot.as_ref().is_some_and(|tx| tx.owner() == owner) {
            slot.take()
        } else {
            None
        }
    }
}

impl Drop for TxGuard<'_> {
    fn drop(&mut self) {
        drop(self.release());
    }
}

/// JSON values in the app's database, by key: for a setting or a cursor, not for records (use `app.db`).
#[derive(Clone, Copy, Debug, Object)]
#[shape(module = "vos:app")]
pub struct Kv;

#[mlua_ir::object(module = "vos:app")]
impl Kv {
    /// The value stored under `key`, or `nil`.
    #[host("Vm")]
    pub async fn get(&self, lua: Lua, key: String) -> mlua::Result<Option<Json<Unknown<Stored>>>> {
        let s = shared(&lua)?;
        let rows = s
            .run_query("select value from _kv where key = :k".into(), Params::named([("k", json!(key))]))
            .await?;
        match rows.first().and_then(|r| r["value"].as_str()) {
            Some(text) => Ok(Some(Json::new(serde_json::from_str(text).map_err(mlua::Error::external)?))),
            None => Ok(None),
        }
    }

    /// Store `value` (any JSON value) under `key`; `nil` deletes the key. Raises inside a `readOnly` tool.
    #[host("Vm")]
    pub async fn set(&self, lua: Lua, key: String, value: Json<Unknown<Stored>>) -> mlua::Result<()> {
        let s = shared(&lua)?;
        s.require_write("app.kv:set")?;
        if value.value.is_null() {
            s.run_exec("delete from _kv where key = :k".into(), Params::named([("k", json!(key))])).await?;
        } else {
            s.run_exec(
                "insert into _kv(key, value) values(:k, :v) on conflict(key) do update set value = excluded.value".into(),
                Params::named([("k", json!(key)), ("v", json!(value.value.to_string()))]),
            )
            .await?;
        }
        Ok(())
    }

    /// Delete `key`. Raises inside a `readOnly` tool.
    #[host("Vm")]
    pub async fn delete(&self, lua: Lua, key: String) -> mlua::Result<()> {
        let s = shared(&lua)?;
        s.require_write("app.kv:delete")?;
        s.run_exec("delete from _kv where key = :k".into(), Params::named([("k", json!(key))])).await?;
        Ok(())
    }
}
