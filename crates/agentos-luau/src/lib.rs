//! Driven adapter: sandboxed Luau (mlua) behind the `AppRuntime` port.
//!
//! One sandboxed Luau VM per app, with the host-provided `app` capability table (see [`api`]), a bundle-only
//! `require`, a memory limit, a CPU budget counted in VM interrupts (deterministic, with a wall-clock
//! backstop), a wall-clock timeout on host calls, tracked `app.state`, and errors reported with the app's file and line.

mod api;
mod budget;
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
pub use runtime::LuauRuntime;
pub use state::DepKey;
pub use vm::{AppVm, Effect, Limits, Result, ShellEffect, VmConfig, VmError};
