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

// --- Slice B: call-site bound enforcement ------------------------------------

/// A trait, a struct that implements it, and a generic fn that requires it.
const SINK_IMPL: &str = "trait Sink {\n  fn take(self, x: string) -> bool;\n}\n\
struct Wrap(number);\n\
impl Sink for Wrap {\n  fn take(self, x: string) -> bool {\n    true\n  }\n}\n\
fn run<T: Sink>(s: T) -> bool {\n  s.take(\"hi\")\n}\n";

#[test]
fn argument_satisfying_the_bound_is_accepted() {
    let src = format!("{SINK_IMPL}fn go() -> bool {{\n  run(Wrap(1))\n}}\n");
    assert!(
        diagnostics(&src).is_empty(),
        "`Wrap` implements `Sink`, so the call is well-typed: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn argument_not_implementing_the_bound_is_an_error() {
    let src = format!("{SINK_IMPL}fn go() -> bool {{\n  run(42)\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("the type `number` does not satisfy the bound `Sink`")),
        "a `number` does not implement `Sink`: {d:?}"
    );
}

#[test]
fn a_matching_bound_propagates_through_a_caller() {
    let src = format!("{SINK_IMPL}fn forward<U: Sink>(u: U) -> bool {{\n  run(u)\n}}\n");
    assert!(
        diagnostics(&src).is_empty(),
        "the caller's `U: Sink` satisfies `run`'s bound: {:?}",
        diagnostics(&src)
    );
}

#[test]
fn an_unbounded_caller_param_does_not_satisfy_the_bound() {
    let src = format!("{SINK_IMPL}fn forward<U>(u: U) -> bool {{\n  run(u)\n}}\n");
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("the type `U` does not satisfy the bound `Sink`")),
        "an unbounded `U` cannot satisfy `run`'s `Sink` bound: {d:?}"
    );
}

#[test]
fn a_derive_satisfies_the_corresponding_bound() {
    let src = "#[derive(Eq)]\nstruct P(number);\n\
fn tagged<T: Eq>(x: T) -> T {\n  x\n}\n\
fn go() -> P {\n  tagged(P(1))\n}\n";
    assert!(
        diagnostics(src).is_empty(),
        "`#[derive(Eq)]` satisfies a `T: Eq` bound: {:?}",
        diagnostics(src)
    );
}

// --- bounds at generic type construction (STATIC.md §3.5) --------------------

const SHOW: &str = "trait Show {\n  fn show(self) -> string;\n}\n";

#[test]
fn named_struct_literal_enforces_field_bound() {
    let src = format!(
        "{SHOW}struct Boxed<T: Show> {{\n  item: T,\n}}\nfn go() -> number {{\n  let b = Boxed {{ item: 42 }};\n  1\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("the type `number` does not satisfy the bound `Show`")),
        "a `number` field cannot instantiate `Boxed<T: Show>`: {d:?}"
    );
}

#[test]
fn tuple_struct_constructor_enforces_bound() {
    let src = format!(
        "{SHOW}struct Wrap<T: Show>(T);\nfn go() -> number {{\n  let w = Wrap(42);\n  1\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("the type `number` does not satisfy the bound `Show`")),
        "a tuple-struct constructor enforces its parameter bound: {d:?}"
    );
}

#[test]
fn enum_variant_constructor_enforces_bound() {
    let src = format!(
        "{SHOW}enum Holder<T: Show> {{ Has(T) }}\nfn go() -> number {{\n  let h = Holder::Has(42);\n  1\n}}\n"
    );
    let d = diagnostics(&src);
    assert!(
        d.iter()
            .any(|m| m.contains("the type `number` does not satisfy the bound `Show`")),
        "an enum variant constructor enforces its parameter bound: {d:?}"
    );
}

#[test]
fn construction_with_a_satisfying_type_is_accepted() {
    let src = format!(
        "{SHOW}struct P;\nimpl Show for P {{\n  fn show(self) -> string {{\n    \"p\"\n  }}\n}}\nstruct Boxed<T: Show> {{\n  item: T,\n}}\nfn go() -> number {{\n  let b = Boxed {{ item: P }};\n  1\n}}\n"
    );
    assert!(
        diagnostics(&src).is_empty(),
        "`P` implements `Show`, so `Boxed {{ item: P }}` is well-typed: {:?}",
        diagnostics(&src)
    );
}
