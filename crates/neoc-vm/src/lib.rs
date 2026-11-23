// The library has no `unsafe`; the one test that edits the process environment needs it
// (`std::env::set_var` is unsafe in edition 2024).
#![cfg_attr(not(test), forbid(unsafe_code))]

pub mod errors;
pub mod lua;

/// The `mlua` this crate is built on, re-exported so a program that embeds the VM names the one binding (and
/// version) the engine, the modules and the hooks in [`lua::lifecycle`] speak.
pub use mlua;

/// The stack every thread that polls a VM needs; see [`lua::lifecycle::VM_THREAD_STACK`].
pub use lua::lifecycle::VM_THREAD_STACK;

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
    lua::std::process::kill_all();
}
