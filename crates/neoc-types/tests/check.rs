//! Static-stage assertions for `neoc_types::check`, driven through the real
//! parser (source → AST → checker). Each test cites the `STATIC.md` rule it
//! exercises. Well-typed programs must produce zero diagnostics; ill-typed
//! programs must produce the exact expected message(s).

use neoc_types::check;

/// Parses `src`, asserts the parser itself raised no diagnostics (so a test
/// failure is about the checker, not a parse problem), then runs the checker.
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

// ---- clean programs (STATIC.md §3-§4) -------------------------------------

#[test]
fn clean_arithmetic() {
    assert_clean("fn add(a: number, b: number) -> number { a + b }");
}

#[test]
fn clean_string_concat() {
    assert_clean("fn greet(n: string) -> string { \"hi \" + n }");
}

#[test]
fn clean_struct_and_field() {
    assert_clean("struct Point { x: number, y: number } fn px(p: Point) -> number { p.x }");
}

#[test]
fn clean_struct_literal() {
    assert_clean(
        "struct Point { x: number, y: number } fn origin() -> Point { Point { x: 0, y: 0 } }",
    );
}

#[test]
fn clean_newtype_field() {
    assert_clean("struct Id(number); fn raw(i: Id) -> number { i.0 }");
}

#[test]
fn clean_if_expression() {
    assert_clean("fn pick(c: bool) -> number { if c { 1 } else { 2 } }");
}

#[test]
fn clean_ext_fn_call() {
    assert_clean("ext fn now() -> number; fn t() -> number { now() }");
}

#[test]
fn clean_type_alias_is_transparent() {
    assert_clean("type Id = number; fn f(i: Id) -> number { i + 1 }");
}

#[test]
fn clean_generic_constructor() {
    assert_clean("fn ok() -> Result<number, string> { Ok(1) }");
}

#[test]
fn clean_closure() {
    assert_clean("fn use_it() -> number { let f = |x: number| x + 1; f(2) }");
}

// ---- unknown names (STATIC.md §2) -----------------------------------------

#[test]
fn unknown_name() {
    let d = diags("fn f() -> number { missing }");
    assert_eq!(d, vec!["unknown name `missing`".to_string()]);
}

#[test]
fn unknown_struct_in_literal() {
    let d = diags("fn f() { Ghost { x: 1 }; }");
    assert!(d.iter().any(|m| m == "unknown struct `Ghost`"), "{d:?}");
}

// ---- type mismatches (STATIC.md §4) ---------------------------------------

#[test]
fn return_type_mismatch() {
    let d = diags("fn f() -> number { \"oops\" }");
    assert_eq!(
        d,
        vec!["function body expects `number`, found `string`".to_string()]
    );
}

#[test]
fn arithmetic_on_string() {
    let d = diags("fn f(s: string) -> number { s - 1 }");
    assert!(
        d.iter()
            .any(|m| m == "arithmetic operand expects `number`, found `string`"),
        "{d:?}"
    );
}

#[test]
fn plus_on_bool_is_rejected() {
    let d = diags("fn f(a: bool, b: bool) -> bool { a + b }");
    assert!(
        d.iter()
            .any(|m| m == "`+` expects two `number`s or two `string`s, found `bool` and `bool`"),
        "{d:?}"
    );
}

#[test]
fn wrong_field_type_in_struct_literal() {
    let d =
        diags("struct Point { x: number, y: number } fn f() -> Point { Point { x: \"a\", y: 0 } }");
    assert!(
        d.iter()
            .any(|m| m == "field `x` of `Point` expects `number`, found `string`"),
        "{d:?}"
    );
}

#[test]
fn missing_field_in_struct_literal() {
    let d = diags("struct Point { x: number, y: number } fn f() -> Point { Point { x: 0 } }");
    assert!(
        d.iter().any(|m| m == "missing field `y` in `Point`"),
        "{d:?}"
    );
}

