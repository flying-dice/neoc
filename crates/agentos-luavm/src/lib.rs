//! A sandboxed standard Lua 5.1 VM (mlua, vendored): the engine factory, the strict `require` registry
//! (`std:*`, `lib:*`, `vnd:*`), the task scheduler and the tracing layer.
//!
//! This crate depends on no other `agentos-*` crate: it is the Lua runtime, not an adapter. The adapter
//! that implements the `AppRuntime` port is built on top of it.
//!
//! # Lints
//!
//! The workspace lints apply (clippy pedantic, no `unsafe`, no `unwrap`/`expect`/`panic!` outside tests).
//! The allows below are the pedantic lints this binding layer trips by construction, not by neglect:
//!
//! - `cast_*`: Lua numbers are IEEE doubles; every module converts between `f64`, `i64` and `usize` at
//!   the Lua boundary, after range-checking the Lua-side value.
//! - `needless_pass_by_value`: mlua callbacks receive their arguments by value (`FromLua`); the
//!   signature is fixed by the binding, not chosen.
//! - `unnecessary_wraps`: callback registration needs `mlua::Result` even for infallible bodies.
//! - `too_many_lines`: a module's `declare` is one flat registration table.
//! - `format_push_string`, `manual_let_else`, `many_single_char_names`, `similar_names`,
//!   `match_same_arms`: style lints. The modules build short messages and date arithmetic whose clearest
//!   form trips them; none changes behaviour.
//! - in tests only: `float_cmp` (Lua numbers that are exactly representable, integer-valued doubles) and
//!   `case_sensitive_file_extension_comparisons` (asserting a fixed suffix we just generated).
//! - with the opt-in `bench` feature only: `unwrap_used`, `expect_used`, `panic`, `missing_panics_doc`
//!   `needless_raw_string_hashes`, `unit_arg` and `await_holding_refcell_ref`. The inline `benches` functions are benchmark harness code, where
//!   a failed setup should abort the run.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::format_push_string,
    clippy::manual_let_else,
    clippy::many_single_char_names,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::unnecessary_wraps
)]
#![cfg_attr(test, allow(clippy::float_cmp, clippy::case_sensitive_file_extension_comparisons))]
#![cfg_attr(
    feature = "bench",
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::missing_panics_doc,
        clippy::needless_raw_string_hashes,
        clippy::unit_arg,
        clippy::await_holding_refcell_ref
    )
)]

/// The `mlua` this crate is built on, re-exported so the adapter on top names one binding and `deny.toml`'s
/// `mlua` wrapper rule (only this crate depends on it directly) holds.
pub use mlua;

pub mod dto;
pub mod errors;
pub mod lua;
