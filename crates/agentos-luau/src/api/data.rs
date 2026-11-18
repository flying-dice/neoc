//! `app.db`: migrations, statements and live sources; `app.kv`: a small durable key-value store.
//!
//! ```lua
//! local db = app.db
//! db:migrate { [1] = "create table notes(...)" }
//! db:exec("insert into notes(body) values (:b)", { b = "hi" })     -- { changes, last_insert_id }
//! local rows = db:all("select * from notes where id > :n", { n = 3 })
//! local row  = db:one("select * from notes where id = :id", { id = 7 })
//! db:tx(function() db:exec(...); db:exec(...) end)                -- one commit, one consistent view
//!
//! -- sources: declared once, parameterised where they are used (a view)
//! local notes = db:query("select id, title from notes order by title, id", { key = "id", page = 60 })
//! local note  = db:row("select * from notes where id = :id", { key = "id", rev = "rev", write = fn })
//! local count = db:scalar("select count(*) from notes")
//! notes { folder = 1 }   -- a source handle
//! ```

use super::{check_effects_allowed, host, need};
use crate::{
    convert::{json_to_lua, lua_to_json, lua_to_params},
    vm::Shared,
};
use agentos_application::{
    data::{ExecOutcome, Params, SqlMigration},
    live::{Conflict, SourceDef, SourceId, SourceKind, TotalMode},
};
use mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::{json, Value};
use std::sync::Arc;

type Sh = Arc<Shared>;

impl Shared {
    fn data(&self, what: &str) -> mlua::Result<()> {
        need(self.caps.data, "data", what)
    }

    /// A read: through the open transaction when there is one.
    async fn run_query(&self, sql: String, params: Params) -> mlua::Result<Vec<Value>> {
        let open = self.tx.lock().take();
        match open {
            Some(mut t) => {
                let out = host(self, t.query(sql, params)).await;
                *self.tx.lock() = Some(t);
                out
            }
            None => host(self, self.db.query(sql, params)).await,
        }
    }

    /// A write: through the open transaction when there is one.
    async fn run_exec(&self, sql: String, params: Params) -> mlua::Result<ExecOutcome> {
        let open = self.tx.lock().take();
        match open {
            Some(mut t) => {
                let out = host(self, t.exec(sql, params)).await;
                *self.tx.lock() = Some(t);
                out
            }
            None => host(self, self.db.exec(sql, params)).await,
        }
    }
}

fn opt_string(opts: &Table, key: &str) -> mlua::Result<Option<String>> {
    match opts.get::<LuaValue>(key)? {
        LuaValue::Nil => Ok(None),
        LuaValue::String(s) => Ok(Some(s.to_string_lossy())),
        other => {
            Err(mlua::Error::runtime(format!("option `{key}` must be a string, got {}", other.type_name())))
        }
    }
}

