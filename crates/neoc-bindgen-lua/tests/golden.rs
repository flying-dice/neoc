//! Golden test: the `sample.luau` fixture must transpile to the committed
//! `sample.neoc`, and that generated header must `neocc check` clean on the
//! current compiler. Exercises clean primitive mapping, a typed constant, an
//! opaque declared handle (`Rgb`), and the `LuaValue` fallback with a `// was:`
//! comment (the untyped `debug` export).

use neoc_bindgen_lua::luau_to_neoc;
use neoc_compiler::{Target, transpile};

#[test]
fn sample_fixture_matches_golden() {
    let source = include_str!("fixtures/sample.luau");
    let expected = include_str!("fixtures/sample.neoc");
    let actual = luau_to_neoc(source, "sample").expect("transpile sample.luau");
    assert_eq!(actual, expected);
}

#[test]
fn generated_header_compiles_clean() {
    let source = include_str!("fixtures/sample.luau");
    let generated = luau_to_neoc(source, "sample").expect("transpile sample.luau");
    let out = transpile(&generated, Target::Lua);
    assert!(
        out.ok(),
        "generated header should parse and type-check clean; diagnostics: {:?}",
        out.diagnostics
    );
}
