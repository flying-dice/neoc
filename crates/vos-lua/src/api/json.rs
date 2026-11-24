//! [`Json`]: plain data that crosses with the app API's own rules ([`crate::convert`]), declared as a Rust type.
//!
//! mlua-ir's own conversion goes through serde, which is not what an app's data needs: a whole number must bind as
//! `7` (not `7.0`), an empty table is `[]`, and a conversion is bounded. So the values an app hands the host (SQL
//! parameters, a stored value, an event's data) and the ones it gets back (rows) cross as [`Json`]: the declared
//! type is `T`'s, the conversion is [`lua_to_json`] and [`json_to_lua`].

use crate::convert::{json_to_lua, lua_to_json};
use mlua_ir::{FromLuaArg, IntoLuaRet, Registry, Shape};
use neoc_vm2::mlua::{self, Lua, Value as LuaValue};
use serde_json::Value;
use std::marker::PhantomData;

/// JSON data declared as `T`.
pub struct Json<T> {
    pub value: Value,
    _declared: PhantomData<fn() -> T>,
}

impl<T> Json<T> {
    pub fn new(value: Value) -> Self {
        Self { value, _declared: PhantomData }
    }
}

impl<T: Shape> Shape for Json<T> {
    fn shape(reg: &mut Registry) -> mlua_ir::ir::Ty {
        T::shape(reg)
    }
}

impl<T> FromLuaArg for Json<T> {
    fn from_lua_arg(value: LuaValue, _: &Lua) -> Result<Self, String> {
        lua_to_json(&value).map(Self::new).map_err(|e| e.to_string())
    }
}

impl<T> IntoLuaRet for Json<T> {
    fn into_lua_ret(self, lua: &Lua, _: &'static str) -> mlua::Result<LuaValue> {
        json_to_lua(lua, &self.value)
    }
}
