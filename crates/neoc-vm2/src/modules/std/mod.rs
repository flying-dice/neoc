//! `std:*` modules: they mirror the Rust standard library. A module is a `#[mlua_ir::module("std:<name>")]` inside
//! its file; its `pub mod` line here is what compiles and links it.

pub mod time;
