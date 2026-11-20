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
