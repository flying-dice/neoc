//! The `SourceMap` newtype contract at the single-file `transpile` boundary.
//!
//! Only the Luau target records neoc↔emitted line correspondences while it
//! lowers (#56); the TS and Python backends build no map, so their `SourceMap`
//! is empty and every lookup falls back to the emitted line. These tests pin
//! that contract at the `transpile` boundary so "the other targets stay
//! map-less" is measured, not argued — and a backend that later starts emitting
//! a map trips a red test rather than changing the contract in silence.

use neoc_compiler::{Target, transpile};

/// A trivially well-typed program. The body is irrelevant to the contract —
/// what matters is the per-target presence/absence of a line map.
const PROGRAM: &str = "fn main() -> number {\n  let x = 1;\n  x\n}\n";

#[test]
fn luau_transpile_carries_a_real_source_map() {
    let out = transpile(PROGRAM, Target::Luau);
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert!(
        !out.source_map.is_empty(),
        "the Luau target records a line map while lowering (#56)"
    );
    // The first emitted line is the lowered `fn main`, off neoc line 1.
    assert_eq!(out.source_map.neoc_line(1), Some(1));
}

#[test]
fn ts_transpile_emits_no_source_map() {
    let out = transpile(PROGRAM, Target::Ts);
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert!(
        out.source_map.is_empty(),
        "the TS backend builds no map (#56); its SourceMap must stay empty"
    );
    assert_eq!(
        out.source_map.neoc_line(1),
        None,
        "an empty map resolves no line"
    );
}

#[test]
fn python_transpile_emits_no_source_map() {
    let out = transpile(PROGRAM, Target::Python);
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert!(
        out.source_map.is_empty(),
        "the Python backend builds no map (#56); its SourceMap must stay empty"
    );
    assert_eq!(
        out.source_map.neoc_line(1),
        None,
        "an empty map resolves no line"
    );
}
