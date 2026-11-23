//! `lib:*` modules: gap-fillers and neoc's own. A module is a `#[mlua_ir::module("lib:<name>")]` inside its file;
//! its `pub mod` line here is what compiles and links it.

pub mod base64;
pub mod chrono;
pub mod crypto;
pub mod csv;
pub mod diff;
pub mod glob;
pub mod hash;
pub mod hex;
pub mod hmac;
pub mod html;
pub mod ini;
pub mod json;
pub mod mime;
pub mod pkey;
pub mod random;
pub mod regex;
pub mod semver;
pub mod tempfile;
pub mod text;
pub mod unicode;
pub mod url;
