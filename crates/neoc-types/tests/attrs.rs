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
fn derive_eq_with_prelude_and_list_fields_is_clean() {
    assert_clean(
        "#[derive(Eq)] struct Bag { o: Option<number>, r: Result<number, string>, xs: [number] }",
    );
}

#[test]
fn derive_eq_with_non_eq_option_payload_is_rejected() {
    assert_diag(
        "struct NoEq { v: number } #[derive(Eq)] struct S { o: Option<NoEq> }",
        "deriving Eq needs field `o` of type `Option<NoEq>` to support equality",
    );
}

#[test]
fn derive_eq_with_non_eq_result_and_list_elements_is_rejected() {
    let d = diags(
        "struct NoEq { v: number } \
         #[derive(Eq)] struct S { r: Result<number, NoEq>, xs: [NoEq] }",
    );
    assert_eq!(
        d,
        vec![
            "deriving Eq needs field `r` of type `Result<number, NoEq>` to support equality"
                .to_string(),
            "deriving Eq needs field `xs` of type `[NoEq]` to support equality".to_string(),
        ]
    );
}

#[test]
fn derive_eq_with_non_eq_type_argument_is_rejected() {
    assert_diag(
        "struct NoEq { v: number } #[derive(Eq)] struct Wrap<T> { v: T } \
         #[derive(Eq)] struct S { w: Wrap<NoEq> }",
        "deriving Eq needs field `w` of type `Wrap<NoEq>` to support equality",
    );
}

#[test]
fn derive_debug_with_non_debug_field_is_rejected() {
    let d = diags(
        "struct Plain { v: number } \
         #[derive(Debug)] struct S { p: Plain, o: Option<Plain> }",
    );
    assert_eq!(
        d,
        vec![
            "deriving Debug needs field `p` of type `Plain` to satisfy Debug".to_string(),
            "deriving Debug needs field `o` of type `Option<Plain>` to satisfy Debug".to_string(),
        ]
    );
}

#[test]
fn derive_debug_with_aggregate_debug_fields_is_clean() {
    assert_clean(
        "#[derive(Debug)] struct Inner { v: number } \
         #[derive(Debug)] struct S { i: Inner, o: Option<Inner>, xs: [Inner], r: Result<Inner, string> }",
    );
}

#[test]
fn derive_display_with_option_result_or_list_field_is_rejected() {
    let d = diags(
        "#[derive(Display)] struct S { o: Option<number>, r: Result<number, string>, xs: [number] }",
    );
    assert_eq!(
        d,
        vec![
            "deriving Display needs field `o` of type `Option<number>` to satisfy Display"
                .to_string(),
            "deriving Display needs field `r` of type `Result<number, string>` to satisfy Display"
                .to_string(),
            "deriving Display needs field `xs` of type `[number]` to satisfy Display".to_string(),
        ]
    );
}

#[test]
fn derive_display_with_non_display_field_is_rejected() {
    assert_diag(
        "struct Inner { x: number } #[derive(Display)] struct Outer { i: Inner }",
        "deriving Display needs field `i` of type `Inner` to satisfy Display",
    );
}

// ---- extern attributes (lang/EXTERN.md §Attributes, §Errors) ---------------

#[test]
fn extern_path_on_mod_is_clean() {
    assert_clean(
        "extern { #[extern(path = \"vnd:rusqlite\")] mod rusqlite { fn open(p: string) -> Unknown; } }",
    );
}

#[test]
fn extern_ambient_on_mod_is_clean() {
    assert_clean("extern { #[extern(ambient)] mod console { fn log(s: string); } }");
}

#[test]
fn extern_name_on_fn_is_clean() {
    assert_clean("extern { #[extern(name = \"readFile\")] fn read_file(p: string) -> string; }");
}

#[test]
fn extern_constructor_on_impl_fn_is_clean() {
    assert_clean(
        "extern { struct Database; impl Database { #[extern(constructor)] fn new(p: string); } }",
    );
}

#[test]
fn extern_path_on_fn_is_rejected() {
    assert_diag(
        "extern { #[extern(path = \"m\")] fn f() -> Unknown; }",
        "`#[extern(path = \"…\")]` applies only to a `mod`",
    );
}

#[test]
fn extern_name_on_mod_is_rejected() {
    assert_diag(
        "extern { #[extern(name = \"m\")] mod fs { fn f() -> Unknown; } }",
        "`#[extern(name = \"…\")]` applies to an `fn` or `struct`, not a `mod`",
    );
}

