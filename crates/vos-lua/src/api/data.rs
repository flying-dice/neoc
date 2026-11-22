//! `app.db`: migrations, statements and transactions; `app.kv`: a small durable key-value store.
//!
//! ```lua
//! local db = app.db
//! db:migrate { [1] = "create table notes(...)" }
//! db:exec("insert into notes(body) values (:b)", { b = "hi" })     -- { changes, last_insert_id }
//! local rows = db:all("select * from notes where id > :n", { n = 3 })
//! local row  = db:one("select * from notes where id = :id", { id = 7 })
//! db:tx(function() db:exec(...); db:exec(...) end)                -- one commit
//! ```

use super::{host, vm_function};
use crate::{
    convert::{json_to_lua, lua_to_json, lua_to_params},
    tx::{answer, OpenTx, Reply},
    vm::Shared,
};
use neoc_vm::lua::lifecycle::turn_id;
use neoc_vm::mlua;
use neoc_vm::mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::{json, Value};
use vos_application::data::{ExecOutcome, Params, SqlMigration};

use super::Sh;

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

pub(super) fn reject_unknown(opts: &Table, allowed: &[&str], what: &str) -> mlua::Result<()> {
    for pair in opts.pairs::<String, LuaValue>() {
        let (k, _) = pair?;
        if !allowed.contains(&k.as_str()) {
            return Err(mlua::Error::runtime(format!(
                "{what}: unknown option `{k}` (known: {})",
                allowed.join(", ")
            )));
        }
    }
    Ok(())
}

pub(super) fn install(lua: &Lua, sh: &Sh, app: &Table) -> mlua::Result<()> {
    let db = lua.create_table()?;
    install_statements(lua, sh, &db)?;
    app.set("db", db)?;
    install_kv(lua, sh, app)
}

/// `db:migrate`, `db:exec`, `db:all`, `db:one`, `db:tx`.
#[allow(clippy::too_many_lines)] // a flat table of host functions: splitting it would only scatter them
fn install_statements(lua: &Lua, sh: &Sh, db: &Table) -> mlua::Result<()> {
    // db:migrate { [1] = sql, [2] = sql }
    db.set(
        "migrate",
        vm_function(lua, sh, |s, _, (_this, steps): (LuaValue, Table)| async move {
            s.require_write("db:migrate")?;
            if s.tx.lock().is_some() {
                // the transaction holds the database's write gate, and a migration waits for that same gate
                return Err(mlua::Error::runtime(
                    "db:migrate cannot run inside db:tx: migrate before the transaction starts",
                ));
            }
            let len = steps.raw_len();
            // not presized: `#steps` is whatever border Lua finds, so a forged table can report 2^30
            let mut migrations = Vec::new();
            for version in 1..=len {
                let LuaValue::String(sql) = steps.raw_get::<LuaValue>(version)? else {
                    return Err(mlua::Error::runtime(format!(
                        "db:migrate: step [{version}] must be a SQL string"
                    )));
                };
                let sql = sql.to_string_lossy();
                migrations.push(SqlMigration {
                    version: u32::try_from(version).map_err(mlua::Error::external)?,
                    name: format!("migration {version}"),
                    sql,
                });
            }
            let keys = steps.pairs::<LuaValue, LuaValue>().count();
            if keys != len {
                return Err(mlua::Error::runtime(
                    "db:migrate takes steps numbered 1..N without gaps: db:migrate { [1] = ..., [2] = ... }",
                ));
            }
            host(&s, s.db.migrate(migrations)).await
        })?,
    )?;

    // db:exec(sql, params) -> { changes, last_insert_id }
    db.set(
        "exec",
        vm_function(
            lua,
            sh,
            |s, lua, (_this, sql, params): (LuaValue, String, Option<LuaValue>)| async move {
                s.require_write("db:exec")?;
                let out = s.run_exec(sql, lua_to_params(params)?).await?;
                json_to_lua(&lua, &json!({"changes": out.changes, "last_insert_id": out.last_insert_id}))
            },
        )?,
    )?;

    // db:all(sql, params) -> rows ; db:one(sql, params) -> row | nil
    db.set(
        "all",
        vm_function(
            lua,
            sh,
            |s, lua, (_this, sql, params): (LuaValue, String, Option<LuaValue>)| async move {
                let rows = s.run_query(sql, lua_to_params(params)?).await?;
                json_to_lua(&lua, &Value::Array(rows))
            },
        )?,
    )?;
    db.set(
        "one",
        vm_function(
            lua,
            sh,
            |s, lua, (_this, sql, params): (LuaValue, String, Option<LuaValue>)| async move {
                let mut rows = s.run_query(sql, lua_to_params(params)?).await?;
                rows.truncate(1);
                match rows.pop() {
                    Some(row) => json_to_lua(&lua, &row),
                    None => Ok(LuaValue::Nil),
                }
            },
        )?,
    )?;

    // db:tx(fn): the statements inside commit together or not at all
    db.set(
        "tx",
        vm_function(lua, sh, |s, _, (_this, body): (LuaValue, Function)| async move {
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
            let result = body.call_async::<mlua::MultiValue>(()).await;
            // whatever the body left in flight is queued ahead of the end, in the transaction
            match (result, guard.release()) {
                (Ok(values), Some(tx)) => {
                    await_reply(&s, tx.end(true)).await?;
                    Ok(values)
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
        })?,
    )?;

    Ok(())
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

/// `app.kv:get(key)`, `:set(key, value)`, `:delete(key)`: JSON values in the app's `_kv` table.
fn install_kv(lua: &Lua, sh: &Sh, app: &Table) -> mlua::Result<()> {
    let kv = lua.create_table()?;

    kv.set(
        "get",
        vm_function(lua, sh, |s, lua, (_this, key): (LuaValue, String)| async move {
            let rows = s
                .run_query("select value from _kv where key = :k".into(), Params::named([("k", json!(key))]))
                .await?;
            match rows.first().and_then(|r| r["value"].as_str()) {
                Some(text) => {
                    let v: Value = serde_json::from_str(text).map_err(mlua::Error::external)?;
                    json_to_lua(&lua, &v)
                }
                None => Ok(LuaValue::Nil),
            }
        })?,
    )?;

    kv.set(
        "set",
        vm_function(lua, sh, |s, _, (_this, key, value): (LuaValue, String, LuaValue)| async move {
            s.require_write("app.kv:set")?;
            let json = lua_to_json(&value)?;
            if json.is_null() {
                s.run_exec("delete from _kv where key = :k".into(), Params::named([("k", json!(key))])).await?;
            } else {
                s.run_exec(
                    "insert into _kv(key, value) values(:k, :v) on conflict(key) do update set value = excluded.value".into(),
                    Params::named([("k", json!(key)), ("v", json!(json.to_string()))]),
                )
                .await?;
            }
            Ok(())
        })?,
    )?;

    kv.set(
        "delete",
        vm_function(lua, sh, |s, _, (_this, key): (LuaValue, String)| async move {
            s.require_write("app.kv:delete")?;
            s.run_exec("delete from _kv where key = :k".into(), Params::named([("k", json!(key))])).await?;
            Ok(())
        })?,
    )?;
    app.set("kv", kv)
}
