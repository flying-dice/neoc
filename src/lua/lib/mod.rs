//! `lib:*` modules — blessed gap-fillers and our own custom code.
//!
//! Either plugs a hole Rust std doesn't cover (`lib:base64`) or is something
//! we wrote (`lib:test`, `lib:vcr`). Modules expose a
//! `pub fn module(lua: &Lua) -> Result<Table>` and are wired into the
//! `require` registry by `crate::lua::modules::install`.

pub mod base64;
pub mod compile;
pub mod http;
pub mod ini;
pub mod json;
pub mod mime;
pub mod test;
pub mod vcr;
