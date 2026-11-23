//! Arguments a function reads as Rust types.
//!
//! A method declared with `neoc_macros::lua_methods` takes its arguments as Rust values
//! (`name: String, opts: Option<Stop>`), and each is read from what the script passed by [`Argument`]. A table of
//! options is a struct marked `neoc_macros::lua_record(from)`: its fields are the keys, each read by [`Field`].
//!
//! A value of the wrong Lua type for a plain argument is raised, as mlua raises it for a closure's parameter.
//! A table of options that is wrong (an unknown key, a value of the wrong type or out of range) is [`Refused`]:
//! the function returns it as its own error (`(nil, "bollard.stop_container: invalid_input: ...")`), because a
//! script recovers from a bad option the way it recovers from any other failure of the call.
//!
//! A table is read raw (no `__index`, no `__pairs`): reading an argument never runs script code.

use std::collections::HashMap;

use mlua::{FromLua, Lua, Table, Value};

use super::{DeclType, Handle, List, Map};

/// Why an argument was refused, in words for the script. A module turns it into its own `invalid_input` error
/// (`impl From<Refused> for FsError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl Refused {
    /// A value of key `name` that is not what the key takes: `option "name" must be <what>`.
    #[must_use]
    pub fn option(name: &str, what: &str) -> Self {
        Self(format!("option \"{name}\" must be {what}"))
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One argument of a function, read from the value a script passed. `Err` outside is raised; `Err` inside is the
/// function's own error.
pub trait Argument: Sized + DeclType {
    /// Reads the argument.
    ///
    /// # Errors
    ///
    /// The conversion error mlua gives for a value of the wrong Lua type.
    fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>>;
}

macro_rules! plain_argument {
    ($($ty:ty),+ $(,)?) => {
        $(impl Argument for $ty {
            fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
                <$ty as FromLua>::from_lua(value, lua).map(Ok)
            }
        })+
    };
}

plain_argument!(
    String,
    bool,
    f64,
    f32,
    i64,
    i32,
    u64,
    u32,
    u16,
    usize,
    mlua::LuaString,
    Value,
    Table,
    mlua::Function,
    mlua::AnyUserData,
);

impl<T: DeclType> Argument for Handle<T> {
    fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
        Self::from_lua(value, lua).map(Ok)
    }
}

impl<T: Argument> Argument for Option<T> {
    fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
        if value.is_nil() {
            return Ok(Ok(None));
        }
        Ok(T::read(value, lua)?.map(Some))
    }
}

/// An argument a closure takes as it comes and reads itself: `opts: Checked<Option<Walk>>` never fails to
/// convert, and `opts.take()?` is the options or the function's own error.
pub struct Checked<T>(Result<T, Refused>);

impl<T> Checked<T> {
    /// The argument, or why it was refused.
    ///
    /// # Errors
    ///
    /// The refusal, as the error `E` of the function.
    pub fn take<E: From<Refused>>(self) -> Result<T, E> {
        self.0.map_err(E::from)
    }
}

impl<T: Argument> FromLua for Checked<T> {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        T::read(value, lua).map(Self)
    }
}

impl<T: DeclType> DeclType for Checked<T> {
    fn decl_type() -> String {
        T::decl_type()
    }
}

/// The value under one key of a table of options. Absent keys arrive as `Value::Nil`.
pub trait Field: Sized + DeclType {
    /// Reads the value of key `name`.
    ///
    /// # Errors
    ///
    /// Why the value is not one the key takes.
    fn field(name: &str, value: Value) -> Result<Self, Refused>;
}

impl<T: Field> Field for Option<T> {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        if value.is_nil() {
            return Ok(None);
        }
        T::field(name, value).map(Some)
    }
}

impl Field for bool {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        match value {
            Value::Boolean(on) => Ok(on),
            _ => Err(Refused::option(name, "a boolean")),
        }
    }
}

impl Field for String {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        match &value {
            Value::String(text) => text
                .to_str()
                .map(|text| text.to_owned())
                .map_err(|_| Refused::option(name, "a string of UTF-8 text")),
            _ => Err(Refused::option(name, "a string")),
        }
    }
}

impl Field for f64 {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        match value {
            Value::Integer(n) => Ok(n as f64),
            Value::Number(n) => Ok(n),
            _ => Err(Refused::option(name, "a number")),
        }
    }
}

