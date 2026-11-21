//! Driven adapter: Lua 5.1 apps (mlua, on `neoc-vm`) behind the `AppRuntime` port.
//!
//! One VM per app, with the host-provided `app` table (see [`api`]), a `require` for `vos:app`, `vos:ui`
//! and every `std:`/`lib:`/`vnd:` module of the VM library except [`NOT_FOR_APPS`], tracked `app.state`, one call at a time per VM
//! (including the callbacks the app's own servers and tasks make), and errors reported with the app's file and
//! line. Nothing is budgeted and there are no permissions; what an app starts is stopped when it is unloaded.

mod api;
mod convert;
mod error;
pub mod reload;
mod require;
mod runtime;
mod state;
mod tx;
mod view;
mod vm;

pub use convert::{json_to_lua, lua_to_json};
pub use error::{ScriptError, ToolFailure};
/// The stack every thread that polls an app's VM needs (neoc-vm's constant, re-exported so that the host, which may not
/// depend on neoc-vm, can build its runtime with it).
pub use neoc_vm::VM_THREAD_STACK;
pub use require::NOT_FOR_APPS;
pub use runtime::{LuaRuntime, UI_MODULE};
pub use state::DepKey;
pub use vm::{AppVm, Effect, Result, ShellEffect, VmConfig, VmError};