#[test]
fn field_access_on_non_struct() {
    let d = diags("fn f(n: number) -> number { n.x }");
    assert!(
        d.iter().any(|m| m == "type `number` has no fields"),
        "{d:?}"
    );
}

#[test]
fn arg_count_mismatch() {
    let d = diags("fn add(a: number, b: number) -> number { a + b } fn f() -> number { add(1) }");
    assert!(
        d.iter()
            .any(|m| m == "this call passes 1 argument(s) but the function takes 2"),
        "{d:?}"
    );
}

#[test]
fn arg_type_mismatch() {
    let d = diags("fn id(x: number) -> number { x } fn f() -> number { id(\"no\") }");
    assert!(
        d.iter()
            .any(|m| m == "argument 1 expects `number`, found `string`"),
        "{d:?}"
    );
}

#[test]
fn if_branches_disagree() {
    let d = diags("fn f(c: bool) -> number { if c { 1 } else { \"x\" } }");
    assert!(
        d.iter()
            .any(|m| m == "`if` branches disagree: `number` vs `string`"),
        "{d:?}"
    );
}

// ---- match exhaustiveness (STATIC.md §7) ----------------------------------

#[test]
fn clean_exhaustive_match() {
    assert_clean("fn f(o: Option<number>) -> number { match o { Some(x) => x, None => 0 } }");
}

#[test]
fn clean_exhaustive_struct_match() {
    // A struct has one shape; an irrefutable struct/tuple pattern is exhaustive.
    assert_clean(
        "struct Point { x: number, y: number } \
         fn sum(p: Point) -> number { match p { Point { x, y } => x + y } }",
    );
    assert_clean("struct Id(number); fn raw(i: Id) -> number { match i { Id(n) => n } }");
}

#[test]
fn refutable_struct_match_still_needs_catch_all() {
    let d = diags(
        "struct Point { x: number, y: number } \
         fn f(p: Point) -> number { match p { Point { x: 0, y } => y } }",
    );
    assert!(d.iter().any(|m| m.contains("not exhaustive")), "{d:?}");
}

#[test]
fn clean_exhaustive_qualified_unit_variant() {
    // Regression: a unit variant covered by its qualified path (`Shape::Dot`)
    // must count toward exhaustiveness, alongside tuple and struct variants.
    assert_clean(
        "enum Shape { Dot, Move(number, number), Circle { radius: number } }\n\
         fn area(s: Shape) -> number {\n\
           match s {\n\
             Shape::Dot => 0,\n\
             Shape::Move(a, b) => a + b,\n\
             Shape::Circle { radius } => radius,\n\
           }\n\
         }",
    );
}

#[test]
fn non_exhaustive_enum_match() {
    let d = diags(
        "enum Color { Red, Green, Blue } \
         fn f(c: Color) -> number { match c { Green => 1, Blue => 2 } }",
    );
    assert!(
        d.iter()
            .any(|m| m == "this match on `Color` is not exhaustive; missing variant(s): Red"),
        "{d:?}"
    );
}

#[test]
fn non_exhaustive_result_match() {
    let d = diags("fn f(r: Result<number, string>) -> number { match r { Ok(v) => v } }");
    assert!(
        d.iter()
            .any(|m| m == "this match on `Result` is not exhaustive; missing variant(s): Err"),
        "{d:?}"
    );
}

#[test]
fn non_exhaustive_nested_variant_payload() {
    // A variant's payload must itself be covered: `Some` and `None` are both
    // named, but `Some(false)` is uncovered (STATIC.md §7).
    let d = diags(
        "enum Opt { Some(bool), None } \
         fn f(o: Opt) -> number { match o { Opt::Some(true) => 1, Opt::None => 0 } }",
    );
    assert!(
        d.iter()
            .any(|m| m == "this match on `Opt` is not exhaustive; missing variant(s): Some(false)"),
        "{d:?}"
    );
}

