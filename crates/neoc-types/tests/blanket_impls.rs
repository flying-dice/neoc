//! Blanket impls (`STATIC.md` §3.5): `impl<T: Show> Describe for T` gives every
//! `Show` type a `Describe`. The body sees the target as a bounded parameter,
//! satisfying types gain the method, and a minimal coherence check rejects the
//! two detectable overlaps.

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    check(&neoc_parser::parse(src).module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const BLANKET: &str = "trait Show {\n  fn show(self) -> string;\n}\n\
trait Describe {\n  fn describe(self) -> string;\n}\n\
impl<T: Show> Describe for T {\n  fn describe(self) -> string {\n    self.show()\n  }\n}\n";

#[test]
fn the_blanket_body_resolves_methods_through_the_condition_bound() {
    // `self.show()` inside the blanket resolves via the target param's `Show`.
    assert!(
        diagnostics(BLANKET).is_empty(),
        "the blanket body type-checks: {:?}",
        diagnostics(BLANKET)
    );
}

#[test]
fn a_satisfying_type_gains_the_blanket_method() {
    let src = format!(
        "{BLANKET}struct P;\nimpl Show for P {{\n  fn show(self) -> string {{\n    \"p\"\n  }}\n}}\nfn go(p: P) -> string {{\n  p.describe()\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`P: Show` gains `describe` from the blanket: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_non_satisfying_type_does_not_gain_the_method() {
    let src = format!("{BLANKET}struct Q;\nfn go(q: Q) -> string {{\n  q.describe()\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter().any(|m| m == "type `Q` has no method `describe`"),
        "`Q` does not implement `Show`, so gains no `describe`: {d:?}"
    );
}

#[test]
fn the_blanket_satisfies_a_bound() {
    // A `P: Show` is accepted where `T: Describe` is required, via the blanket.
    let src = format!(
        "{BLANKET}struct P;\nimpl Show for P {{\n  fn show(self) -> string {{\n    \"p\"\n  }}\n}}\nfn needs<T: Describe>(x: T) -> string {{\n  x.describe()\n}}\nfn go() -> string {{\n  needs(P)\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`P` satisfies `Describe` via the blanket: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_blanket_body_calling_an_unbounded_method_is_an_error() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\n\
trait Greet {\n  fn greet(self) -> string;\n}\n\
trait Describe {\n  fn describe(self) -> string;\n}\n\
impl<T: Show> Describe for T {\n  fn describe(self) -> string {\n    self.greet()\n  }\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m.contains("type parameter `T` has no method `greet`")),
        "the blanket body only sees the condition bound's methods: {d:?}"
    );
}

#[test]
fn two_blankets_of_one_trait_conflict() {
    let src = "trait A {\n  fn a(self) -> string;\n}\n\
trait B {\n  fn b(self) -> string;\n}\n\
trait Show {\n  fn show(self) -> string;\n}\n\
impl<T: A> Show for T {\n  fn show(self) -> string {\n    self.a()\n  }\n}\n\
impl<T: B> Show for T {\n  fn show(self) -> string {\n    self.b()\n  }\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter().any(|m| m == "conflicting blanket impls of `Show`"),
        "two blanket impls of one trait are unorderable: {d:?}"
    );
}

#[test]
fn a_blanket_method_signature_is_checked_against_the_trait() {
    // The blanket's `describe` returns `number`, but the trait declares
    // `-> string`. A blanket impl's methods must conform like any other impl's;
    // before they were recorded under the target parameter, this mismatch went
    // unflagged because conformance found no method entry to check.
    let src = "trait Show {\n  fn show(self) -> string;\n}\n\
trait Describe {\n  fn describe(self) -> string;\n}\n\
impl<T: Show> Describe for T {\n  fn describe(self) -> number {\n    1\n  }\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m.contains("method `describe` does not match trait `Describe`")),
        "a blanket method that violates the trait signature is flagged: {d:?}"
    );
}

#[test]
fn a_satisfying_type_projects_the_blanket_associated_type() {
    // The blanket binds `type Item = number`; a satisfying type's call site must
    // see `item()` return the bound `number`, not the phantom `Self::Item`. The
    // `-> number` return on `go` only type-checks if the projection lands.
    let src = "trait Source {\n  fn raw(self) -> number;\n}\n\
trait Boxed {\n  type Item;\n  fn item(self) -> Self::Item;\n}\n\
impl<T: Source> Boxed for T {\n  type Item = number;\n  fn item(self) -> Self::Item {\n    self.raw()\n  }\n}\n\
struct P;\nimpl Source for P {\n  fn raw(self) -> number {\n    1\n  }\n}\n\
fn go(p: P) -> number {\n  p.item()\n}\n";
    assert!(
        diagnostics(src).is_empty(),
        "`p.item()` projects the blanket's `Item = number` at the call site: {:?}",
        diagnostics(src)
    );
}

#[test]
fn a_blanket_overlapping_a_concrete_impl_is_rejected() {
    let src = format!(
        "{BLANKET}struct P;\nimpl Show for P {{\n  fn show(self) -> string {{\n    \"p\"\n  }}\n}}\nimpl Describe for P {{\n  fn describe(self) -> string {{\n    \"d\"\n  }}\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m == "`impl Describe for P` overlaps the blanket impl of `Describe`"),
        "a concrete impl overlapping a blanket is rejected: {d:?}"
    );
}
