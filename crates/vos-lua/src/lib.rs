//! Driven adapter: Luau apps (mlua, on `neoc-vm2`) behind the `AppRuntime` port.
//!
//! One VM per app, with the host-provided `app` table (see [`api`]), a `require` for `vos:app`
//! and every `std:`/`lib:`/`vnd:` module of the VM library except [`NOT_FOR_APPS`], one call at a time per VM
//! (including the callbacks the app's own servers and tasks make), and errors reported with the app's file and
//! line. Nothing is budgeted and there are no permissions; what an app starts is stopped when it is unloaded.
//!
//! It also implements the `FileStore` port ([`DiskFiles`]: each app's `app.files` on disk) and the `HostFiles` port
//! ([`LocalFiles`]: the machine's own file system, for the Files page), next to it: the adapter of the host's disk, because the hardened
//! filesystem operations it is built on are `neoc-vm2`'s (`std:fs`), which only this crate may use.

mod api;
mod convert;
mod error;
mod files;
mod host_files;
pub mod reload;
mod require;
mod runtime;
mod tx;
mod vm;

pub use convert::{json_to_lua, lua_to_json};
pub use error::{ScriptError, ToolFailure};
pub use files::DiskFiles;
pub use host_files::LocalFiles;
/// The stack every thread that polls an app's VM needs (neoc-vm2's constant, re-exported so that the host, which may
/// not depend on neoc-vm2, can build its runtime with it).
pub use neoc_vm2::VM_THREAD_STACK;
pub use require::NOT_FOR_APPS;
pub use runtime::LuaRuntime;
pub use vm::{AppVm, Effect, Result, VmConfig, VmError, MAX_QUEUED_NOTIFICATIONS};