#[test]
fn clean_exhaustive_nested_variant_payload() {
    assert_clean(
        "enum Opt { Some(bool), None } \
         fn f(o: Opt) -> number { match o { Opt::Some(true) => 1, Opt::Some(false) => 2, Opt::None => 0 } }",
    );
}

#[test]
fn guarded_arm_does_not_cover() {
    // A guarded arm cannot guarantee coverage, so it leaves the value missing.
    let d = diags(
        "enum Color { Red, Green, Blue } \
         fn f(c: Color, b: bool) -> number { match c { Red if b => 0, Green => 1, Blue => 2 } }",
    );
    assert!(
        d.iter()
            .any(|m| m == "this match on `Color` is not exhaustive; missing variant(s): Red"),
        "{d:?}"
    );
}

#[test]
fn non_exhaustive_bool_match() {
    let d = diags("fn f(b: bool) -> number { match b { true => 1 } }");
    assert!(
        d.iter()
            .any(|m| m.contains("match on `bool` is not exhaustive")),
        "{d:?}"
    );
}

#[test]
fn catch_all_makes_match_exhaustive() {
    assert_clean(
        "enum Color { Red, Green, Blue } \
         fn f(c: Color) -> number { match c { Green => 1, _ => 0 } }",
    );
}

#[test]
fn redundant_arm_after_catch_all() {
    let d = diags("fn f(o: Option<number>) -> number { match o { x => 0, None => 1 } }");
    assert!(
        d.iter()
            .any(|m| m
                == "this match arm is unreachable; an earlier arm already matches every value"),
        "{d:?}"
    );
}

#[test]
fn number_match_needs_catch_all() {
    let d = diags("fn f(n: number) -> number { match n { 1 => 10, 2 => 20 } }");
    assert!(
        d.iter()
            .any(|m| m.contains("literal arms can never be exhaustive")),
        "{d:?}"
    );
}

// ---- the `?` operator (STATIC.md §6) --------------------------------------

#[test]
fn clean_try_in_result_fn() {
    assert_clean(
        "ext fn parse(s: string) -> Result<number, string>; \
         fn f(s: string) -> Result<number, string> { let n = parse(s)?; Ok(n) }",
    );
}

#[test]
fn try_outside_result_fn() {
    let d = diags(
        "ext fn parse(s: string) -> Result<number, string>; \
         fn f(s: string) -> number { parse(s)? }",
    );
    assert!(
        d.iter()
            .any(|m| m == "`?` on a `Result` is only valid in a function returning `Result`"),
        "{d:?}"
    );
}

#[test]
fn try_error_type_mismatch() {
    let d = diags(
        "ext fn parse(s: string) -> Result<number, string>; \
         fn f(s: string) -> Result<number, bool> { let n = parse(s)?; Ok(n) }",
    );
    assert!(
        d.iter().any(|m| m
            == "`?` error type `string` does not match the function's error type `bool`"),
        "{d:?}"
    );
}

#[test]
fn try_on_non_result() {
    let d = diags("fn f() -> Result<number, string> { let x = 5; x? }");
    assert!(
        d.iter()
            .any(|m| m == "`?` expects a `Result` or `Option`, found `number`"),
        "{d:?}"
    );
}

// ---- trait-impl completeness (STATIC.md §8) -------------------------------

#[test]
fn clean_complete_trait_impl() {
    assert_clean(
        "trait Greet { fn hello(self) -> string; } \
         struct P; \
         impl Greet for P { fn hello(self) -> string { \"hi\" } }",
    );
}

#[test]
fn incomplete_trait_impl() {
    let d = diags(
        "trait Shape { fn area(self) -> number; fn name(self) -> string; } \
         struct Sq; \
         impl Shape for Sq { fn area(self) -> number { 1 } }",
    );
    assert!(
        d.iter()
            .any(|m| m == "`impl Shape for Sq` is missing method `name`"),
        "{d:?}"
    );
}

