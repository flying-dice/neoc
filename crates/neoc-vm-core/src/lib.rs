//! The core of neoc's Lua runtime on mlua-ir: the engine, the lifecycle of what a script starts, the scheduler, the
//! host that makes every module call a traced, lifecycle-aware one, and the helpers the modules share. It declares no
//! module: `neoc-vm-std`, `neoc-vm-lib-*` and `neoc-vm-vnd-*` do, and `neoc-vm` links them all.
//!
//! - [`make_engine`] gives a ready VM; [`modules`] is the `require` registry; [`lifecycle`] is what a script starts
//!   and who may run its code; [`host`] makes every module call a traced, lifecycle-aware one.
//! - [`scheduler`] runs a script's tasks (`std:task`); [`script_env`] is the environment an embedder runs an
//!   untrusted script in; [`panics`] ends a host function's panic as an error of the call that reached it.
//! - [`osstr`], [`byte_stream`], [`child_exit`], [`command_options`], [`log_sink`] and [`random`] are what more than
//!   one module needs.

pub mod byte_stream;
pub mod child_exit;
pub mod command_options;
pub mod engine;
pub mod host;
pub mod lifecycle;
pub mod limits;
pub mod log_sink;
pub mod modules;
pub mod osstr;
pub mod panics;
pub mod random;
pub mod scheduler;
pub mod script_env;
pub mod span;

pub use engine::{
    WorkerEngineFn, install_runtime_state, load_source, make_bare_engine, make_engine, make_engine_in,
    prepare_script, set_worker_engine, worker_engine,
};
pub use lifecycle::{Life, VM_THREAD_STACK};

/// The `mlua` this crate is built on, re-exported so an embedder names the one binding the engine and the hooks in
/// [`lifecycle`] speak.
pub use mlua;
/// mlua-ir, re-exported: the schema (`mlua_ir::schema`) is the API's description.
pub use mlua_ir;