#[test]
fn extern_constructor_outside_impl_is_rejected() {
    assert_diag(
        "extern { #[extern(constructor)] fn f() -> Unknown; }",
        "`#[extern(constructor)]` applies only to an `fn` inside an `impl`",
    );
}

#[test]
fn extern_constructor_with_return_type_is_rejected() {
    assert_diag(
        "extern { struct D; impl D { #[extern(constructor)] fn new() -> D; } }",
        "a `#[extern(constructor)]` takes no return type — it constructs `Self`",
    );
}

#[test]
fn extern_constructor_with_name_is_rejected() {
    assert_diag(
        "extern { struct D; impl D { #[extern(constructor, name = \"mk\")] fn new(); } }",
        "a constructor cannot also set `#[extern(name = \"…\")]`",
    );
}

#[test]
fn extern_unknown_param_is_rejected() {
    assert_diag(
        "extern { #[extern(bogus)] fn f() -> Unknown; }",
        "unknown `#[extern]` parameter `bogus`",
    );
}

#[test]
fn extern_nested_ambient_is_rejected() {
    assert_diag(
        "extern { #[extern(ambient)] mod a { #[extern(ambient)] mod b { fn f() -> Unknown; } } }",
        "`#[extern(ambient)]` is already in effect from an enclosing ambient `mod`",
    );
}

#[test]
fn extern_attr_outside_block_is_rejected() {
    assert_diag(
        "#[extern(ambient)] fn f() {}",
        "`extern` attributes apply only to items inside an `extern` block",
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

// ---- Option at the extern boundary (lang/EXTERN.md §Option marshalling) ----

#[test]
fn extern_option_as_whole_param_return_and_field_is_clean() {
    assert_clean(
        "extern { fn lookup(k: string, d: Option<number>) -> Option<string>; \
         struct Settings { title: Option<string> } \
         impl Settings { fn rename(self, t: Option<string>) -> Option<string>; } }",
    );
}

#[test]
fn extern_nested_option_param_is_rejected() {
    assert_diag(
        "extern { fn f(x: Option<Option<number>>); }",
        "`Option<Option<number>>` is ambiguous across an `extern` boundary: `Some(None)` and `None` both reach the host as `nil`",
    );
}

#[test]
fn extern_nested_option_return_is_rejected() {
    assert_diag(
        "extern { fn f() -> Option<Option<string>>; }",
        "`Option<Option<string>>` is ambiguous across an `extern` boundary: `Some(None)` and `None` both reach the host as `nil`",
    );
}

#[test]
fn extern_nested_option_field_is_rejected() {
    assert_diag(
        "extern { struct S { v: Option<Option<number>> } }",
        "`Option<Option<number>>` is ambiguous across an `extern` boundary: `Some(None)` and `None` both reach the host as `nil`",
    );
}

#[test]
fn extern_nested_option_through_alias_is_rejected() {
    assert_diag(
        "type Maybe = Option<number>; extern { fn f(x: Option<Maybe>); }",
        "`Option<Option<number>>` is ambiguous across an `extern` boundary: `Some(None)` and `None` both reach the host as `nil`",
    );
}

#[test]
fn extern_nested_option_in_method_is_rejected() {
    assert_diag(
        "extern { struct S; impl S { fn m(self, x: Option<Option<number>>); } }",
        "`Option<Option<number>>` is ambiguous across an `extern` boundary: `Some(None)` and `None` both reach the host as `nil`",
    );
}

#[test]
fn extern_option_in_list_is_rejected() {
    assert_diag(
        "extern { fn f() -> [Option<string>]; }",
        "`[Option<string>]` puts an `Option` inside another type at an `extern` boundary; `Option<T>` marshals only as a whole parameter, return, or field type",
    );
}

#[test]
fn extern_option_in_callback_is_rejected() {
    assert_diag(
        "extern { fn f(cb: fn(Option<number>)); }",
        "`fn(Option<number>) -> ()` puts an `Option` inside another type at an `extern` boundary; `Option<T>` marshals only as a whole parameter, return, or field type",
    );
}

#[test]
fn nested_option_outside_extern_is_clean() {
    assert_clean("fn f(x: Option<Option<number>>) -> Option<Option<number>> { x }");
}
