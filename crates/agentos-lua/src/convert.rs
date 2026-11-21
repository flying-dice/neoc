//! Lua <-> JSON, written by hand (not serde) so the rules are the ones apps need:
//!
//! - whole numbers become JSON integers (`{ id = 7 }` binds as `7`, not `7.0`);
//! - JSON `null` and a Lua `nil` are the same thing (never the truthy `null` lightuserdata);
//! - a table with keys `1..n` is an array, a table with string keys is an object, an empty table is `[]`;
//! - functions, userdata, cycles and non-finite numbers are errors, with the path to the offender;
//! - the result is bounded: a [`Walk`] counts nodes and bytes as it goes, so a table that is shared at every
//!   level (`for i = 1, 30 do t = { t, t } end`, 2^30 paths to the leaves) or a huge string repeated is an
//!   error instead of gigabytes of JSON that no Lua table accounts for.

use agentos_application::data::Params;
use neoc_vm::mlua;
use neoc_vm::mlua::{Lua, Table, Value as LuaValue};
use serde_json::{Map, Number, Value};
use std::collections::HashSet;

const MAX_DEPTH: usize = 32;

/// Most JSON values (every scalar, array and object counts one) one conversion may produce. A tool result
/// or window argument of a hundred thousand values is already far past anything a person or an agent reads.
pub const MAX_NODES: usize = 100_000;

/// Most bytes of string content and object keys one conversion may produce: 4 MiB: a value
/// handed back to the host is far past anything a person or an agent reads before it gets here.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// What a conversion has used so far, and the tables it is inside of right now. One `Walk` can span many
/// `lua_to_json_in` calls (a whole view tree), so the budget is for the whole result.
pub struct Walk {
    nodes: usize,
    bytes: usize,
    max_nodes: usize,
    max_bytes: usize,
    /// The tables on the path from the root to here (by address): meeting one again is a cycle. A table met
    /// a second time off that path is shared, not circular, and is fine (it still costs its nodes again).
    inside: HashSet<usize>,
}

impl Walk {
    pub fn new() -> Self {
        Self::with_limits(MAX_NODES, MAX_BYTES)
    }
    pub fn with_limits(max_nodes: usize, max_bytes: usize) -> Self {
        Self { nodes: 0, bytes: 0, max_nodes, max_bytes, inside: HashSet::new() }
    }
    /// Count one value.
    pub fn node(&mut self, path: &str) -> mlua::Result<()> {
        self.nodes += 1;
        if self.nodes > self.max_nodes {
            return Err(bad(
                path,
                &format!("more than {} values (is a table shared at every level?)", self.max_nodes),
            ));
        }
        Ok(())
    }
    /// Count `len` bytes of string content or object keys.
    pub fn text(&mut self, path: &str, len: usize) -> mlua::Result<()> {
        self.bytes = self.bytes.saturating_add(len);
        if self.bytes > self.max_bytes {
            return Err(bad(path, &format!("more than {} bytes of text", self.max_bytes)));
        }
        Ok(())
    }
    /// Enter a table; `false` when it is already on the path (a cycle).
    pub fn enter(&mut self, t: &Table) -> bool {
        self.inside.insert(t.to_pointer().addr())
    }
    pub fn leave(&mut self, t: &Table) {
        self.inside.remove(&t.to_pointer().addr());
    }
}

impl Default for Walk {
    fn default() -> Self {
        Self::new()
    }
}

fn bad(path: &str, why: &str) -> mlua::Error {
    mlua::Error::runtime(format!("cannot convert {path} to JSON: {why}"))
}

/// A whole `f64` that survives a round trip through `i64`.
fn as_integer(n: f64) -> Option<i64> {
    // 2^53: beyond it a double no longer represents every integer
    const LIMIT: f64 = 9_007_199_254_740_992.0;
    #[allow(clippy::cast_possible_truncation)]
    (n.fract() == 0.0 && n.abs() < LIMIT).then_some(n as i64)
}

pub fn lua_to_json(v: &LuaValue) -> mlua::Result<Value> {
    lua_to_json_in(v, "value", &mut Walk::new())
}

/// Like [`lua_to_json`], inside a budget that several conversions share, naming the value `path` in errors.
pub fn lua_to_json_in(v: &LuaValue, path: &str, walk: &mut Walk) -> mlua::Result<Value> {
    to_json(v, path, 0, walk)
}

