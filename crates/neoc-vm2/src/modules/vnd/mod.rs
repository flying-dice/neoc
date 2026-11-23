//! `vnd:*` modules: vendored crates, each named after its crate. A module is a `#[mlua_ir::module("vnd:<name>")]`
//! inside its file; its `pub mod` line here is what compiles and links it.

pub mod serde_json;
pub mod serde_yaml;
pub mod toml;
