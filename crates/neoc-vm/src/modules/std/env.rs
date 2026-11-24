//! Environment, platform constants and well-known directories: `std:env` (mirrors `std::env`).
//!
//! Ported from the old VM's `std:env`. What changed on purpose: every failure is a typed error (`var` tells an unset
//! variable from one that is not Unicode, `current_dir` says why), and a malformed `_NEOC_ARGS` raises a typed
//! error.

#[mlua_ir::module("std:env")]
pub mod env {
    use std::collections::BTreeMap;

    use mlua::{Lua, Value};
    use mlua_ir::{LuaError, Raise};
    use serde::Serialize;

    /// Target operating system (`"linux"`, `"macos"`, `"windows"`, ...).
    #[allow(non_upper_case_globals)]
    pub const os: &str = std::env::consts::OS;
    /// Target CPU architecture (`"x86_64"`, `"aarch64"`, ...).
    #[allow(non_upper_case_globals)]
    pub const arch: &str = std::env::consts::ARCH;
    /// Target OS family (`"unix"` or `"windows"`).
    #[allow(non_upper_case_globals)]
    pub const family: &str = std::env::consts::FAMILY;
    /// Executable file suffix for the target (`".exe"` on Windows, empty elsewhere).
    #[allow(non_upper_case_globals)]
    pub const exe_suffix: &str = std::env::consts::EXE_SUFFIX;
    /// Dynamic-library file name prefix for the target (`"lib"` on Unix, empty on Windows).
    #[allow(non_upper_case_globals)]
    pub const dll_prefix: &str = std::env::consts::DLL_PREFIX;
    /// Dynamic-library file name suffix for the target (`".so"`, `".dylib"`, `".dll"`).
    #[allow(non_upper_case_globals)]
    pub const dll_suffix: &str = std::env::consts::DLL_SUFFIX;

    /// Why there is no home directory to name.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum HomeDirError {
        /// The platform does not say where the user's home is.
        #[error("home directory not found")]
        NotFound,
    }

    /// Why the current directory cannot be named.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CurrentDirError {
        /// The directory the process was in is gone.
        #[error("the current directory no longer exists")]
        NotFound,
        /// The process may not read its current directory.
        #[error("permission denied")]
        PermissionDenied,
        /// The operating system refused for another reason.
        #[error("{reason}")]
        Other {
            /// What the operating system said.
            reason: String,
        },
    }

    /// Why an environment variable has no value to give.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum VarError {
        /// The variable is not set.
        #[error("{name}: environment variable not found")]
        NotPresent {
            /// The variable asked for.
            name: String,
        },
        /// The variable is set, to something that is not Unicode text.
        #[error("{name}: environment variable was not valid unicode")]
        NotUnicode {
            /// The variable asked for.
            name: String,
        },
    }

    /// Why the script's arguments cannot be read.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ArgsError {
        /// The host set `_NEOC_ARGS` to something that is not a list of strings.
        #[error("_NEOC_ARGS: expected array of strings")]
        Malformed,
    }

    /// The current user's home directory.
    pub fn home_dir() -> Result<String, HomeDirError> {
        std::env::home_dir().map(|dir| dir.to_string_lossy().into_owned()).ok_or(HomeDirError::NotFound)
    }

    /// The platform's directory for temporary files.
    pub fn temp_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    /// The process's current working directory.
    pub fn current_dir() -> Result<String, CurrentDirError> {
        std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CurrentDirError::NotFound,
            std::io::ErrorKind::PermissionDenied => CurrentDirError::PermissionDenied,
            _ => CurrentDirError::Other { reason: error.to_string() },
        })
    }

    /// The value of the environment variable `key`.
    ///
    /// ```luau
    /// local value, err = env.var("_SURELY_NOT_SET_")
    /// assert(value == nil and err ~= nil and err.kind == "not_present" and err.name == "_SURELY_NOT_SET_")
    /// ```
    pub fn var(key: String) -> Result<String, VarError> {
        std::env::var(&key).map_err(|error| match error {
            std::env::VarError::NotPresent => VarError::NotPresent { name: key },
            std::env::VarError::NotUnicode(_) => VarError::NotUnicode { name: key },
        })
    }

    /// Every environment variable by name, leaving out those whose name or value is not Unicode text.
    pub fn vars() -> BTreeMap<String, String> {
        std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect()
    }

    /// The command-line arguments given to the script; none when the host passed none.
    pub fn args(lua: &Lua) -> Result<Vec<String>, Raise<ArgsError>> {
        match lua.globals().get::<Value>("_NEOC_ARGS") {
            // unset, nil or unreadable: no arguments
            Err(_) | Ok(Value::Nil) => Ok(Vec::new()),
            Ok(Value::Table(table)) => {
                table.sequence_values::<String>().collect::<mlua::Result<_>>().map_err(|_| Raise(ArgsError::Malformed))
            }
            Ok(_) => Err(Raise(ArgsError::Malformed)),
        }
    }
}

