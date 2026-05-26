//! Trait-as-type existential (`STATIC.md` §3.4): a trait name used as a type
//! accepts any implementor, resolves methods through the trait, and is
//! type-erasing (no downcast).

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    check(&neoc_parser::parse(src).module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const SETUP: &str = "trait Drawable {\n  fn draw(self) -> string;\n}\n\
struct Circle;\n\
impl Drawable for Circle {\n  fn draw(self) -> string {\n    \"o\"\n  }\n}\n";

#[test]
fn an_implementor_is_assignable_to_the_trait_type() {
    let src = format!(
        "{SETUP}fn render(x: Drawable) -> string {{\n  x.draw()\n}}\nfn go() -> string {{\n  render(Circle)\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`Circle` implements `Drawable`, so it is assignable to `Drawable`: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_method_resolves_through_the_trait_type() {
    let src = format!("{SETUP}fn render(x: Drawable) -> string {{\n  x.draw()\n}}\n");
    assert!(
        diagnostics(&src).is_empty(),
        "`draw` resolves on a `Drawable`-typed receiver: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn a_non_implementor_is_rejected_at_a_trait_position() {
    let src = format!(
        "{SETUP}fn render(x: Drawable) -> string {{\n  x.draw()\n}}\nfn go() -> string {{\n  render(42)\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("argument 1 expects `Drawable`, found `number`")),
        "a `number` does not implement `Drawable`: {d:?}"
    );
}

#[test]
fn a_method_not_on_the_trait_is_an_error() {
    let src = "trait Drawable {\n  fn draw(self) -> string;\n}\nfn describe(x: Drawable) -> string {\n  x.area()\n}\n";
    let d = diagnostics(src);
    assert!(
        d.iter()
            .any(|m| m == "type `Drawable` has no method `area`"),
        "only the trait's own methods resolve: {d:?}"
    );
}

#[test]
fn the_existential_is_not_downcastable_to_a_concrete_type() {
    let src = format!(
        "{SETUP}fn get() -> Drawable {{\n  Circle\n}}\nfn use_it() -> string {{\n  let c: Circle = get();\n  \"x\"\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m == "let binding expects `Circle`, found `Drawable`"),
        "a trait-typed value is not assignable to a concrete type: {d:?}"
    );
}

#[test]
fn a_homogeneous_list_of_implementors_is_assignable_to_a_trait_list() {
    let src = format!(
        "{SETUP}fn scene() -> number {{\n  let items: [Drawable] = [Circle, Circle];\n  1\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`[Circle, Circle]` is assignable to `[Drawable]`: {:?}",
        diagnostics(&src)
    );
}