#[test]
fn extraneous_trait_impl_method() {
    let d = diags(
        "trait Shape { fn area(self) -> number; } \
         struct Sq; \
         impl Shape for Sq { fn area(self) -> number { 1 } fn extra(self) -> number { 2 } }",
    );
    assert!(
        d.iter()
            .any(|m| m == "method `extra` is not a member of trait `Shape`"),
        "{d:?}"
    );
}

#[test]
fn trait_default_method_may_be_omitted() {
    assert_clean(
        "trait Greet { fn hi(self) -> string { \"hi\" } fn bye(self) -> string; } \
         struct P; \
         impl Greet for P { fn bye(self) -> string { \"bye\" } }",
    );
}

#[test]
fn trait_impl_parameter_type_must_match() {
    let d = diags(
        "trait Show { fn show(self, n: number) -> string; } \
         struct P; \
         impl Show for P { fn show(self, n: string) -> string { \"x\" } }",
    );
    assert!(
        d.iter().any(|m| m
            == "method `show` does not match trait `Show`: expected `fn(number) -> string`, found `fn(string) -> string`"),
        "{d:?}"
    );
}

#[test]
fn trait_impl_return_type_must_match() {
    let d = diags(
        "trait Area { fn area(self) -> number; } \
         struct Sq; \
         impl Area for Sq { fn area(self) -> string { \"big\" } }",
    );
    assert!(
        d.iter().any(|m| m
            == "method `area` does not match trait `Area`: expected `fn() -> number`, found `fn() -> string`"),
        "{d:?}"
    );
}

#[test]
fn trait_mismatch_labels_the_trait_declaration() {
    // A conformance mismatch anchors on the impl method and carries a related
    // label pointing at the trait's declaration (STATIC.md §8).
    let src = "trait Area { fn area(self) -> number; } \
               struct Sq; \
               impl Area for Sq { fn area(self) -> string { \"big\" } }";
    let parsed = neoc_parser::parse(src);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let diags = check(&parsed.module).diagnostics;
    let mismatch = diags
        .iter()
        .find(|d| d.message.contains("does not match trait `Area`"))
        .expect("expected a conformance mismatch");
    assert_eq!(mismatch.labels.len(), 1, "{:?}", mismatch.labels);
    assert!(
        mismatch.labels[0]
            .message
            .contains("declared here in trait `Area`"),
        "{:?}",
        mismatch.labels
    );
    // The label points at the trait's `area`, which precedes the impl's.
    assert!(
        mismatch.labels[0].start < mismatch.start,
        "label {:?} should precede the impl method at {}",
        mismatch.labels[0],
        mismatch.start
    );
}

#[test]
fn trait_impl_self_returning_method_matches() {
    // `-> Self` in the trait reads as the target type in the impl: a faithful
    // `-> P` impl conforms.
    assert_clean(
        "trait Dup { fn dup(self) -> Self; } \
         struct P; \
         impl Dup for P { fn dup(self) -> P { P } }",
    );
}

// ---- method calls & associated functions (STATIC.md §3.4) -----------------

#[test]
fn clean_method_call() {
    assert_clean(
        "struct Counter { n: number } \
         impl Counter { fn get(self) -> number { self.n } } \
         fn f(c: Counter) -> number { c.get() }",
    );
}

#[test]
fn unknown_method() {
    let d = diags("struct P { n: number } fn f(p: P) -> number { p.nope() }");
    assert!(
        d.iter().any(|m| m == "type `P` has no method `nope`"),
        "{d:?}"
    );
}

#[test]
fn clean_associated_function() {
    assert_clean(
        "struct P { n: number } \
         impl P { fn make() -> P { P { n: 0 } } } \
         fn f() -> P { P::make() }",
    );
}

// ---- lists (STATIC.md §6.5) -----------------------------------------------

