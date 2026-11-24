//! neoc's Lua runtime on mlua-ir: each module is written once in plain Rust, and its types, docs and errors are
//! read from that Rust (`mlua_ir::schema()`), so the declarations cannot drift from the code.
//!
//! This crate is the runtime as a whole, and what an embedder depends on: `neoc-vm-core` is the engine, the
//! lifecycle and the scheduler, and the modules live in `neoc-vm-std` (`std:*`), `neoc-lib-*` (`lib:*`) and
//! `neoc-vnd-*` (`vnd:*`). This crate depends on them all, so that every module is linked (its `inventory`
//! registration is in the program) and offers their paths as they were: [`make_engine`], [`modules`], [`lifecycle`]
//! and the rest of the core, and [`declarations`], which writes the `.d.luau` / `.d.ts` files from the modules
//! linked.

pub mod declarations;
pub mod modules;

pub use neoc_vm_core::{
    Life, VM_THREAD_STACK, WorkerEngineFn, engine, host, install_runtime_state, lifecycle, load_source,
    make_bare_engine, make_engine, make_engine_in, panics, prepare_script, scheduler, script_env,
    set_worker_engine, worker_engine,
};

/// The `mlua` this crate is built on, re-exported so an embedder names the one binding the engine and the hooks in
/// [`lifecycle`] speak.
pub use neoc_vm_core::mlua;
/// mlua-ir, re-exported: the schema (`mlua_ir::schema`) is the API's description.
pub use neoc_vm_core::mlua_ir;

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
