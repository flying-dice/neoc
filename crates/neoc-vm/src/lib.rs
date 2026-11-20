// The library has no `unsafe`; the one test that edits the process environment needs it
// (`std::env::set_var` is unsafe in edition 2024).
#![cfg_attr(not(test), forbid(unsafe_code))]

pub mod errors;
pub mod lua;