fn to_json(v: &LuaValue, path: &str, depth: usize, walk: &mut Walk) -> mlua::Result<Value> {
    if depth > MAX_DEPTH {
        return Err(bad(path, "nested too deeply"));
    }
    walk.node(path)?;
    if let LuaValue::String(s) = v {
        walk.text(path, s.as_bytes().len())?;
    }
    Ok(match v {
        LuaValue::Nil => Value::Null,
        LuaValue::Boolean(b) => Value::Bool(*b),
        LuaValue::Integer(i) => Value::from(*i),
        LuaValue::Number(n) => match as_integer(*n) {
            Some(i) => Value::from(i),
            None => Value::Number(Number::from_f64(*n).ok_or_else(|| bad(path, "a non-finite number"))?),
        },
        LuaValue::String(s) => Value::String(s.to_string_lossy()),
        LuaValue::Table(t) => {
            if !walk.enter(t) {
                return Err(bad(path, "a table that contains itself"));
            }
            let json = table_to_json(t, path, depth, walk);
            walk.leave(t);
            json?
        }
        other => return Err(bad(path, &format!("a {} cannot be stored or sent", other.type_name()))),
    })
}

/// A table key as an array position (1-based), if it is one.
fn array_index(k: &LuaValue) -> Option<usize> {
    let i = match k {
        LuaValue::Integer(i) => *i,
        LuaValue::Number(n) => as_integer(*n)?,
        _ => return None,
    };
    usize::try_from(i).ok().filter(|i| *i >= 1)
}

fn table_to_json(t: &Table, path: &str, depth: usize, walk: &mut Walk) -> mlua::Result<Value> {
    let mut array: Vec<(usize, Value)> = Vec::new();
    let mut object = Map::new();
    for pair in t.pairs::<LuaValue, LuaValue>() {
        let (k, v) = pair?;
        match k {
            LuaValue::String(s) => {
                let key = s.to_string_lossy();
                walk.text(path, key.len())?;
                let child = to_json(&v, &format!("{path}.{key}"), depth + 1, walk)?;
                object.insert(key, child);
            }
            LuaValue::Integer(_) | LuaValue::Number(_) => {
                let idx =
                    array_index(&k).ok_or_else(|| bad(path, "array keys must be whole numbers from 1"))?;
                array.push((idx, to_json(&v, &format!("{path}[{idx}]"), depth + 1, walk)?));
            }
            other => return Err(bad(path, &format!("a table key of type {}", other.type_name()))),
        }
    }
    match (array.is_empty(), object.is_empty()) {
        (true, true) => Ok(Value::Array(vec![])),
        (false, true) => {
            array.sort_by_key(|(i, _)| *i);
            if array.iter().enumerate().any(|(n, (i, _))| *i != n + 1) {
                return Err(bad(path, "an array with holes (use false or a placeholder instead of nil)"));
            }
            Ok(Value::Array(array.into_iter().map(|(_, v)| v).collect()))
        }
        (true, false) => Ok(Value::Object(object)),
        (false, false) => Err(bad(path, "a table that mixes array entries and named keys")),
    }
}

pub fn json_to_lua(lua: &Lua, v: &Value) -> mlua::Result<LuaValue> {
    Ok(match v {
        Value::Null => LuaValue::Nil,
        Value::Bool(b) => LuaValue::Boolean(*b),
        Value::Number(n) => LuaValue::Number(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => LuaValue::String(lua.create_string(s)?),
        Value::Array(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, json_to_lua(lua, item)?)?;
            }
            LuaValue::Table(t)
        }
        Value::Object(map) => {
            let t = lua.create_table_with_capacity(0, map.len())?;
            for (k, item) in map {
                t.raw_set(k.as_str(), json_to_lua(lua, item)?)?;
            }
            LuaValue::Table(t)
        }
    })
}

