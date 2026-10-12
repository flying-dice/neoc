//! Supertraits (`STATIC.md` §3.4): `trait A: B` requires an implementor of `A`
//! to implement `B`, grants `B`'s methods to a bound by `A`, and lets an `A`
//! value flow where `B` is expected.

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    check(&neoc_parser::parse(src).module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const HIER: &str = "trait Show {\n  fn show(self) -> string;\n}\n\
trait Greet: Show {\n  fn greet(self) -> string;\n}\n\
struct P;\n\
impl Show for P {\n  fn show(self) -> string {\n    \"p\"\n  }\n}\n\
impl Greet for P {\n  fn greet(self) -> string {\n    \"hi\"\n  }\n}\n";

#[test]
fn a_bound_by_a_subtrait_grants_the_supertrait_methods() {
    let src = format!("{HIER}fn describe<T: Greet>(x: T) -> string {{\n  x.show()\n}}\n");
    assert!(
        diagnostics(&src).is_empty(),
        "`T: Greet` grants `show` from the supertrait `Show`: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_subtrait_value_is_assignable_to_a_supertrait_position() {
    let src = format!(
        "{HIER}fn want_show(s: Show) -> string {{\n  s.show()\n}}\nfn go() -> string {{\n  let g: Greet = P;\n  want_show(g)\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "a `Greet` value flows into a `Show` position: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_supertrait_value_is_not_assignable_to_a_subtrait_position() {
    let src = format!(
        "{HIER}fn want_greet(g: Greet) -> string {{\n  g.greet()\n}}\nfn go() -> string {{\n  let s: Show = P;\n  want_greet(s)\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("argument 1 expects `Greet`, found `Show`")),
        "a `Show` value does not satisfy a `Greet` position: {d:?}"
    );
}

#[test]
fn implementing_a_subtrait_requires_the_supertrait() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\n\
trait Greet: Show {\n  fn greet(self) -> string;\n}\n\
struct P;\n\
impl Greet for P {\n  fn greet(self) -> string {\n    \"hi\"\n  }\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m
                == "`P` implements `Greet` but not its supertrait `Show`; add `impl Show for P`"),
        "an implementor of `Greet` must also implement `Show`: {d:?}"
    );
}

#[test]
fn an_unknown_supertrait_is_an_error() {
    let d = diagnostics("trait Greet: Bogus {\n  fn greet(self) -> string;\n}\n");
    assert!(
        d.iter()
            .any(|m| m == "unknown supertrait `Bogus` on trait `Greet`"),
        "{d:?}"
    );
}

#[test]
fn a_supertrait_cycle_is_an_error() {
    let d = diagnostics(
        "trait A: B {\n  fn a(self) -> string;\n}\ntrait B: A {\n  fn b(self) -> string;\n}\n",
    );
    assert!(
        d.iter().any(|m| m.contains("supertrait cycle")),
        "mutually dependent supertraits are rejected: {d:?}"
    );
}

#[test]
fn an_ill_arity_supertrait_reports_only_its_arity() {
    let src = "trait Base<T> {\n  fn base(self) -> T;\n}\n\
trait Sub<T>: Base {\n  fn sub(self) -> T;\n}\n\
struct C {\n  n: number,\n}\n\
impl Sub<number> for C {\n  fn sub(self) -> number {\n    self.n\n  }\n}\n";
    assert_eq!(
        diagnostics(src),
        vec!["trait `Base` expects 1 type argument(s), found 0"]
    );
}
