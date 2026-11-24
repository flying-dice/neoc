//! The modules of the runtime, by namespace: `std:*` mirrors the Rust standard library, `lib:*` is gap-fillers and
//! neoc's own modules, `vnd:*` is vendored crates named after the crate. A script reaches a module only through
//! `require("namespace:name")`; there is no ambient global.
//!
//! The registry (`names`, `build`, `install`, `install_allowed`) is `neoc-vm-core`'s. A module appears in it by
//! existing: writing `#[mlua_ir::module("lib:base64")] pub mod base64 { ... }` registers it. The only line a new
//! module needs in this crate is its `pub mod` in `lib/mod.rs`, `std/mod.rs` or `vnd/mod.rs`, which gets it compiled
//! and linked.

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

pub mod lib;
pub mod std;
pub mod vnd;

pub use neoc_vm_core::modules::{build, install, install_allowed, names, unknown_module};
