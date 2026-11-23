//! `lib:*` modules: gap-fillers and neoc's own. A module is a `#[mlua_ir::module("lib:<name>")]` inside its file;
//! its `pub mod` line here is what compiles and links it.

pub mod base64;
pub mod hex;
pub mod hash;
pub mod hmac;
pub mod random;
pub mod crypto;
pub mod pkey;
pub mod text;
pub mod unicode;
pub mod regex;
pub mod diff;
