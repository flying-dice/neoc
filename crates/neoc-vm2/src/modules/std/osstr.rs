//! Paths between Lua strings and the operating system: what `std:path` and `std:fs` take and return.
//!
//! A Lua string is bytes. On Unix a path is bytes too, so every path round-trips. Elsewhere a path is text (UTF-16
//! on Windows), so a Lua string that is not valid UTF-8 is not a path and is refused as an argument, and a path that
//! is not valid Unicode is given to a script with replacement characters.
//!
//! Ported from the old VM's `std/osstr.rs`: the two conversions became [`OsPath`]'s argument and return conversions.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use mlua::{Lua, Value};
use mlua_ir::ir::Ty;
use mlua_ir::{FromLuaArg, IntoLuaRet, Registry, Shape, expected};

/// A path, or a part of one: a string to a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsPath(pub PathBuf);

impl OsPath {
    /// The path of an operating-system string.
    pub fn of(text: impl AsRef<OsStr>) -> Self {
        OsPath(PathBuf::from(text.as_ref()))
    }
}

impl AsRef<Path> for OsPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// As a field of a record a script is given: the same string [`IntoLuaRet`] gives.
///
/// On Unix a path that is UTF-8 is written as text and only one that is not as its raw bytes. Both are the same
/// Lua string to mlua's serializer, but a typed error's fields go through JSON, where bytes would be an array of
/// numbers and the error's `path` a table instead of a string.
impl serde::Serialize for OsPath {
    #[cfg(unix)]
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use std::os::unix::ffi::OsStrExt;
        match self.0.to_str() {
            Some(text) => serializer.serialize_str(text),
            None => serializer.serialize_bytes(self.0.as_os_str().as_bytes()),
        }
    }

    #[cfg(not(unix))]
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string_lossy())
    }
}

/// As a field of a record a script gives: the same string [`FromLuaArg`] takes.
impl<'de> serde::Deserialize<'de> for OsPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PathVisitor;

        impl serde::de::Visitor<'_> for PathVisitor {
            type Value = OsPath;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<OsPath, E> {
                Ok(OsPath(PathBuf::from(text)))
            }

            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<OsPath, E> {
                from_bytes(bytes).map(OsPath).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PathVisitor)
    }
}

impl Shape for OsPath {
    fn shape(_: &mut Registry) -> Ty {
        Ty::String
    }
}

/// A script's bytes as a path; never refused on Unix.
#[cfg(unix)]
fn from_bytes(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn from_bytes(bytes: &[u8]) -> Result<PathBuf, String> {
    std::str::from_utf8(bytes).map(PathBuf::from).map_err(|_| "a path must be valid UTF-8 on this platform".to_owned())
}

impl FromLuaArg for OsPath {
    fn from_lua_arg(value: Value, _: &Lua) -> Result<Self, String> {
        match value {
            Value::String(text) => from_bytes(&text.as_bytes()).map(OsPath),
            other => Err(expected("a string", &other)),
        }
    }
}

impl IntoLuaRet for OsPath {
    #[cfg(unix)]
    fn into_lua_ret(self, lua: &Lua, _: &'static str) -> mlua::Result<Value> {
        use std::os::unix::ffi::OsStrExt;
        lua.create_string(self.0.as_os_str().as_bytes()).map(Value::String)
    }

    #[cfg(not(unix))]
    fn into_lua_ret(self, lua: &Lua, _: &'static str) -> mlua::Result<Value> {
        lua.create_string(self.0.to_string_lossy().as_bytes()).map(Value::String)
    }
}
