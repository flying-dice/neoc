//! Attribute well-formedness assertions for `neoc_types::check`
//! (`STATIC.md` §9), driven through the real parser (source → AST → checker).
//! Each test cites the rule it exercises. Well-formed attribute uses produce
//! zero diagnostics; ill-formed ones produce the exact expected message.

use neoc_types::check;

/// Parses `src`, asserts the parser raised no diagnostics (so a failure is
/// about the checker, not a parse problem), then returns the checker messages.
fn diags(src: &str) -> Vec<String> {
    let parsed = neoc_parser::parse(src);
    assert!(
        parsed.diagnostics.is_empty(),
        "parser rejected the test program: {:?}",
        parsed.diagnostics
    );
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

fn assert_clean(src: &str) {
    let d = diags(src);
    assert!(d.is_empty(), "expected no diagnostics, got: {d:?}");
}

fn assert_diag(src: &str, expected: &str) {
    let d = diags(src);
    assert!(
        d.iter().any(|m| m == expected),
        "expected diagnostic `{expected}`, got: {d:?}"
    );
}

// ---- derive: placement -----------------------------------------------------

#[test]
fn derive_on_struct_is_clean() {
    assert_clean("#[derive(Clone)] struct Point { x: number, y: number }");
}

#[test]
fn derive_on_enum_is_clean() {
    assert_clean("#[derive(Clone, Debug)] enum Color { Red, Green, Blue }");
}

#[test]
fn derive_on_fn_is_rejected() {
    assert_diag(
        "#[derive(Clone)] fn f() {}",
        "`derive` applies only to a struct or enum",
    );
}

#[test]
fn derive_on_const_is_rejected() {
    assert_diag(
        "#[derive(Clone)] const N: number = 1;",
        "`derive` applies only to a struct or enum",
    );
}

// ---- derive: argument shape ------------------------------------------------

#[test]
fn unknown_derive_name_is_rejected() {
    assert_diag(
        "#[derive(Ord)] struct S { x: number }",
        "unknown derive `Ord`; the derivable traits are Clone, Eq, Debug, Display",
    );
}

#[test]
fn unknown_derive_among_known_is_rejected() {
    assert_diag(
        "#[derive(Clone, Bogus)] struct S { x: number }",
        "unknown derive `Bogus`; the derivable traits are Clone, Eq, Debug, Display",
    );
}

#[test]
fn empty_derive_is_rejected() {
    assert_diag(
        "#[derive()] struct S;",
        "`derive` needs at least one trait, e.g. `#[derive(Clone)]`",
    );
}

#[test]
fn derive_string_arg_is_rejected() {
    assert_diag(
        "#[derive(\"Clone\")] struct S { x: number }",
        "`derive` arguments are trait names, e.g. `#[derive(Clone, Eq)]`",
    );
}

#[test]
fn all_four_derives_are_clean() {
    assert_clean("#[derive(Clone, Eq, Debug, Display)] struct P { a: number, b: string }");
}

// ---- derive: Eq / Display field obligations --------------------------------

#[test]
fn derive_eq_with_primitive_fields_is_clean() {
    assert_clean("#[derive(Eq)] struct Pair { a: number, b: string }");
}

#[test]
fn derive_eq_with_non_eq_field_is_rejected() {
    assert_diag(
        "struct Inner { x: number } #[derive(Eq)] struct Outer { i: Inner }",
        "deriving Eq needs field `i` of type `Inner` to support equality",
    );
}

#[test]
fn derive_eq_with_eq_field_is_clean() {
    assert_clean(
        "#[derive(Eq)] struct Inner { x: number } \
         #[derive(Eq)] struct Outer { i: Inner }",
    );
}

#[test]
fn derive_display_with_non_display_field_is_rejected() {
    assert_diag(
        "struct Inner { x: number } #[derive(Display)] struct Outer { i: Inner }",
        "deriving Display needs field `i` of type `Inner` to satisfy Display",
    );
}

// ---- link ------------------------------------------------------------------

#[test]
fn link_name_kv_on_extern_is_clean() {
    assert_clean("#[link(name = \"color\")] extern \"lua\" { fn paint(s: string); }");
}

#[test]
fn link_positional_on_extern_is_clean() {
    assert_clean("#[link(\"color\")] extern \"lua\" { fn paint(s: string); }");
}

#[test]
fn link_on_ext_block_is_clean() {
    assert_clean("#[link(name = \"m\")] ext { fn p(s: string); }");
}

#[test]
fn link_on_fn_is_rejected() {
    assert_diag(
        "#[link(name = \"m\")] fn f() {}",
        "`link` applies only to an extern block",
    );
}

#[test]
fn link_on_struct_is_rejected() {
    assert_diag(
        "#[link(\"m\")] struct S { x: number }",
        "`link` applies only to an extern block",
    );
}

#[test]
fn link_with_non_string_arg_is_rejected() {
    assert_diag(
        "#[link(42)] extern \"lua\" { fn p(s: string); }",
        "`link` needs a string module name, e.g. `#[link(name = \"color\")]`",
    );
}

// ---- test ------------------------------------------------------------------

#[test]
fn test_on_unit_fn_is_clean() {
    assert_clean("#[test] fn t() {}");
}

#[test]
fn test_on_result_unit_fn_is_clean() {
    assert_clean("#[test] fn t() -> Result<(), string> { Ok(()) }");
}

#[test]
fn test_on_fn_with_params_is_rejected() {
    assert_diag(
        "#[test] fn t(x: number) {}",
        "a `#[test]` function takes no parameters",
    );
}

#[test]
fn test_on_fn_returning_number_is_rejected() {
    assert_diag(
        "#[test] fn t() -> number { 1 }",
        "a `#[test]` function returns `()` or `Result<(), E>`",
    );
}

#[test]
fn test_on_struct_is_rejected() {
    assert_diag(
        "#[test] struct S { x: number }",
        "`test` applies only to a function",
    );
}

// ---- unknown attributes ----------------------------------------------------

#[test]
fn unknown_attribute_is_rejected() {
    assert_diag("#[foo] fn f() {}", "unknown attribute `foo`");
}

#[test]
fn unknown_attribute_on_struct_is_rejected() {
    assert_diag(
        "#[wibble] struct S { x: number }",
        "unknown attribute `wibble`",
    );
}

// ---- field-validation attributes -------------------------------------------

#[test]
fn min_on_number_field_is_clean() {
    assert_clean("struct S { #[min(0)] x: number }");
}

#[test]
fn max_len_on_string_field_is_clean() {
    assert_clean("struct S { #[max_len(200)] name: string }");
}

#[test]
fn max_len_on_list_field_is_clean() {
    assert_clean("struct S { #[max_len(3)] items: [number] }");
}

#[test]
fn min_on_string_field_is_rejected() {
    assert_diag(
        "struct S { #[min(0)] name: string }",
        "`min` on field `name` needs a number, not `string`",
    );
}

#[test]
fn max_len_on_number_field_is_rejected() {
    assert_diag(
        "struct S { #[max_len(3)] x: number }",
        "`max_len` on field `x` needs a string or list, not `number`",
    );
}

#[test]
fn unknown_field_attribute_is_rejected() {
    assert_diag(
        "struct S { #[bogus] x: number }",
        "unknown attribute `bogus` on field `x`",
    );
}
