//! Criterion entry point for `vnd:*` benchmarks.
//!
//! Bench *content* lives next to the module it exercises, in
//! `src/lua/vnd/<module>.rs` under `#[cfg(feature = "bench")] pub fn benches`.
//! This file is just the harness — adding a new vendored module's benches
//! means appending it to the `criterion_group!` list below.
//!
//! Each module's `benches` registers three tiers, so per-bench regressions
//! tell you exactly *where* the cost shifted:
//!   - `library`       — pure underlying crate, no mlua.
//!   - `into_out_lua`  — mlua marshalling alone (identity-shape Rust fn).
//!   - `through_lua`   — end-to-end via `require("vnd:<module>")` from Lua.
//!
//! Workflow:
//!   cargo bench --bench vnd -- --save-baseline main
//!   ...edit code...
//!   cargo bench --bench vnd -- --baseline main
//!   cargo bench --bench vnd hyper -- --baseline main    # subset
//!
//! HTML report: target/criterion/report/index.html

use agentos_luavm::lua::vnd;
use criterion::{criterion_group, criterion_main};

criterion_group!(
    benches,
    vnd::serde_json::benches,
    vnd::toml::benches,
    vnd::serde_yaml::benches,
    vnd::jsonschema::benches,
    vnd::hyper::benches,
);
criterion_main!(benches);
