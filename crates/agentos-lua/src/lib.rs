//! Driven adapter: Lua 5.1 apps (mlua, on `agentos-luavm`) behind the `AppRuntime` port.
//!
//! One VM per app, with the host-provided `app` table (see [`api`]), a `require` for `@agentos/app`, `@agentos/ui`
//! and every `std:`/`lib:`/`vnd:` module of the VM library, tracked `app.state`, one call at a time per VM
//! (including the callbacks the app's own servers and tasks make), and errors reported with the app's file and
//! line. Nothing is budgeted and there are no permissions; what an app starts is stopped when it is unloaded.

mod api;
mod convert;
mod error;
pub mod reload;
mod require;
mod runtime;
mod state;
mod view;
mod vm;

pub use convert::{json_to_lua, lua_to_json};
pub use error::{ScriptError, ToolFailure};
pub use runtime::{LuaRuntime, UI_MODULE};
pub use state::DepKey;
pub use vm::{AppVm, Effect, Result, ShellEffect, VmConfig, VmError};
