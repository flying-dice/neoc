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

#[test]
fn an_omitted_associated_fn_default_resolves_on_the_type() {
    let src = "trait Make {\n  fn tag() -> string {\n    \"made\"\n  }\n}\nstruct D;\nimpl Make for D {}\nfn go() -> string {\n  D::tag()\n}\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
}

#[test]
fn a_default_clashing_with_an_inherent_method_is_an_error() {
    let src = "trait Greet {\n  fn hi(self) -> string {\n    \"x\"\n  }\n}\nstruct D;\nimpl D {\n  fn hi(self) -> string {\n    \"y\"\n  }\n}\nimpl Greet for D {}\n";
    assert_eq!(
        diagnostics(src),
        [
            "`impl Greet for D` inherits default method `hi`, but `D` already has a method `hi`; override `hi` in the impl"
        ]
    );
}

#[test]
fn a_default_clashing_with_another_traits_method_is_an_error() {
    let src = "trait A {\n  fn a(self) -> string {\n    \"x\"\n  }\n}\ntrait B {\n  fn a(self) -> string;\n}\nstruct D;\nimpl B for D {\n  fn a(self) -> string {\n    \"y\"\n  }\n}\nimpl A for D {}\n";
    assert_eq!(
        diagnostics(src),
        [
            "`impl A for D` inherits default method `a`, but `D` already has a method `a`; override `a` in the impl"
        ]
    );
}

#[test]
fn two_traits_giving_the_same_default_is_an_error_at_each_impl() {
    let src = "trait A {\n  fn a(self) -> string {\n    \"x\"\n  }\n}\ntrait B {\n  fn a(self) -> number {\n    1\n  }\n}\nstruct D;\nimpl A for D {}\nimpl B for D {}\n";
    assert_eq!(diagnostics(src).len(), 2, "{:?}", diagnostics(src));
}

#[test]
fn a_blanket_default_clashing_with_an_inherent_method_is_an_error() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\ntrait Greet {\n  fn hi(self) -> string {\n    \"x\"\n  }\n}\nimpl<T: Show> Greet for T {}\nstruct D;\nimpl Show for D {\n  fn show(self) -> string {\n    \"d\"\n  }\n}\nimpl D {\n  fn hi(self) -> number {\n    41\n  }\n}\n";
    assert_eq!(
        diagnostics(src),
        [
            "`impl Greet for T` (for `D`) inherits default method `hi`, but `D` already has a method `hi`; override `hi` in the impl"
        ]
    );
}

#[test]
fn a_blanket_default_without_a_clash_is_accepted() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\ntrait Greet {\n  fn hi(self) -> string {\n    \"x\"\n  }\n}\nimpl<T: Show> Greet for T {}\nstruct D;\nimpl Show for D {\n  fn show(self) -> string {\n    \"d\"\n  }\n}\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
}

#[test]
fn a_default_body_naming_a_self_path_is_an_error() {
    let src = "fn name() -> string {\n  \"free\"\n}\ntrait Named {\n  fn name() -> string;\n  fn tag() -> string {\n    Self::name()\n  }\n}\n";
    assert_eq!(
        diagnostics(src),
        [
            "`Self::name` cannot be used in a default method body; it is shared by every implementor, so call a method on `self` instead"
        ]
    );
}

#[test]
fn two_blankets_giving_the_same_default_is_an_error_for_each() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\ntrait A {\n  fn a(self) -> string {\n    \"A\"\n  }\n}\ntrait B {\n  fn a(self) -> number {\n    1\n  }\n}\nimpl<T: Show> A for T {}\nimpl<T: Show> B for T {}\nstruct D;\nimpl Show for D {\n  fn show(self) -> string {\n    \"d\"\n  }\n}\n";
    let mut d = diagnostics(src);
    d.sort();
    assert_eq!(
        d,
        [
            "`impl A for T` (for `D`) inherits default method `a`, but trait `B` also gives `D` a default `a`; override `a` in the impl",
            "`impl B for T` (for `D`) inherits default method `a`, but trait `A` also gives `D` a default `a`; override `a` in the impl",
        ]
    );
}

#[test]
fn a_blanket_and_a_concrete_impl_giving_the_same_default_are_both_errors() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\ntrait A {\n  fn a(self) -> string {\n    \"A\"\n  }\n}\ntrait B {\n  fn a(self) -> number {\n    1\n  }\n}\nimpl<T: Show> A for T {}\nstruct D;\nimpl Show for D {\n  fn show(self) -> string {\n    \"d\"\n  }\n}\nimpl B for D {}\n";
    assert_eq!(diagnostics(src).len(), 2, "{:?}", diagnostics(src));
}

#[test]
fn a_blanket_method_clashing_with_an_inherited_default_is_an_error_at_both() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\ntrait A {\n  fn a(self) -> number;\n}\ntrait B {\n  fn a(self) -> string {\n    \"B\"\n  }\n}\nimpl<T: Show> A for T {\n  fn a(self) -> number {\n    1\n  }\n}\nstruct D;\nimpl Show for D {\n  fn show(self) -> string {\n    \"d\"\n  }\n}\nimpl B for D {}\n";
    let mut d = diagnostics(src);
    d.sort();
    assert_eq!(
        d,
        [
            "`impl A for T` (for `D`) gives `D` a method `a`, but trait `B` also gives `D` a default `a`; override `a` in that trait's impl",
            "`impl B for D` inherits default method `a`, but `D` already has a method `a`; override `a` in the impl",
        ]
    );
}

#[test]
fn a_blanket_method_overriding_its_own_default_is_accepted() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\ntrait A {\n  fn a(self) -> string {\n    \"x\"\n  }\n}\nimpl<T: Show> A for T {\n  fn a(self) -> string {\n    \"y\"\n  }\n}\nstruct D;\nimpl Show for D {\n  fn show(self) -> string {\n    \"d\"\n  }\n}\n";
    assert!(diagnostics(src).is_empty(), "{:?}", diagnostics(src));
}
