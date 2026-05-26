//! Bound-directed method resolution (`STATIC.md` §method resolution): a method
//! call on a bounded type parameter resolves through the bound trait's methods.

use neoc_types::check;

fn diagnostics(src: &str) -> Vec<String> {
    check(&neoc_parser::parse(src).module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const TRAIT: &str = "trait Show {\n  fn show(self) -> string;\n}\n";

#[test]
fn bounded_param_method_resolves_inline() {
    let src = format!("{TRAIT}fn describe<T: Show>(x: T) -> string {{\n  x.show()\n}}\n");
    assert!(
        diagnostics(&src).is_empty(),
        "a method from the parameter's bound should resolve: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn bounded_param_method_resolves_via_where_clause() {
    let src = format!("{TRAIT}fn describe<T>(x: T) -> string where T: Show {{\n  x.show()\n}}\n");
    assert!(
        diagnostics(&src).is_empty(),
        "a `where` bound should resolve the method too: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn unbounded_param_method_is_an_error() {
    let src = format!("{TRAIT}fn describe<T>(x: T) -> string {{\n  x.show()\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("type parameter `T` has no method `show`")),
        "an unbounded parameter has no methods: {d:?}"
    );
}

#[test]
fn a_method_not_on_the_bound_is_an_error() {
    let src = format!("{TRAIT}fn describe<T: Show>(x: T) -> string {{\n  x.nope()\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("type parameter `T` has no method `nope`")),
        "only the bound's methods resolve: {d:?}"
    );
}

const SINK: &str = "trait Sink {\n  fn take(self, x: string) -> bool;\n}\n";

#[test]
fn bound_method_call_checks_arity() {
    let src = format!("{SINK}fn run<T: Sink>(s: T) -> bool {{\n  s.take()\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("method `take` takes 1 argument(s) but 0 were supplied")),
        "a bound method's arity is enforced: {d:?}"
    );
}

#[test]
fn bound_method_call_checks_argument_types() {
    let src = format!("{SINK}fn run<T: Sink>(s: T) -> bool {{\n  s.take(42)\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("argument 1 of `take` expects `string`, found `number`")),
        "a bound method's argument types are checked: {d:?}"
    );
}
