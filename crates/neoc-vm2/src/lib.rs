//! neoc's Lua runtime on mlua-ir: each module is written once in plain Rust, and its types, docs and errors are
//! read from that Rust (`mlua_ir::schema()`), so the declarations cannot drift from the code.
//!
//! - [`make_engine`] gives a ready VM; [`modules`] is the `require` registry; [`lifecycle`] is what a script starts
//!   and who may run its code; [`host`] makes every module call a traced, lifecycle-aware one.
//! - [`scheduler`] runs a script's tasks (`std:task`); [`script_env`] is the environment an embedder runs an
//!   untrusted script in; [`panics`] ends a host function's panic as an error of the call that reached it.
//! - [`declarations`] writes the `.d.luau` / `.d.ts` files from the modules this crate links.
//!
//! neoc-vm is the reference this crate is ported from, module by module.

mod byte_stream;
mod child_exit;
mod command_options;
pub mod declarations;
pub mod engine;
pub mod host;
pub mod lifecycle;
mod limits;
pub mod modules;
pub mod panics;
pub mod scheduler;
pub mod script_env;
mod span;

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

/// End the program with `code`, as `std::process::exit` does, after killing the processes its scripts started
/// (`std:process`): an exit runs no destructor and no task, so they would live on. A program that embeds the VM ends
/// with this wherever it would call `std::process::exit` (clippy refuses that one in this workspace).
pub fn exit(code: i32) -> ! {
    kill_processes();
    #[allow(clippy::disallowed_methods)] // the one place
    std::process::exit(code)
}

/// Kill every process a script of this program started (`std:process`) and that still runs, at once. For the end of
/// a program whose engines may outlive `main` (a worker that still runs); [`exit`] does it too.
pub fn kill_processes() {
    modules::std::process::kill_all();
}
