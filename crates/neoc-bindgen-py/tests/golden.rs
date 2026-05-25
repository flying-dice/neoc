//! Golden test: the `sample.py` fixture must transpile to the committed
//! `sample.neoc`, and that header must `neoc check` clean — it parses and
//! type-checks with no diagnostics.
//!
//! Exercises a typed function (`lighten`), a function returning a declared
//! handle (`to_rgb` -> `Rgb`), the `PyValue` fallback with a `// was:` comment
//! (an unannotated/`Any` `grab` and a `dict` `merge`), two annotated consts,
//! and the skipping of private names (`_private`, `_hidden`) and imports.

use neoc_bindgen_py::py_to_neoc;
use neoc_compiler::{Target, transpile};

#[test]
fn sample_fixture_matches_golden() {
    let source = include_str!("fixtures/sample.py");
    let expected = include_str!("fixtures/sample.neoc");
    let actual = py_to_neoc(source, "sample").expect("transpile sample.py");
    assert_eq!(actual, expected);
}

#[test]
fn golden_checks_clean() {
    let header = include_str!("fixtures/sample.neoc");
    let out = transpile(header, Target::Lua);
    assert!(
        out.ok(),
        "generated header should check clean, got diagnostics: {:#?}",
        out.diagnostics
    );
}
