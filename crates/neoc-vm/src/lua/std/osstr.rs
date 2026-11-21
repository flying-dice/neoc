//! Paths between Lua strings and the operating system: shared by `std:fs` and `std:path`.
//!
//! A Lua string is bytes. On Unix a path is bytes too, so every path round-trips: a name that `fs.read_dir`
//! returns can always be handed back. Elsewhere a path is text (UTF-16 on Windows), so a Lua string that is not
//! valid UTF-8 is not a path, and a path that is not valid Unicode is converted with replacement characters.

use std::ffi::OsStr;
use std::path::PathBuf;

use mlua::{Lua, LuaString};

/// A script's string as a path, or `None` when this platform cannot represent it (never on Unix).
#[cfg(unix)]
pub(crate) fn to_path(s: &LuaString) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(OsStr::from_bytes(&s.as_bytes())))
}

#[cfg(not(unix))]
pub(crate) fn to_path(s: &LuaString) -> Option<PathBuf> {
    s.to_str().ok().map(|text| PathBuf::from(&*text))
}

/// An operating-system string as a Lua string.
#[cfg(unix)]
pub(crate) fn to_lua(lua: &Lua, s: &OsStr) -> mlua::Result<LuaString> {
    use std::os::unix::ffi::OsStrExt;
    lua.create_string(s.as_bytes())
}

#[cfg(not(unix))]
pub(crate) fn to_lua(lua: &Lua, s: &OsStr) -> mlua::Result<LuaString> {
    lua.create_string(s.to_string_lossy().as_bytes())
}
