//! Criterion entry point for `lib:*` benchmarks.
//!
//! Bench *content* lives next to the module it exercises, in
//! `src/lua/lib/<module>.rs` under `#[cfg(feature = "bench")] pub fn benches`.
//! This file is just the harness — adding a new lib module's benches means
//! appending it to the `criterion_group!` list below.
//!
//! Each module's `benches` registers three tiers, so per-bench regressions
//! tell you exactly *where* the cost shifted:
//!   - `library`         — pure underlying crate, no mlua.
//!   - `into_out_lua`  — mlua marshalling alone (identity-shape Rust fn).
//!   - `through_lua`   — end-to-end via `require("lib:<module>")` from Lua.
//!
//! Workflow:
//!   cargo bench --bench lib -- --save-baseline main
//!   ...edit code...
//!   cargo bench --bench lib -- --baseline main
//!   cargo bench --bench lib base64 -- --baseline main   # subset
//!
//! HTML report: target/criterion/report/index.html

use criterion::{criterion_group, criterion_main};
use neoc::lua::lib;

criterion_group!(
    benches,
    lib::args::benches,
    lib::base64::benches,
    lib::compress::benches,
    lib::csv::benches,
    lib::glob::benches,
    lib::http::benches,
    lib::ini::benches,
    lib::json::benches,
    lib::mime::benches,
    lib::regex::benches,
    lib::semver::benches,
    lib::template::benches,
    lib::text::benches,
);
criterion_main!(benches);
