//! `std:*` modules: they mirror the Rust standard library. A module is a `#[mlua_ir::module("std:<name>")]` inside
//! its file; its `pub mod` line here is what compiles and links it.

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