/// SQL parameters from Lua: `nil` (none), `{ a, b }` (positional) or `{ name = v }` (named binds).
pub fn lua_to_params(v: Option<LuaValue>) -> mlua::Result<Params> {
    let Some(v) = v else { return Ok(Params::None) };
    match lua_to_json(&v)? {
        Value::Null => Ok(Params::None),
        Value::Array(items) if items.is_empty() => Ok(Params::None),
        Value::Array(items) => Ok(Params::Positional(items)),
        Value::Object(map) => Ok(Params::Named(map.into_iter().collect())),
        other => Err(mlua::Error::runtime(format!("SQL params must be a table, got {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eval(lua: &Lua, src: &str) -> LuaValue {
        lua.load(src).eval().unwrap()
    }

    #[test]
    fn whole_numbers_are_integers_and_fractions_stay_floats() {
        let lua = Lua::new();
        let v = lua_to_json(&eval(&lua, "return { id = 7, ratio = 0.5, big = 2^40, neg = -3 }")).unwrap();
        assert_eq!(v, json!({"id": 7, "ratio": 0.5, "big": 1_099_511_627_776_i64, "neg": -3}));
        assert!(v["id"].is_i64(), "7, not 7.0");
    }

    #[test]
    fn arrays_objects_and_the_empty_table() {
        let lua = Lua::new();
        assert_eq!(
            lua_to_json(&eval(&lua, "return {1, 'a', true, {2}}")).unwrap(),
            json!([1, "a", true, [2]])
        );
        assert_eq!(lua_to_json(&eval(&lua, "return {}")).unwrap(), json!([]));
        assert_eq!(lua_to_json(&eval(&lua, "return nil")).unwrap(), Value::Null);
        assert_eq!(lua_to_json(&eval(&lua, "return { a = { b = 'c' } }")).unwrap(), json!({"a": {"b": "c"}}));
    }

    #[test]
    fn what_cannot_be_json_is_an_error_naming_where() {
        let lua = Lua::new();
        let e = |src: &str| lua_to_json(&eval(&lua, src)).unwrap_err().to_string();
        assert!(e("return { f = function() end }").contains("value.f"), "functions");
        assert!(
            e("return { 1, nil, 3 }").contains("holes") || e("return { [1] = 1, [3] = 3 }").contains("holes")
        );
        assert!(e("return { 1, a = 2 }").contains("mixes"));
        assert!(e("return { n = 0/0 }").contains("non-finite"));
        assert!(e("local t = {} t.self = t return t").contains("contains itself"), "a cycle");
        assert!(e("local t = 1 for i = 1, 40 do t = { t } end return t").contains("too deeply"), "depth");
        assert!(e("return { [true] = 1 }").contains("table key"));
    }

    #[test]
    fn a_table_shared_at_every_level_is_cut_off_not_expanded() {
        let lua = Lua::new();
        let e = |src: &str| lua_to_json(&eval(&lua, src)).unwrap_err().to_string();
        // 2^30 paths to the leaf from 31 tables
        assert!(e("local t = { 1 } for i = 1, 30 do t = { t, t } end return t")
            .contains("more than 100000 values"));
        // a megabyte shared forty times
        assert!(e("local s = string.rep('x', 1000000) local t = {} for i = 1, 40 do t[i] = s end return t")
            .contains("bytes of text"));
        // sharing that is small is fine, and counted each time it is met
        let v = lua_to_json(&eval(&lua, "local a = { 1, 2 } return { a, a, a }")).unwrap();
        assert_eq!(v, json!([[1, 2], [1, 2], [1, 2]]));
    }

    #[test]
    fn a_walk_shared_by_several_conversions_spends_one_budget() {
        let lua = Lua::new();
        let mut walk = Walk::with_limits(5, 1024);
        let two = eval(&lua, "return { 1, 2 }"); // three values
        assert!(lua_to_json_in(&two, "a", &mut walk).is_ok());
        assert!(lua_to_json_in(&two, "b", &mut walk).unwrap_err().to_string().contains("more than 5 values"));
    }

    #[test]
    fn json_round_trips_and_null_is_nil() {
        let lua = Lua::new();
        let v = json!({"id": 7, "tags": ["a", "b"], "gone": null, "nested": {"ok": true}});
        let l = json_to_lua(&lua, &v).unwrap();
        lua.globals().set("v", l).unwrap();
        let ok: bool = lua
            .load("return v.id == 7 and v.tags[2] == 'b' and v.gone == nil and v.nested.ok == true")
            .eval()
            .unwrap();
        assert!(ok);
        let back = lua_to_json(&lua.globals().get::<LuaValue>("v").unwrap()).unwrap();
        assert_eq!(back, json!({"id": 7, "tags": ["a", "b"], "nested": {"ok": true}}));
    }

    #[test]
    fn sql_params_are_none_positional_or_named() {
        let lua = Lua::new();
        assert_eq!(lua_to_params(None).unwrap(), Params::None);
        assert_eq!(lua_to_params(Some(eval(&lua, "return {}"))).unwrap(), Params::None);
        assert_eq!(
            lua_to_params(Some(eval(&lua, "return {1, 'x'}"))).unwrap(),
            Params::Positional(vec![json!(1), json!("x")])
        );
        let Params::Named(n) = lua_to_params(Some(eval(&lua, "return { id = 7 }"))).unwrap() else {
            panic!("named")
        };
        assert_eq!(n, vec![("id".to_owned(), json!(7))]);
        assert!(lua_to_params(Some(eval(&lua, "return 5"))).is_err());
    }
}