fn reject_unknown(opts: &Table, allowed: &[&str], what: &str) -> mlua::Result<()> {
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

/// Declare a source: prepare it (so bad SQL fails at load, with the app's file and line), validate it
/// against what preparing found, and return the factory that makes handles.
async fn declare(
    sh: &Sh,
    lua: &Lua,
    sql: String,
    build: impl FnOnce(String) -> SourceKind,
) -> mlua::Result<Function> {
    let info = host(sh, sh.db.analyze(sql.clone())).await?;
    let id = {
        let sources = sh.sources.lock();
        format!("s{}", sources.len() + 1)
    };
    let def = SourceDef { id: SourceId(id.clone()), kind: build(id.clone()), sql, info };
    def.validate().map_err(|e| mlua::Error::runtime(format!("source {id}: {e}")))?;
    let params = def.info.named_params.clone();
    sh.sources.lock().push(def);

    lua.create_function(move |lua, given: Option<Table>| {
        let handle = lua.create_table()?;
        let values = lua.create_table()?;
        if let Some(given) = &given {
            for pair in given.pairs::<String, LuaValue>() {
                let (k, v) = pair?;
                if !params.contains(&k) {
                    return Err(mlua::Error::runtime(format!(
                        "source {id} has no param `{k}` (its params: {})",
                        if params.is_empty() { "none".to_owned() } else { params.join(", ") }
                    )));
                }
                values.set(k, v)?;
            }
        }
        for p in &params {
            if values.get::<LuaValue>(p.as_str())?.is_nil() {
                return Err(mlua::Error::runtime(format!("source {id} needs param `{p}`")));
            }
        }
        handle.set("source", id.as_str())?;
        handle.set("params", values)?;
        Ok(handle)
    })
}

pub(super) fn install(lua: &Lua, sh: &Sh, app: &Table) -> mlua::Result<()> {
    let db = lua.create_table()?;
    install_statements(lua, sh, &db)?;
    install_sources(lua, sh, &db)?;
    app.set("db", db)?;
    install_kv(lua, sh, app)
}

/// `db:migrate`, `db:exec`, `db:all`, `db:one`, `db:tx`.
#[allow(clippy::too_many_lines)] // a flat table of host functions: splitting it would only scatter them
fn install_statements(lua: &Lua, sh: &Sh, db: &Table) -> mlua::Result<()> {
    // db:migrate { [1] = sql, [2] = sql }
    let s = sh.clone();
    db.set(
        "migrate",
        lua.create_async_function(move |_, (_this, steps): (LuaValue, Table)| {
            let s = s.clone();
            async move {
                s.data("db:migrate")?;
                check_effects_allowed(&s, "db:migrate")?;
                let len = steps.raw_len();
                let mut migrations = Vec::with_capacity(len);
                for version in 1..=len {
                    let LuaValue::String(sql) = steps.raw_get::<LuaValue>(version)? else {
                        return Err(mlua::Error::runtime(format!("db:migrate: step [{version}] must be a SQL string")));
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
            }
        })?,
    )?;

    // db:exec(sql, params) -> { changes, last_insert_id }
    let s = sh.clone();
    db.set(
        "exec",
        lua.create_async_function(move |lua, (_this, sql, params): (LuaValue, String, Option<LuaValue>)| {
            let s = s.clone();
            async move {
                s.data("db:exec")?;
                check_effects_allowed(&s, "db:exec")?;
                let out = s.run_exec(sql, lua_to_params(params)?).await?;
                json_to_lua(&lua, &json!({"changes": out.changes, "last_insert_id": out.last_insert_id}))
            }
        })?,
    )?;

    // db:all(sql, params) -> rows ; db:one(sql, params) -> row | nil
    let s = sh.clone();
    db.set(
        "all",
        lua.create_async_function(move |lua, (_this, sql, params): (LuaValue, String, Option<LuaValue>)| {
            let s = s.clone();
            async move {
                s.data("db:all")?;
                let rows = s.run_query(sql, lua_to_params(params)?).await?;
                json_to_lua(&lua, &Value::Array(rows))
            }
        })?,
    )?;
    let s = sh.clone();
    db.set(
        "one",
        lua.create_async_function(move |lua, (_this, sql, params): (LuaValue, String, Option<LuaValue>)| {
            let s = s.clone();
            async move {
                s.data("db:one")?;
                let mut rows = s.run_query(sql, lua_to_params(params)?).await?;
                rows.truncate(1);
                match rows.pop() {
                    Some(row) => json_to_lua(&lua, &row),
                    None => Ok(LuaValue::Nil),
                }
            }
        })?,
    )?;

    // db:tx(fn): the statements inside commit together or not at all
    let s = sh.clone();
    db.set(
        "tx",
        lua.create_async_function(move |_, (_this, body): (LuaValue, Function)| {
            let s = s.clone();
            async move {
                s.data("db:tx")?;
                check_effects_allowed(&s, "db:tx")?;
                if s.tx.lock().is_some() {
                    return Err(mlua::Error::runtime("db:tx cannot be nested"));
                }
                let tx = host(&s, s.db.begin()).await?;
                *s.tx.lock() = Some(tx);
                let result = body.call_async::<mlua::MultiValue>(()).await;
                let tx = s.tx.lock().take();
                match (result, tx) {
                    (Ok(values), Some(tx)) => {
                        host(&s, tx.commit()).await?;
                        Ok(values)
                    }
                    (Err(e), Some(tx)) => {
                        if let Err(rollback) = host(&s, tx.rollback()).await {
                            tracing::warn!(app = %s.app, error = %rollback, "rolling back a failed db:tx failed");
                        }
                        Err(e)
                    }
                    (result, None) => result,
                }
            }
        })?,
    )?;

    Ok(())
}

/// `db:query`, `db:row`, `db:scalar`: live sources.
#[allow(clippy::too_many_lines)] // a flat table of host functions: splitting it would only scatter them
fn install_sources(lua: &Lua, sh: &Sh, db: &Table) -> mlua::Result<()> {
    // db:query(sql, { key, page, total }) -> factory of `rows` source handles
    let s = sh.clone();
    db.set(
        "query",
        lua.create_async_function(move |lua, (_this, sql, opts): (LuaValue, String, Table)| {
            let s = s.clone();
            async move {
                s.data("db:query")?;
                reject_unknown(&opts, &["key", "page", "total"], "db:query")?;
                let key = opt_string(&opts, "key")?
                    .ok_or_else(|| mlua::Error::runtime("db:query needs { key = \"<unique column>\" }"))?;
                let page = match opts.get::<LuaValue>("page")? {
                    LuaValue::Nil => None,
                    LuaValue::Integer(p) => {
                        Some(u32::try_from(p).map_err(|_| mlua::Error::runtime("page must be positive"))?)
                    }
                    LuaValue::Number(p) if p.fract() == 0.0 && p >= 0.0 =>
                    {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        Some(p as u32)
                    }
                    _ => return Err(mlua::Error::runtime("page must be a whole number")),
                };
                let total = match opt_string(&opts, "total")?.as_deref() {
                    None | Some("exact") => TotalMode::Exact,
                    Some("estimate") => TotalMode::Estimate,
                    Some("none") => TotalMode::None,
                    Some(other) => {
                        return Err(mlua::Error::runtime(format!(
                            "total must be exact, estimate or none, got `{other}`"
                        )))
                    }
                };
                declare(&s, &lua, sql, |_| SourceKind::Rows { key, page, total }).await
            }
        })?,
    )?;

    // db:row(sql, { key, rev, write, conflict, writable }) -> factory of `row` source handles
    let s = sh.clone();
    db.set(
        "row",
        lua.create_async_function(move |lua, (_this, sql, opts): (LuaValue, String, Table)| {
            let s = s.clone();
            async move {
                s.data("db:row")?;
                reject_unknown(&opts, &["key", "rev", "write", "conflict", "writable"], "db:row")?;
                let key = opt_string(&opts, "key")?
                    .ok_or_else(|| mlua::Error::runtime("db:row needs { key = \"<unique column>\" }"))?;
                let rev = opt_string(&opts, "rev")?;
                let conflict = match opt_string(&opts, "conflict")?.as_deref() {
                    None | Some("human") => Conflict::HumanWins,
                    Some("reject") => Conflict::Reject,
                    Some(other) => {
                        return Err(mlua::Error::runtime(format!(
                            "conflict must be \"human\" or \"reject\", got `{other}`"
                        )))
                    }
                };
                let writable = match opts.get::<LuaValue>("writable")? {
                    LuaValue::Nil => None,
                    v => match lua_to_json(&v)? {
                        Value::Array(a) => Some(
                            a.iter()
                                .map(|f| f.as_str().map(str::to_owned))
                                .collect::<Option<Vec<_>>>()
                                .ok_or_else(|| {
                                    mlua::Error::runtime("writable must be a list of field names")
                                })?,
                        ),
                        _ => return Err(mlua::Error::runtime("writable must be a list of field names")),
                    },
                };
                let write = match opts.get::<LuaValue>("write")? {
                    LuaValue::Nil => None,
                    LuaValue::Function(f) => Some(f),
                    _ => {
                        return Err(mlua::Error::runtime("write must be a function(key, field, value, ctx)"))
                    }
                };
                let has_write = write.is_some();
                let factory =
                    declare(&s, &lua, sql, |_| SourceKind::Row { key, rev, writable, has_write, conflict })
                        .await?;
                if let Some(write) = write {
                    let id = s.sources.lock().last().map(|d| d.id.0.clone()).unwrap_or_default();
                    s.writers.lock().insert(id, write);
                }
                Ok(factory)
            }
        })?,
    )?;

    // db:scalar(sql) -> factory of `value` source handles
    let s = sh.clone();
    db.set(
        "scalar",
        lua.create_async_function(move |lua, (_this, sql): (LuaValue, String)| {
            let s = s.clone();
            async move {
                s.data("db:scalar")?;
                declare(&s, &lua, sql, |_| SourceKind::Scalar).await
            }
        })?,
    )?;
    Ok(())
}

/// `app.kv:get(key)`, `:set(key, value)`, `:delete(key)`: JSON values in the app's `_kv` table.
fn install_kv(lua: &Lua, sh: &Sh, app: &Table) -> mlua::Result<()> {
    let kv = lua.create_table()?;

    let s = sh.clone();
    kv.set(
        "get",
        lua.create_async_function(move |lua, (_this, key): (LuaValue, String)| {
            let s = s.clone();
            async move {
                s.data("app.kv:get")?;
                let rows = s
                    .run_query(
                        "select value from _kv where key = :k".into(),
                        Params::named([("k", json!(key))]),
                    )
                    .await?;
                match rows.first().and_then(|r| r["value"].as_str()) {
                    Some(text) => {
                        let v: Value = serde_json::from_str(text).map_err(mlua::Error::external)?;
                        json_to_lua(&lua, &v)
                    }
                    None => Ok(LuaValue::Nil),
                }
            }
        })?,
    )?;

    let s = sh.clone();
    kv.set(
        "set",
        lua.create_async_function(move |_, (_this, key, value): (LuaValue, String, LuaValue)| {
            let s = s.clone();
            async move {
                s.data("app.kv:set")?;
                check_effects_allowed(&s, "app.kv:set")?;
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
            }
        })?,
    )?;

    let s = sh.clone();
    kv.set(
        "delete",
        lua.create_async_function(move |_, (_this, key): (LuaValue, String)| {
            let s = s.clone();
            async move {
                s.data("app.kv:delete")?;
                check_effects_allowed(&s, "app.kv:delete")?;
                s.run_exec("delete from _kv where key = :k".into(), Params::named([("k", json!(key))]))
                    .await?;
                Ok(())
            }
        })?,
    )?;
    app.set("kv", kv)
}
