//! `std:*` modules: they mirror the Rust standard library. A module is a `#[mlua_ir::module("std:<name>")]` inside
//! its file; its `pub mod` line here is what compiles and links it. All of them are one crate: they share the
//! helpers of one another (`std:io`'s readers and writers, `std:net`'s listeners, `std:time`'s durations) and the
//! tests that need none of the `lib:` and `vnd:` modules.

// What mlua-ir's macros imply, for every module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

pub mod collections;
pub mod env;
pub mod fs;
pub mod io;
pub mod net;
pub mod path;
pub mod process;
pub mod task;
pub mod thread;
pub mod time;
pub mod workers;

/// End the program with `code`, as `std::process::exit` does, after killing the processes its scripts started
/// (`std:process`): an exit runs no destructor and no task, so they would live on. A program that embeds the VM ends
/// with this wherever it would call `std::process::exit` (clippy refuses that one in this workspace).
pub fn exit(code: i32) -> ! {
    kill_processes();
    #[allow(clippy::disallowed_methods)] // the one place
    ::std::process::exit(code)
}

/// Kill every process a script of this program started (`std:process`) and that still runs, at once. For the end of
/// a program whose engines may outlive `main` (a worker that still runs); [`exit`] does it too.
pub fn kill_processes() {
    process::kill_all();
}