/// A whole number, from a Lua integer or a float that holds one.
fn whole(value: &Value) -> Option<i64> {
    match *value {
        Value::Integer(n) => Some(n),
        // 2^63 is the first float past every i64
        Value::Number(n)
            if n.fract() == 0.0
                && n >= -9_223_372_036_854_775_808.0
                && n < 9_223_372_036_854_775_808.0 =>
        {
            Some(n as i64)
        }
        _ => None,
    }
}

macro_rules! whole_field {
    ($($ty:ty => $what:literal),+ $(,)?) => {
        $(impl Field for $ty {
            fn field(name: &str, value: Value) -> Result<Self, Refused> {
                whole(&value)
                    .and_then(|n| <$ty>::try_from(n).ok())
                    .ok_or_else(|| Refused::option(name, $what))
            }
        })+
    };
}

whole_field!(
    i64 => "a whole number",
    i32 => "a whole number",
    u32 => "a non-negative whole number",
    u16 => "a non-negative whole number",
);

/// A count: a non-negative whole number, and one past what a `u64` holds is "as far as it goes".
impl Field for u64 {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let refused = || Refused::option(name, "a non-negative whole number");
        match value {
            Value::Integer(n) => u64::try_from(n).map_err(|_| refused()),
            // `as` saturates
            Value::Number(n) if n.is_finite() && n >= 0.0 && n.fract() == 0.0 => Ok(n as u64),
            _ => Err(refused()),
        }
    }
}

impl Field for Value {
    fn field(_: &str, value: Value) -> Result<Self, Refused> {
        Ok(value)
    }
}

impl Field for mlua::LuaString {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        match value {
            Value::String(text) => Ok(text),
            _ => Err(Refused::option(name, "a string")),
        }
    }
}

impl<T: Field> Field for List<T> {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let what = || format!("a list of {}", T::decl_type());
        let Value::Table(list) = value else {
            return Err(Refused::option(name, &what()));
        };
        let mut items = Vec::with_capacity(list.raw_len());
        for index in 1..=list.raw_len() {
            let item = list
                .raw_get::<Value>(index)
                .map_err(|_| Refused::option(name, &what()))?;
            items.push(T::field(name, item).map_err(|_| Refused::option(name, &what()))?);
        }
        Ok(Self(items))
    }
}

impl<V: Field> Field for HashMap<String, V> {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let what = || format!("a table of {} under text", V::decl_type());
        let Value::Table(table) = value else {
            return Err(Refused::option(name, &what()));
        };
        table
            .pairs::<Value, Value>()
            .map(|pair| match pair {
                Ok((key @ Value::String(_), value)) => Ok((
                    String::field(name, key).map_err(|_| Refused::option(name, &what()))?,
                    V::field(name, value).map_err(|_| Refused::option(name, &what()))?,
                )),
                _ => Err(Refused::option(name, &what())),
            })
            .collect()
    }
}

impl<V: DeclType> DeclType for HashMap<String, V> {
    fn decl_type() -> String {
        Map::<String, V>::decl_type()
    }
}

/// The keys of a table of options, each checked against `allowed`: what `lua_record(from)` calls before it
/// reads the fields.
///
/// # Errors
///
/// A value that is not a table, a key that is not a string, a key that is not one of `allowed`.
pub fn options(value: Value, allowed: &[&str]) -> Result<Table, Refused> {
    let Value::Table(table) = value else {
        return Err(Refused(format!(
            "options must be a table, got {}",
            value.type_name()
        )));
    };
    for pair in table.pairs::<Value, Value>() {
        let Ok((key, _)) = pair else {
            return Err(Refused("the options could not be read".to_owned()));
        };
        let Value::String(key) = &key else {
            return Err(Refused("option names must be strings".to_owned()));
        };
        let key = key.to_string_lossy();
        if !allowed.contains(&&*key) {
            return Err(Refused(format!(
                "unknown option \"{key}\" (this function takes: {})",
                allowed.join(", ")
            )));
        }
    }
    Ok(table)
}

/// The value under `key` of a table [`options`] checked, read raw.
#[must_use]
pub fn value_of(table: &Table, key: &str) -> Value {
    table.raw_get(key).unwrap_or(Value::Nil)
}

/// The `(nil, err)` of a method whose argument was refused: the refusal as the method's own error `e`, under
/// the method's name.
///
/// # Errors
///
/// None: the error is returned to the script, not raised.
pub fn refused<T, E: std::fmt::Display>(
    op: &str,
    e: E,
) -> mlua::Result<crate::lua::sandbox::Fallible<T>> {
    crate::lua::sandbox::from_err(op, e)
}
