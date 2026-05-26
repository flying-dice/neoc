//! Associated types (`STATIC.md` §3.4): a trait declares `type Item;`, an impl
//! binds it (`type Item = number;`), and methods refer to it as `Self::Item`,
//! which resolves to the impl's binding.

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    check(&neoc_parser::parse(src).module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const CONTAINER: &str = "trait Container {\n  type Item;\n  fn first(self) -> Self::Item;\n}\n\
struct Bag {\n  v: number,\n}\n";

#[test]
fn an_impl_binding_resolves_self_item_in_a_method_body() {
    let src = format!(
        "{CONTAINER}impl Container for Bag {{\n  type Item = number;\n  fn first(self) -> Self::Item {{\n    self.v\n  }}\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`Self::Item` resolves to the bound `number`: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn the_impl_may_write_the_bound_type_directly() {
    let src = format!(
        "{CONTAINER}impl Container for Bag {{\n  type Item = number;\n  fn first(self) -> number {{\n    self.v\n  }}\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`-> number` conforms to `-> Self::Item` when `Item = number`: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_call_site_sees_the_projected_return_type() {
    let src = format!(
        "{CONTAINER}impl Container for Bag {{\n  type Item = number;\n  fn first(self) -> Self::Item {{\n    self.v\n  }}\n}}\nfn go(b: Bag) -> number {{\n  b.first()\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`b.first()` returns the bound `number` at the call site: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_missing_associated_type_binding_is_an_error() {
    let src = format!(
        "{CONTAINER}impl Container for Bag {{\n  fn first(self) -> Self::Item {{\n    self.v\n  }}\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m == "`impl Container for Bag` is missing associated type `Item`"),
        "an impl must bind every declared associated type: {d:?}"
    );
}

#[test]
fn binding_an_undeclared_associated_type_is_an_error() {
    let src = "trait Marker {\n  fn m(self) -> number;\n}\n\
struct P;\n\
impl Marker for P {\n  type Bogus = number;\n  fn m(self) -> number {\n    1\n  }\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m == "`Bogus` is not an associated type of trait `Marker`"),
        "a binding must name a declared associated type: {d:?}"
    );
}