#[cfg(test)]
mod tests {
    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("std:env")"#).eval().unwrap();
        lua.globals().set("env", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn platform_constants_are_set() {
        let (os, arch, family, exe, prefix, suffix): (String, String, String, String, String, String) =
            eval("return env.os, env.arch, env.family, env.exe_suffix, env.dll_prefix, env.dll_suffix");
        assert_eq!(os, std::env::consts::OS);
        assert_eq!(arch, std::env::consts::ARCH);
        assert!(family == "unix" || family == "windows");
        assert_eq!((exe.as_str(), prefix.as_str()), if family == "windows" { (".exe", "") } else { ("", "lib") });
        assert!(!suffix.is_empty());
    }

    #[test]
    fn var_returns_existing() {
        unsafe { std::env::set_var("_NEOC_VM_TEST_VAR", "hello") };
        let (value, err): (String, mlua::Value) = eval(r#"return env.var("_NEOC_VM_TEST_VAR")"#);
        assert_eq!(value, "hello");
        assert!(err.is_nil());
        let listed: String = eval(r#"return env.vars()["_NEOC_VM_TEST_VAR"]"#);
        assert_eq!(listed, "hello");
        unsafe { std::env::remove_var("_NEOC_VM_TEST_VAR") };
    }

    #[test]
    fn a_missing_var_is_a_typed_error() {
        let (kind, name, text): (String, String, String) = eval(
            r#"
            local value, err = env.var("_NEOC_DEFINITELY_MISSING_VAR_XYZ")
            assert(value == nil and err.op == "std:env.var")
            return err.kind, err.name, tostring(err)
            "#,
        );
        assert_eq!((kind.as_str(), name.as_str()), ("not_present", "_NEOC_DEFINITELY_MISSING_VAR_XYZ"));
        assert_eq!(text, "std:env.var: not_present: _NEOC_DEFINITELY_MISSING_VAR_XYZ: environment variable not found");
        // a name no variable can have is unset, not a panic
        let odd: String = eval(r#"local _, err = env.var("a=b\0"); return err.kind"#);
        assert_eq!(odd, "not_present");
    }

    #[test]
    fn vars_are_strings_by_name() {
        let ok: bool = eval(
            r#"
            local count = 0
            for key, value in env.vars() do
                assert(type(key) == "string" and type(value) == "string")
                count += 1
            end
            return count > 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn directories_are_named() {
        let (home, temp, current): (String, String, String) =
            eval("return assert(env.home_dir()), env.temp_dir(), assert(env.current_dir())");
        assert!(!home.is_empty() && !temp.is_empty());
        assert_eq!(current, std::env::current_dir().unwrap().to_string_lossy());
    }

    #[test]
    fn args_come_from_the_host() {
        let (none, some, refused, not_strings): (Vec<String>, Vec<String>, String, String) = eval(
            r#"
            local none = env.args()
            _NEOC_ARGS = { "hello", "world" }
            local some = env.args()
            _NEOC_ARGS = 42
            local _, refused = pcall(env.args)
            _NEOC_ARGS = { "a", {} }
            local ok, not_strings = pcall(env.args)
            assert(not ok)
            return none, some, tostring(refused), tostring(not_strings)
            "#,
        );
        assert!(none.is_empty());
        assert_eq!(some, ["hello", "world"]);
        assert!(refused.contains("std:env.args: malformed: _NEOC_ARGS: expected array of strings"), "got: {refused}");
        assert!(not_strings.contains("std:env.args: malformed"), "got: {not_strings}");
    }
}
