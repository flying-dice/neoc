//! Trait default methods (`STATIC.md` §3.4): a default-bodied method may be
//! omitted or overridden by an impl, and its body sees `Self` as the trait.

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    check(&neoc_parser::parse(src).module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const SETUP: &str = "trait Greet {\n  fn name(self) -> string;\n  fn hi(self) -> string {\n    \"hi \" + self.name()\n  }\n}\n\
struct D;\n\
impl Greet for D {\n  fn name(self) -> string {\n    \"d\"\n  }\n}\n";

#[test]
fn an_omitted_default_is_callable_on_the_implementor() {
    let src = format!("{SETUP}fn go(d: D) -> string {{\n  d.hi()\n}}\n");
    assert!(diagnostics(&src).is_empty(), "{:?}", diagnostics(&src));
}

#[test]
fn a_default_resolves_through_a_generic_bound() {
    let src = format!("{SETUP}fn go<T: Greet>(x: T) -> string {{\n  x.hi()\n}}\n");
    assert!(diagnostics(&src).is_empty(), "{:?}", diagnostics(&src));
}

#[test]
fn a_default_reaches_a_supertrait_method_through_self() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\n\
trait Greet: Show {\n  fn hi(self) -> string {\n    self.show()\n  }\n}\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
}

#[test]
fn a_default_calling_a_method_the_trait_lacks_is_an_error() {
    let src = "trait Greet {\n  fn hi(self) -> string {\n    self.nope()\n  }\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m == "type parameter `Self` has no method `nope` from its bounds"),
        "{d:?}"
    );
}

#[test]
fn an_impl_missing_the_required_method_a_default_calls_is_an_error() {
    let src = "trait Greet {\n  fn name(self) -> string;\n  fn hi(self) -> string {\n    self.name()\n  }\n}\n\
struct D;\nimpl Greet for D {}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m == "`impl Greet for D` is missing method `name`"),
        "{d:?}"
    );
}

#[test]
fn a_default_is_not_granted_to_a_type_that_does_not_implement_the_trait() {
    let src = format!("{SETUP}struct Q;\nfn go(q: Q) -> string {{\n  q.hi()\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter().any(|m| m == "type `Q` has no method `hi`"),
        "{d:?}"
    );
}

#[test]
fn a_default_with_a_wrong_argument_count_is_an_error() {
    let src = format!("{SETUP}fn go(d: D) -> string {{\n  d.hi(1)\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m == "method `hi` takes 0 argument(s) but 1 were supplied"),
        "{d:?}"
    );
}
