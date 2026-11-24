//! The modules of the runtime, by namespace: `std:*` mirrors the Rust standard library, `lib:*` is gap-fillers and
//! neoc's own modules, `vnd:*` is vendored crates named after the crate. A script reaches a module only through
//! `require("namespace:name")`; there is no ambient global.
//!
//! The registry (`names`, `build`, `install`, `install_allowed`) is `neoc-vm-core`'s. A module appears in it by
//! existing: writing `#[mlua_ir::module("lib:base64")] pub mod base64 { ... }` registers it. What a new module needs
//! in this crate is its own crate (`neoc-vm-std`, `neoc-lib-<module>` or `neoc-vnd-<module>`) among the dependencies,
//! and a `pub use` of it below, which is what links it.

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

/// `lib:*`: the modules of the `neoc-lib-*` crates, under the paths they had.
pub mod lib {
    pub use neoc_lib_archive as archive;
    pub use neoc_lib_args as args;
    pub use neoc_lib_base64 as base64;
    pub use neoc_lib_cache as cache;
    pub use neoc_lib_chrono as chrono;
    pub use neoc_lib_compile as compile;
    pub use neoc_lib_compress as compress;
    pub use neoc_lib_crypto as crypto;
    pub use neoc_lib_csv as csv;
    pub use neoc_lib_diff as diff;
    pub use neoc_lib_glob as glob;
    pub use neoc_lib_hash as hash;
    pub use neoc_lib_hex as hex;
    pub use neoc_lib_hmac as hmac;
    pub use neoc_lib_html as html;
    pub use neoc_lib_http as http;
    pub use neoc_lib_ini as ini;
    pub use neoc_lib_json as json;
    pub use neoc_lib_log as log;
    pub use neoc_lib_mime as mime;
    pub use neoc_lib_pkey as pkey;
    pub use neoc_lib_random as random;
    pub use neoc_lib_regex as regex;
    pub use neoc_lib_semver as semver;
    pub use neoc_lib_tempfile as tempfile;
    pub use neoc_lib_template as template;
    pub use neoc_lib_test as test;
    pub use neoc_lib_text as text;
    pub use neoc_lib_unicode as unicode;
    pub use neoc_lib_url as url;
    pub use neoc_lib_vcr as vcr;
    pub mod agui;
}
/// `std:*`: the modules of `neoc-vm-std`, under the paths they had.
pub mod std {
    pub use neoc_vm_std::*;
}
pub mod vnd;

pub use neoc_vm_core::modules::{build, install, install_allowed, names, unknown_module};
