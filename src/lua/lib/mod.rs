//! `lib:*` modules — blessed gap-fillers and our own custom code.
//!
//! Either plugs a hole Rust std doesn't cover (`lib:base64`) or is something
//! we wrote (`lib:test`, `lib:vcr`). Modules expose a
//! `pub fn module(lua: &Lua) -> Result<Table>` and are wired into the
//! `require` registry by `crate::lua::modules::install`.

pub mod args;
pub mod base64;
pub mod compile;
pub mod compress;
pub mod csv;
pub mod diff;
pub mod glob;
pub mod hash;
pub mod html;
pub mod http;
pub mod ini;
pub mod json;
pub mod log;
pub mod mime;
pub mod regex;
pub mod semver;
pub mod template;
pub mod test;
pub mod text;
pub mod url;
pub mod vcr;
