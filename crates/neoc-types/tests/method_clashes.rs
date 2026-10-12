//! One method per name on a type (`STATIC.md` §8): two explicit methods of a
//! name — inherent, trait-impl, `extern` impl or blanket — share one method
//! table and are rejected at the later impl.

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    let parsed = neoc_parser::parse(src);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const TRAITS: &str = "trait Greet {\n  fn hi(self) -> string;\n  fn a(self) -> string;\n}\n\
trait Wave {\n  fn hi(self) -> string;\n}\n\
struct D;\nstruct E;\n";

fn expect_clash(src: &str, expected: &str) {
    let d = diagnostics(src);
    assert_eq!(d, vec![expected.to_string()], "{src}");
}

#[test]
fn inherent_then_trait_impl_clashes() {
    let src = format!(
        "{TRAITS}impl D {{\n  fn hi(self) -> string {{ \"d\" }}\n}}\n\
impl Greet for D {{\n  fn hi(self) -> string {{ \"g\" }}\n  fn a(self) -> string {{ \"a\" }}\n}}\n"
    );
    expect_clash(
        &src,
        "`impl Greet for D` gives `D` a method `hi`, but `impl D` already does; a type has one method per name",
    );
}

#[test]
fn trait_impl_then_inherent_clashes() {
    let src = format!(
        "{TRAITS}impl Greet for D {{\n  fn hi(self) -> string {{ \"g\" }}\n  fn a(self) -> string {{ \"a\" }}\n}}\n\
impl D {{\n  fn hi(self) -> string {{ \"d\" }}\n}}\n"
    );
    expect_clash(
        &src,
        "`impl D` gives `D` a method `hi`, but `impl Greet for D` already does; a type has one method per name",
    );
}

#[test]
fn two_trait_impls_providing_one_name_clash() {
    let src = format!(
        "{TRAITS}impl Greet for D {{\n  fn hi(self) -> string {{ \"g\" }}\n  fn a(self) -> string {{ \"a\" }}\n}}\n\
impl Wave for D {{\n  fn hi(self) -> string {{ \"w\" }}\n}}\n"
    );
    expect_clash(
        &src,
        "`impl Wave for D` gives `D` a method `hi`, but `impl Greet for D` already does; a type has one method per name",
    );
}

#[test]
fn two_inherent_blocks_clash() {
    let src = format!(
        "{TRAITS}impl D {{\n  fn hi(self) -> string {{ \"1\" }}\n}}\nimpl D {{\n  fn hi(self) -> string {{ \"2\" }}\n}}\n"
    );
    expect_clash(
        &src,
        "`impl D` gives `D` a method `hi`, but `impl D` already does; a type has one method per name",
    );
}

#[test]
fn a_repeated_name_inside_one_impl_clashes() {
    let src = format!(
        "{TRAITS}impl D {{\n  fn hi(self) -> string {{ \"1\" }}\n  fn hi(self) -> string {{ \"2\" }}\n}}\n"
    );
    expect_clash(
        &src,
        "`impl D` gives `D` a method `hi`, but `impl D` already does; a type has one method per name",
    );
}

const EXTERN_SETTINGS: &str = "extern {\n  struct Settings {}\n  impl Settings {\n    fn rename(self, t: string) -> string;\n  }\n}\n";
const NEOC_SETTINGS: &str = "impl Settings {\n  fn rename(self, t: string) -> string { t }\n}\n";
const CLASH_SETTINGS: &str = "a method `rename`, but";

#[test]
fn neoc_impl_after_extern_impl_clashes() {
    let d = diagnostics(&format!("{EXTERN_SETTINGS}{NEOC_SETTINGS}"));
    assert_eq!(d.len(), 1, "{d:?}");
    assert!(d[0].contains(CLASH_SETTINGS), "{d:?}");
}

#[test]
fn neoc_impl_before_extern_impl_clashes() {
    // The neoc impl precedes the extern block's `struct`, so declare the type
    // in a first block and the impl in a second.
    let src = "extern {\n  struct Settings {}\n}\n".to_string()
        + NEOC_SETTINGS
        + "extern {\n  impl Settings {\n    fn rename(self, t: string) -> string;\n  }\n}\n";
    let d = diagnostics(&src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert!(d[0].contains(CLASH_SETTINGS), "{d:?}");
}

#[test]
fn blanket_impl_method_clashes_with_a_concrete_impl() {
    let src = "trait Tag {\n  fn tag(self) -> string;\n}\n\
trait Show {\n  fn show(self) -> string;\n}\n\
struct D;\n\
impl Show for D {\n  fn show(self) -> string { \"d\" }\n}\n\
impl D {\n  fn tag(self) -> string { \"own\" }\n}\n\
impl<T: Show> Tag for T {\n  fn tag(self) -> string { self.show() }\n}\n";
    expect_clash(
        src,
        "`impl Tag for T` gives `D` a method `tag`, but `impl D` already does; a type has one method per name",
    );
}

#[test]
fn distinct_names_on_one_type_are_fine() {
    let src = format!(
        "{TRAITS}impl D {{\n  fn own(self) -> string {{ \"d\" }}\n}}\n\
impl Greet for D {{\n  fn hi(self) -> string {{ \"g\" }}\n  fn a(self) -> string {{ \"a\" }}\n}}\n"
    );
    assert!(diagnostics(&src).is_empty(), "{:?}", diagnostics(&src));
}

#[test]
fn one_name_on_different_types_is_fine() {
    let src = format!(
        "{TRAITS}impl D {{\n  fn hi(self) -> string {{ \"d\" }}\n}}\n\
impl Wave for E {{\n  fn hi(self) -> string {{ \"w\" }}\n}}\n"
    );
    assert!(diagnostics(&src).is_empty(), "{:?}", diagnostics(&src));
}

#[test]
fn a_blanket_method_does_not_clash_with_a_type_it_does_not_reach() {
    let src = "trait Tag {\n  fn tag(self) -> string;\n}\n\
trait Show {\n  fn show(self) -> string;\n}\n\
struct D;\nstruct E;\n\
impl Show for D {\n  fn show(self) -> string { \"d\" }\n}\n\
impl E {\n  fn tag(self) -> string { \"own\" }\n}\n\
impl<T: Show> Tag for T {\n  fn tag(self) -> string { self.show() }\n}\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
}