#[test]
fn clean_list_and_index() {
    assert_clean("fn f(xs: [number]) -> number { xs[0] }");
}

#[test]
fn list_elements_must_agree() {
    let d = diags("fn f() -> [number] { [1, \"two\"] }");
    assert!(
        d.iter()
            .any(|m| m == "list elements disagree: `number` vs `string`"),
        "{d:?}"
    );
}

// ---- derived methods (DYNAMIC.md §6) --------------------------------------

#[test]
fn derive_clone_method() {
    assert_clean("#[derive(Clone)] struct P { x: number } fn f(p: P) -> P { p.clone() }");
}

#[test]
fn derive_eq_returns_bool() {
    assert_clean(
        "#[derive(Eq)] struct P { x: number } \
         fn f(p: P, q: P) -> number { if p.eq(q) { 1 } else { 0 } }",
    );
}

#[test]
fn derive_eq_wrong_arg_is_error() {
    let d = diags("#[derive(Eq)] struct P { x: number } fn f(p: P) -> bool { p.eq(1) }");
    assert!(
        d.iter()
            .any(|m| m == "argument 1 of `eq` expects `P`, found `number`"),
        "{d:?}"
    );
}

#[test]
fn derive_display_returns_string() {
    assert_clean("#[derive(Display)] struct P { x: number } fn f(p: P) -> string { p.display() }");
}

#[test]
fn derive_debug_returns_string() {
    assert_clean("#[derive(Debug)] struct P { x: number } fn f(p: P) -> string { p.debug() }");
}

#[test]
fn multiple_derives_register_each() {
    assert_clean(
        "#[derive(Clone, Eq, Debug, Display)] struct P { x: number } \
         fn f(p: P, q: P) -> string { \
             let c = p.clone(); \
             let _ = c.eq(q); \
             let _ = c.debug(); \
             c.display() \
         }",
    );
}

#[test]
fn no_clone_derive_is_unknown_method() {
    let d = diags("struct P { x: number } fn f(p: P) -> P { p.clone() }");
    assert!(
        d.iter().any(|m| m == "type `P` has no method `clone`"),
        "{d:?}"
    );
}

#[test]
fn absent_derive_not_registered() {
    // Deriving only `Clone` must not conjure an `eq` method.
    let d = diags("#[derive(Clone)] struct P { x: number } fn f(p: P, q: P) -> bool { p.eq(q) }");
    assert!(
        d.iter().any(|m| m == "type `P` has no method `eq`"),
        "{d:?}"
    );
}

#[test]
fn user_impl_takes_precedence_over_derive() {
    // A hand-written `clone` returning `number` wins over the derived one,
    // so the body type-checks against `number`.
    assert_clean(
        "#[derive(Clone)] struct P { x: number } \
         impl P { fn clone(self) -> number { self.x } } \
         fn f(p: P) -> number { p.clone() }",
    );
}

#[test]
fn derive_on_enum_method() {
    assert_clean(
        "#[derive(Clone, Eq)] enum Color { Red, Green, Blue } \
         fn f(c: Color, d: Color) -> bool { let _ = c.clone(); c.eq(d) }",
    );
}

#[test]
fn derive_on_generic_struct() {
    assert_clean(
        "#[derive(Clone)] struct Box<T> { value: T } \
         fn f(b: Box<number>) -> Box<number> { b.clone() }",
    );
}

// ---- located diagnostics ---------------------------------------------------

#[test]
fn unknown_name_diagnostic_spans_the_identifier() {
    // The `unknown name` diagnostic's byte range must slice the source to the
    // offending identifier, so the LSP underlines `missing` rather than line 0.
    let src = "fn f() -> number { missing }";
    let parsed = neoc_parser::parse(src);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let result = check(&parsed.module);
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("unknown name"))
        .expect("expected an `unknown name` diagnostic");
    assert_eq!(&src[d.start..d.end], "missing");
}
