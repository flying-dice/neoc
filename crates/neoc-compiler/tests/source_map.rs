//! The `SourceMap` newtype contract at the single-file `transpile` boundary.
//!
//! The Lua target records neoc↔emitted line correspondences while it lowers
//! (#56). These tests pin that contract at the `transpile` boundary so the map
//! is measured, not argued.

use neoc_compiler::{Target, transpile};

/// A trivially well-typed program. The body is irrelevant to the contract —
/// what matters is the presence of a line map.
const PROGRAM: &str = "fn main() -> number {\n  let x = 1;\n  x\n}\n";

#[test]
fn lua_transpile_carries_a_real_source_map() {
    let out = transpile(PROGRAM, Target::Lua);
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert!(
        !out.source_map.is_empty(),
        "the Lua target records a line map while lowering (#56)"
    );
    // The first emitted line is the lowered `fn main`, off neoc line 1.
    assert_eq!(out.source_map.neoc_line(1), Some(1));
}
