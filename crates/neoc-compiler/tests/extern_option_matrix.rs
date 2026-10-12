//! `lang/EXTERN.md` §Option marshalling as an invariant, swept across every way a
//! program can touch a host `Option`.
//!
//! A host `Option<T>` is `nil` / bare value; neoc's is a tagged table. Whatever
//! the access form, a program either marshals the crossing (the emitted Lua
//! carries `__option_to_host` / `__option_from_host`) or the checker rejects it.
//! Never silently both unmarked and accepted. Each case lists the marker its
//! form must emit when accepted.

use neoc_compiler::{Target, transpile};

const HOST: &str = r"
extern {
    struct Settings { title: Option<string>, size: number }
    fn mk() -> Settings;
    fn lookup(k: string, d: Option<number>) -> Option<string>;
    fn println(v: Unknown);
    impl Settings { fn rename(self, t: Option<string>) -> Option<string>; }
}
type S = Settings;
fn id<T>(x: T) -> T { x }
";

const TO: &str = "__option_to_host";
const FROM: &str = "__option_from_host";

/// (name, items after the host declarations, `main` body, marker when accepted)
const CASES: &[(&str, &str, &str, &str)] = &[
    ("field read", "", "let t = mk(); let x = t.title;", FROM),
    (
        "field write",
        "",
        "let t = mk(); t.title = Some(\"a\");",
        TO,
    ),
    (
        "literal",
        "",
        "let t = Settings { title: Some(\"a\"), size: 1 };",
        TO,
    ),
    (
        "literal shorthand",
        "",
        "let title = Some(\"a\"); let t = Settings { title, size: 1 };",
        TO,
    ),
    ("call param", "", "let r = lookup(\"a\", Some(1));", TO),
    ("call return", "", "let r = lookup(\"a\", None);", FROM),
    (
        "method param",
        "",
        "let t = mk(); let r = t.rename(Some(\"a\"));",
        TO,
    ),
    (
        "method return",
        "",
        "let t = mk(); let r = t.rename(None);",
        FROM,
    ),
    (
        "alias-typed read",
        "fn f(v: S) -> Option<string> { v.title }",
        "",
        FROM,
    ),
    (
        "alias-typed method",
        "fn f(v: S) { v.rename(None); }",
        "",
        TO,
    ),
    (
        "typed closure read",
        "",
        "let f = |v: Settings| v.title;",
        FROM,
    ),
    (
        "list element read",
        "fn f(a: [Settings]) { let x = a[0].title; }",
        "",
        FROM,
    ),
    (
        "for-loop element read",
        "fn f(a: [Settings]) { for s in a { let x = s.title; } }",
        "",
        FROM,
    ),
    (
        "generic passthrough read",
        "",
        "let x = id(mk()).title;",
        FROM,
    ),
    (
        "tail position read",
        "fn g() -> Option<string> { mk().title }",
        "",
        FROM,
    ),
    (
        "question mark on host result",
        "fn g() -> Option<string> { let a = lookup(\"a\", None)?; Some(a) }",
        "",
        FROM,
    ),
    (
        "let pattern",
        "",
        "let t = mk(); let Settings { title, .. } = t;",
        FROM,
    ),
    (
        "alias pattern",
        "fn f(v: S) -> Option<string> { let S { title, .. } = v; title }",
        "",
        FROM,
    ),
    (
        "untyped closure pattern",
        "",
        "let f = |v| { let Settings { title, .. } = v; title };",
        FROM,
    ),
    (
        "unknown scrutinee pattern",
        "",
        "let u: Unknown = mk(); let Settings { title, .. } = u;",
        FROM,
    ),
    (
        "match pattern",
        "fn f(v: Settings) -> Option<string> { match v { Settings { title, .. } => title } }",
        "",
        FROM,
    ),
    (
        "Self pattern",
        "impl Settings { fn f(s: Self) -> Option<string> { let Self { title, .. } = s; title } }",
        "",
        FROM,
    ),
    (
        "nested neoc struct read",
        "struct W { s: Settings } fn f(w: W) { let x = w.s.title; }",
        "",
        FROM,
    ),
    (
        "nested neoc struct write",
        "struct W { s: Settings } fn f(w: W) { w.s.title = None; }",
        "",
        TO,
    ),
    (
        "list element write",
        "fn f(a: [Settings]) { a[0].title = None; }",
        "",
        TO,
    ),
    (
        "result payload read",
        "fn f(r: Result<Settings, string>) { match r { Ok(s) => { let x = s.title; }, Err(e) => {} } }",
        "",
        FROM,
    ),
    (
        "closure capture read",
        "",
        "let t = mk(); let f = || t.title;",
        FROM,
    ),
    (
        "if branch read",
        "",
        "let t = mk(); let x = if true { t.title } else { None };",
        FROM,
    ),
    (
        "method chain",
        "",
        "let x = mk().rename(None).is_some();",
        FROM,
    ),
    (
        "neoc option forwarded to host",
        "fn f(o: Option<number>) { lookup(\"a\", o); }",
        "",
        TO,
    ),
    (
        "closure param fixed by a fn parameter",
        "fn apply(f: fn(Settings) -> Option<string>) -> Option<string> { f(mk()) }",
        "let a = apply(|v| v.title);",
        FROM,
    ),
    (
        "closure param fixed by Option::map",
        "",
        "let n = Some(mk()).map(|s| s.title.is_some());",
        FROM,
    ),
    (
        "closure param fixed through a generic call",
        "fn test<T>(x: T, f: fn(T) -> bool) -> bool { f(x) }",
        "let r = test(mk(), |s| s.title.is_some());",
        FROM,
    ),
    (
        "closure param inside a generic impl",
        "struct Wrap<T> { v: T }\nimpl<T> Wrap<T> {\n    fn with(self, f: fn(Settings, T) -> bool) -> bool { f(mk(), self.v) }\n    fn go(self) -> bool { self.with(|s, t| s.title.is_some()) }\n}",
        "",
        FROM,
    ),
    (
        "closure param in a generic fn",
        "fn apply<T>(x: T, f: fn(T) -> bool) -> bool { f(x) }\nfn outer<T>(y: T) -> bool { apply(mk(), |s| s.title.is_some()) }",
        "",
        FROM,
    ),
    (
        "Self literal",
        "impl Settings { fn f(t: Option<string>) -> Settings { Self { title: t, size: 1 } } }",
        "",
        TO,
    ),
];

/// Forms that hand a host `fn` over as a value: raw into a host slot, otherwise rejected.
const FN_VALUES: &[(&str, &str, bool)] = &[
    ("direct host slot", "println(lookup);", true),
    ("let binding", "let f = lookup;", false),
    ("neoc argument", "id(lookup);", false),
    ("list element", "let l = [lookup];", false),
    ("shorthand field", "let lookup = lookup; ", false),
];

fn build(items: &str, body: &str) -> neoc_compiler::CompileOutput {
    let src = format!("{HOST}{items}\npub fn main() {{ {body} }}\n");
    transpile(&src, Target::Lua)
}

/// How many times the output applies `marker`, not counting its definition.
fn wraps(output: &str, marker: &str) -> usize {
    let applied = output.matches(&format!("{marker}(")).count();
    let defined = output.matches(&format!("function {marker}(")).count();
    applied - defined
}

/// A callee's type parameter named like the caller's is not the caller's.
#[test]
fn callee_type_parameter_named_like_the_callers_is_its_own() {
    let out = build(
        "fn apply<T>(x: T, f: fn(T) -> number) -> number { f(x) }\nfn outer<T>(y: T) -> number { apply(\"ab\", |s| 1) }",
        "",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
}

#[test]
fn every_access_form_marshals_or_is_rejected() {
    let mut silent = Vec::new();
    let mut accepted = 0;
    for (name, items, body, marker) in CASES {
        let out = build(items, body);
        accepted += usize::from(out.ok());
        if out.ok() && wraps(&out.output, marker) == 0 {
            silent.push(format!(
                "{name}: accepted without `{marker}`\n{}",
                out.output
            ));
        }
    }
    assert!(silent.is_empty(), "{}", silent.join("\n---\n"));
    // The sweep means little if the forms are rejected for unrelated reasons.
    assert!(accepted * 2 > CASES.len(), "only {accepted} forms accepted");
}

#[test]
fn host_fn_values_pass_raw_into_host_slots_and_are_rejected_elsewhere() {
    for (name, body, raw) in FN_VALUES {
        let out = build("", body);
        if *raw {
            assert!(out.ok(), "{name}: {:#?}", out.diagnostics);
            assert!(
                !out.output.contains("function(__a"),
                "{name}: wrapped:\n{}",
                out.output
            );
        } else {
            assert!(!out.ok(), "{name}: accepted:\n{}", out.output);
        }
    }
}

/// An `Unknown`-typed value is read raw (`EXTERN.md` §Option marshalling): the
/// checker cannot tell a host `Option` from any other field there.
#[test]
fn unknown_typed_receiver_reads_raw() {
    let out = build("", "let u: Unknown = mk(); let x = u.title;");
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(!out.output.contains(FROM), "{}", out.output);
}

#[test]
fn unannotated_closure_parameter_reads_raw() {
    let out = build("", "let f = |u| u.title; println(f(mk()));");
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(!out.output.contains(FROM), "{}", out.output);
}

/// Closure parameters fixed by context other than a plain call: each site must
/// wrap the read of `s.title` itself, not merely marshal something elsewhere.
const CLOSURE_SITES: &[(&str, &str, &str)] = &[
    (
        "trait default method",
        "trait Visit {\n    fn visit(self, f: fn(Settings) -> bool) -> bool;\n    fn twice(self, f: fn(Settings) -> bool) -> bool { self.visit(f) }\n}\nstruct P { n: number }\nimpl Visit for P { fn visit(self, f: fn(Settings) -> bool) -> bool { f(mk()) } }",
        "let r = P { n: 1 }.twice(|s| s.title.is_some());",
    ),
    (
        "bounded type parameter",
        "trait Visit { fn visit(self, f: fn(Settings) -> bool) -> bool; }\nfn bounded<T: Visit>(x: T) -> bool { x.visit(|s| s.title.is_some()) }",
        "",
    ),
    (
        "struct literal field",
        "struct W { f: fn(Settings) -> bool }",
        "let w = W { f: |s| s.title.is_some() };",
    ),
    (
        "return position",
        "fn ret() -> fn(Settings) -> bool { |s| s.title.is_some() }",
        "",
    ),
    (
        "block-wrapped argument",
        "fn call(f: fn(Settings) -> bool) -> bool { f(mk()) }",
        "let r = call({ |s| s.title.is_some() });",
    ),
    (
        "if branches",
        "fn call(f: fn(Settings) -> bool) -> bool { f(mk()) }",
        "let r = call(if true { |s| s.title.is_some() } else { |s| s.size > 0 });",
    ),
    (
        "annotated let",
        "",
        "let f: fn(Settings) -> bool = |s| s.title.is_some();",
    ),
    (
        "assignment",
        "struct W { f: fn(Settings) -> bool }",
        "let w = W { f: |s| true }; w.f = |s| s.title.is_some();",
    ),
    (
        "array literal under an annotated let",
        "",
        "let fs: [fn(Settings) -> bool] = [|s| s.title.is_some()];",
    ),
    (
        "array literal argument",
        "fn run(fs: [fn(Settings) -> bool]) -> bool { fs[0](mk()) }",
        "let r = run([|s| s.title.is_some()]);",
    ),
    (
        "closure returned by a closure argument",
        "fn outer(f: fn(number) -> fn(Settings) -> bool) -> bool { f(1)(mk()) }",
        "let r = outer(|n| |s| s.title.is_some());",
    ),
    (
        "closure returned by a closure argument over a typed parameter",
        "fn call(f: fn(Settings) -> fn(Settings) -> bool) -> bool { f(mk())(mk()) }",
        "let r = call(|a| |s| s.title.is_some());",
    ),
    (
        "closure returned by a fn tail closure",
        "fn mkf() -> fn() -> fn(Settings) -> bool { || |s| s.title.is_some() }",
        "",
    ),
    (
        "Some under an annotated let",
        "",
        "let o: Option<fn(Settings) -> bool> = Some(|s| s.title.is_some());",
    ),
    (
        "Ok under an annotated let",
        "",
        "let r: Result<fn(Settings) -> bool, string> = Ok(|s| s.title.is_some());",
    ),
    (
        "generic struct literal field bound by a sibling",
        "struct W<T> { f: fn(T) -> bool, x: T }",
        "let w = W { f: |s| s.title.is_some(), x: mk() };",
    ),
    (
        "generic struct variant literal",
        "enum E<T> { V { f: fn(T) -> bool, x: T } }",
        "let e = E::V { f: |s| s.title.is_some(), x: mk() };",
    ),
    (
        "const closure",
        "const F: fn(Settings) -> bool = |s| s.title.is_some();",
        "",
    ),
    (
        "const array of closures",
        "const L: [fn(Settings) -> bool] = [|s| s.title.is_some()];",
        "",
    ),
    (
        "generic literal fixed by the parameter type",
        "struct W<T> { f: fn(T) -> bool }\nfn take(w: W<Settings>) -> bool { let g = w.f; g(mk()) }",
        "let r = take(W { f: |s| s.title.is_some() });",
    ),
    (
        "generic literal fixed by an annotated let",
        "struct W<T> { f: fn(T) -> bool }",
        "let w: W<Settings> = W { f: |s| s.title.is_some() };",
    ),
    (
        "generic literal fixed by the return type",
        "struct W<T> { f: fn(T) -> bool }\nfn mkw() -> W<Settings> { W { f: |s| s.title.is_some() } }",
        "",
    ),
    (
        "generic tuple variant fixed by the parameter type",
        "enum E<T> { A(fn(T) -> bool) }\nfn run(e: E<Settings>) {}",
        "run(E::A(|s| s.title.is_some()));",
    ),
    (
        "type parameter bound by a later argument",
        "fn both<T>(f: fn(T) -> bool, g: fn() -> T) -> bool { f(g()) }",
        "let r = both(|s| s.title.is_some(), || mk());",
    ),
    (
        "type parameter bound by the expected result",
        "fn wrap<T>(f: fn(T) -> bool) -> fn(T) -> bool { f }",
        "let g: fn(Settings) -> bool = wrap(|s| s.title.is_some());",
    ),
    (
        "method under an expected result type",
        "struct Tool;\nimpl Tool { fn pred<U>(self, f: fn(U) -> bool) -> fn(U) -> bool { f } }",
        "let t = Tool; let p: fn(Settings) -> bool = t.pred(|s| s.title.is_some());",
    ),
    (
        "method call as a typed argument",
        "struct Tool;\nimpl Tool { fn pred<U>(self, f: fn(U) -> bool) -> fn(U) -> bool { f } }\nfn run(p: fn(Settings) -> bool) -> bool { p(mk()) }",
        "let t = Tool; let r = run(t.pred(|s| s.title.is_some()));",
    ),
    (
        "try under an expected type",
        "fn mr<T>(f: fn(T) -> bool) -> Result<fn(T) -> bool, string> { Ok(f) }\nfn g() -> Result<bool, string> { let p: fn(Settings) -> bool = mr(|s| s.title.is_some())?; Ok(p(mk())) }",
        "",
    ),
    (
        "bound by an earlier closure",
        "fn both<T>(g: fn() -> T, f: fn(T) -> bool) -> bool { f(g()) }",
        "let r = both(|| mk(), |s| s.title.is_some());",
    ),
];

#[test]
fn closure_sites_wrap_the_host_read() {
    let mut bad = Vec::new();
    for (name, items, body) in CLOSURE_SITES {
        let out = build(items, body);
        if !out.ok() {
            bad.push(format!("{name}: rejected {:#?}", out.diagnostics));
        } else if !out.output.contains("__option_from_host(s.title)") {
            bad.push(format!("{name}: read not wrapped\n{}", out.output));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n---\n"));
}

/// An extern `fn` that takes an `Option` is handed to the host raw only through
/// a slot the signature types concretely; a type-parameter slot returns it to
/// neoc, which would then call it unmarshalled.
#[test]
fn host_fn_value_through_a_generic_host_slot_is_rejected() {
    for body in [
        "let f = hid(take); f(Some(\"a\"));",
        "let c = Cell { v: take }; let g = c.v; g(Some(\"a\"));",
        "let c: Cell<fn(Option<string>)> = Cell { v: take };",
    ] {
        let src = format!(
            "extern {{ fn take(t: Option<string>); fn hid<T>(x: T) -> T; struct Cell<T> {{ v: T }} }}\npub fn main() {{ {body} }}\n"
        );
        let out = transpile(&src, Target::Lua);
        assert!(!out.ok(), "{body}: accepted\n{}", out.output);
    }
}

/// A value typed `Unknown` is already host-shaped: it reaches an `Option`
/// parameter raw rather than through `__option_to_host`.
#[test]
fn unknown_value_reaches_an_option_parameter_raw() {
    let src = "extern { fn take(t: Option<string>); fn dyn_s() -> Unknown; }\npub fn main() { let u: Unknown = dyn_s(); take(u); }\n";
    let out = transpile(src, Target::Lua);
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert_eq!(wraps(&out.output, TO), 0, "{}", out.output);
}

/// A callee's type parameters are not the enclosing call's, with or without an
/// extern in sight.
#[test]
fn inner_call_type_parameters_do_not_leak_from_the_outer_call() {
    let out = build(
        "fn outer<T>(a: T, k: number) -> number { k }\nfn inner<T>(f: fn(T) -> number) -> number { 7 }",
        "let r = outer(\"s\", inner(|x| x * 2));",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
}

/// `return` inside a closure is not the enclosing fn's `return`.
#[test]
fn closure_return_is_not_hinted_from_the_enclosing_fn() {
    let out = build(
        "struct N { title: Option<string> }\nfn outer() -> fn(Settings) -> bool {\n    let g = |x: number| { if x > 0 { return |d| d.title.is_none(); } |e| false };\n    |d| d.title.is_some()\n}",
        "",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert_eq!(
        out.output.matches("__option_from_host(d.title)").count(),
        1,
        "{}",
        out.output
    );
}

const NAMED: &str = "trait Named { fn name(self) -> string; }\nstruct Dog { n: string }\nimpl Named for Dog { fn name(self) -> string { self.n } }\nstruct Wrap<T> { v: T }\nimpl<T> Wrap<T> {\n    fn map<U>(self, f: fn(T) -> U) -> Wrap<U> { Wrap { v: f(self.v) } }\n    fn fold<U>(self, init: U, f: fn(T, U) -> U) -> U { f(self.v, init) }\n}\n";

/// A method's own type parameter is not the caller's of the same name.
#[test]
fn method_type_parameters_do_not_capture_the_callers() {
    for (name, body) in [
        (
            "Option::map",
            "fn show<U: Named>(o: Option<U>) -> Option<string> { o.map(|x| x.name()) }",
        ),
        (
            "argument path",
            "fn names<U: Named>(w: Wrap<U>) -> string { w.fold(\"hi \", |x, acc| x.name()) }",
        ),
    ] {
        let out = build(&format!("{NAMED}{body}"), "");
        assert!(out.ok(), "{name}: {:#?}", out.diagnostics);
    }
}

#[test]
fn caller_type_parameter_named_like_a_methods_does_not_type_the_closure_as_extern() {
    let out = build(
        "struct N { title: Option<string>, size: number }\nstruct Wrap<T> { v: T }\nimpl<T> Wrap<T> { fn map<U>(self, f: fn(T) -> U) -> Wrap<U> { Wrap { v: f(self.v) } } }\nfn f<U>(w: Wrap<U>, g: fn(U) -> Settings) -> Wrap<Settings> {\n    w.map(|x| { println(x.title.is_some()); g(x) })\n}",
        "",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert_eq!(wraps(&out.output, FROM), 0, "{}", out.output);
}

/// Nested struct literals are checked in time linear in their depth.
#[test]
fn nested_struct_literals_check_in_linear_time() {
    let mut lit = String::from("B { x: None, n: 0 }");
    for _ in 0..30 {
        lit = format!("B {{ x: Some({lit}), n: 1 }}");
    }
    let src =
        format!("struct B {{ x: Option<B>, n: number }}\npub fn main() {{ let b = {lit}; }}\n");
    let start = std::time::Instant::now();
    let out = transpile(&src, Target::Lua);
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// A host-slot argument typed only by the slot's expected type still unwraps.
#[test]
fn host_slot_argument_typed_by_the_slot_unwraps() {
    let out = build(
        "fn via<T>(f: fn(T) -> T, d: Unknown) -> T { f(d) }",
        "let o: Option<number> = Some(3); let r = lookup(\"a\", via(|v| v, o)); let s = mk(); s.title = via(|v| v, Some(\"x\"));",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(wraps(&out.output, TO) >= 2, "{}", out.output);
}

/// Hints are noted, never inferred, ahead of the scope they belong to: a name
/// bound after the hint site must not be read at it.
#[test]
fn hints_do_not_resolve_names_ahead_of_their_scope() {
    let out = build(
        "fn apply(o: Option<Settings>, f: fn(Option<Settings>) -> Option<string>) -> Option<string> { f(o) }\nfn run() -> string { let o = Some(mk()); o.map(|s| s.title).unwrap_or(None).unwrap_or(\"absent\") }",
        "let r = apply(Some(mk()), |o| o.map(|s| s.title).unwrap_or(None));",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(wraps(&out.output, FROM) >= 2, "{}", out.output);
}

#[test]
fn shadowed_names_do_not_change_what_a_closure_is_typed_as() {
    for (name, src) in [
        (
            "method receiver rebound",
            "struct A { n: number }\nimpl A { fn m(self, f: fn(number) -> number) -> number { f(self.n) } }\nstruct S { size: number }\nfn run(a: A) -> number { let f = |x| x; let a = f(1); a.m(|t| t.size) }",
        ),
        (
            "callee rebound",
            "struct S { size: number }\nfn run2() -> fn(S) -> number { let wrap = |f| f; wrap(|s| s.size) }",
        ),
        (
            "closure parameter shadows",
            "struct A { n: number }\nimpl A { fn m(self, f: fn(number) -> number) -> number { f(self.n) } }\nstruct B { s: string }\nimpl B { fn m<U>(self, f: fn(U) -> number) -> number { 0 } }\nfn mk(a: A) -> fn(B) -> number { |a| a.m(|t| t.len) }",
        ),
    ] {
        let out = transpile(&format!("{src}\npub fn main() {{}}\n"), Target::Lua);
        assert!(out.ok(), "{name}: {:#?}", out.diagnostics);
    }
}

#[test]
fn host_fn_does_not_escape_through_a_rebound_callee() {
    let out = transpile(
        "extern { fn take(o: Option<string>); }\nfn run() -> fn(Option<string>) { let store = |f| f; Some(store(take)).unwrap() }\npub fn main() {}\n",
        Target::Lua,
    );
    assert!(!out.ok(), "{}", out.output);
}

/// Nested method calls with closure arguments check in time linear in depth.
#[test]
fn nested_method_calls_with_closures_check_in_linear_time() {
    let mut e = String::from("a");
    for _ in 0..40 {
        e = format!("a.m2({e}.m(|x| x))");
    }
    let src = format!(
        "struct A {{ n: number }}\nimpl A {{ fn m(self, f: fn(number) -> number) -> A {{ self }} fn m2(self, x: A) -> A {{ x }} }}\nfn run(a: A) -> A {{ {e} }}\npub fn main() {{}}\n"
    );
    let start = std::time::Instant::now();
    let out = transpile(&src, Target::Lua);
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// The arguments bind a callee's type parameters; the expected result type
/// only fills what they leave open.
#[test]
fn arguments_bind_type_parameters_before_the_expected_type() {
    let out = build(
        "fn first<T>(x: T, f: fn(T) -> Option<string>) -> T { x }",
        "println(first(mk(), |s| s.title));",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(wraps(&out.output, FROM) >= 1, "{}", out.output);

    for src in [
        "trait Shape { fn area(self) -> number; }\nstruct Circle { r: number }\nimpl Shape for Circle { fn area(self) -> number { self.r } }\nfn app<T>(x: T, f: fn(T) -> number) -> T { f(x); x }\nfn mk() -> Shape { app(Circle { r: 1 }, |c| c.r) }",
        "trait Named { fn name(self) -> string; }\nstruct P {}\nimpl Named for P { fn name(self) -> string { \"p\" } }\nfn via<T: Named>(f: fn(T) -> string, x: T) -> T { x }\nfn run() { let n: Named = via(|p| p.name(), P {}); }",
    ] {
        let out = transpile(&format!("{src}\npub fn main() {{}}\n"), Target::Lua);
        assert!(out.ok(), "{src}\n{:#?}", out.diagnostics);
    }
}

#[test]
fn a_fn_item_inside_a_closure_keeps_its_own_return_hint() {
    let out = build(
        "fn run(f: fn(number) -> number) -> number { f(1) }\nfn mkf<T>(f: fn(T) -> Option<string>) -> fn(T) -> Option<string> { f }",
        "let n = run(|x| { fn inner() -> fn(Settings) -> Option<string> { if true { return mkf(|s| s.title); } mkf(|s: Settings| s.title) } x });",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(wraps(&out.output, FROM) >= 2, "{}", out.output);
}

/// Argument order never changes what a closure parameter is typed as.
#[test]
fn a_later_annotated_closure_types_an_earlier_one() {
    for call in [
        "both(mk(), |s| s.title, |n: Settings| n)",
        "both(mk(), |s| s.title, id(|n: Settings| n))",
    ] {
        let out = build(
            "fn both<T>(seed: Unknown, f: fn(T) -> Option<string>, g: fn(T) -> T) -> Option<string> { f(g(seed)) }",
            &format!("println({call});"),
        );
        assert!(out.ok(), "{call}\n{:#?}", out.diagnostics);
        assert!(wraps(&out.output, FROM) >= 1, "{call}\n{}", out.output);
    }
}

/// A chain of `?` under a type hint stays linear to check.
#[test]
fn nested_try_hints_stay_linear() {
    let n = 400;
    let ty = (0..n).fold("number".to_string(), |t, _| format!("Option<{t}>"));
    let src = format!(
        "fn deep(x: {ty}) -> Option<number> {{ let y: number = x{}; Some(y) }}\npub fn main() {{}}\n",
        "?".repeat(n)
    );
    let start = std::time::Instant::now();
    let _ = transpile(&src, Target::Lua);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// Closure shapes the ordering rule must see through.
#[test]
fn wrapped_annotated_closures_type_earlier_ones() {
    for g in [
        "if true { |n: Settings| n } else { |n: Settings| n }",
        "match 1 { _ => |n: Settings| n }",
        "{ |n: Settings| n }",
    ] {
        let out = build(
            "fn both<T>(seed: Unknown, f: fn(T) -> Option<string>, g: fn(T) -> T) -> Option<string> { f(g(seed)) }",
            &format!("println(both(mk(), {{ |s| s.title }}, {g}));"),
        );
        assert!(out.ok(), "{g}\n{:#?}", out.diagnostics);
        assert!(wraps(&out.output, FROM) >= 1, "{g}\n{}", out.output);
    }
}

/// A closure may not contradict the type the call already fixed.
#[test]
fn a_closure_contradicting_the_call_is_rejected() {
    for body in [
        "println(run(mk(), |s: Other| s.title));",
        "println(run(5, |s: Settings| s.title));",
    ] {
        let out = build(
            "struct Other { title: Option<string> }\nfn run<T>(seed: T, f: fn(T) -> Option<string>) -> Option<string> { f(seed) }",
            body,
        );
        assert!(!out.ok(), "{body}");
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.message.contains("closure parameter")),
            "{body}\n{:#?}",
            out.diagnostics
        );
    }
}

/// `?` nested through blocks stays linear to check.
#[test]
fn try_through_blocks_stays_linear() {
    let n = 40;
    let ty = (0..n).fold("number".to_string(), |t, _| format!("Option<{t}>"));
    let mut body = "mko()?".to_string();
    for _ in 0..n {
        body = format!("{{ {body} }}?");
    }
    let src = format!(
        "extern {{ fn mko() -> {ty}; }}\nfn g() -> Option<number> {{ let x: number = {body}; Some(x) }}\npub fn main() {{}}\n"
    );
    let start = std::time::Instant::now();
    let _ = transpile(&src, Target::Lua);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// A closure parameter no hint could type is rejected where a host `Option`
/// could flow through it, instead of reading the field raw.
#[test]
fn an_untypable_closure_parameter_is_rejected_when_members_are_read() {
    for (items, body) in [
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| s.title.is_some()], || mk()));",
        ),
        (
            "struct B<T> { f: fn(T) -> bool }\nfn run<T>(b: B<T>, g: fn() -> T) -> bool { let h = b.f; h(g()) }",
            "println(run(B { f: |s| s.title.is_some() }, || mk()));",
        ),
        (
            "fn run<T>(f: Option<fn(T) -> bool>, g: fn() -> T) -> bool { match f { Some(h) => h(g()), None => false } }",
            "println(run(Some(|s| s.title.is_some()), || mk()));",
        ),
        (
            "fn mr<T>(f: fn(T) -> bool) -> Option<fn(T) -> bool> { Some(f) }\nfn call(f: fn() -> Option<fn(Settings) -> bool>) -> Option<bool> { let p = f()?; Some(p(mk())) }\nfn g(c: bool) -> Option<bool> { call(|| Some(if c { mr(|s| s.title.is_some()) } else { None }?)) }",
            "println(g(true));",
        ),
        // A sibling closure in the same argument, and the parameter routed
        // through a binding, a `match`, a call, or a block.
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[1](g()) }",
            "println(run([|a| true, |s| s.title.is_some()], || mk()));",
        ),
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| { let t = s; t.title.is_some() }], || mk()));",
        ),
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| match s { t => t.title.is_some() }], || mk()));",
        ),
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| id(s).title.is_some()], || mk()));",
        ),
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| { s }.title.is_some()], || mk()));",
        ),
        // A bare type-parameter slot, a closure value called, a closure
        // returning the parameter.
        ("", "println(id(|s| s.title.is_some())(mk()));"),
        (
            "fn useo(o: Option<fn(Settings) -> bool>) -> bool { match o { Some(h) => h(mk()), None => false } }",
            "let o = Some(|s| s.title.is_some()); println(useo(o));",
        ),
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| { let f = || s; f().title.is_some() }], || mk()));",
        ),
        (
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            "println(run([|s| (|| s)().title.is_some()], || mk()));",
        ),
    ] {
        let out = build(items, body);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.message.contains("closure parameter is not known here")),
            "{body}\n{:#?}\n{}",
            out.diagnostics,
            out.output
        );
    }
    // Annotated, or typed by the binding argument first: accepted and marshalled.
    let out = build(
        "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
        "println(run([|s: Settings| s.title.is_some()], || mk()));",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
}

/// A hint naming the caller's own type parameter is known, not open; and the
/// marker never reaches a diagnostic or blocks plain calls, indexing, or `?`.
#[test]
fn caller_type_parameters_are_known_and_the_marker_stays_quiet() {
    let out = build(
        "trait Named { fn name(self) -> string; }\nstruct Dog { n: string }\nimpl Named for Dog { fn name(self) -> string { self.n } }\nfn apply<T>(x: T, f: fn(T) -> string) -> string { f(x) }\nfn outer<U: Named>(y: U) -> string { apply(y, |v| v.name()) }",
        "println(outer(Dog { n: \"d\" }));",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    for body in [
        "println(run([|s| s()], || mk()));",
        "println(run([|s| s[0]], || mk()));",
    ] {
        let out = build(
            "fn run<T>(fs: [fn(T) -> bool], g: fn() -> T) -> bool { fs[0](g()) }",
            body,
        );
        assert!(
            out.diagnostics.iter().all(|d| !d.message.contains('_')),
            "{body}\n{:#?}",
            out.diagnostics
        );
    }
}

/// The marker behaves as an unknown value everywhere but a member read: bounds,
/// `for`, host arguments, operators, and display; and it follows calls, index,
/// and `?`.
#[test]
fn the_marker_acts_unknown_except_for_member_reads() {
    let items = "trait Shout { fn shout(self) -> string; }\nfn show<T: Shout>(x: T) -> number { 1 }\nfn hold<T, U>(f: fn(T) -> U) -> number { 1 }";
    for body in [
        "let r = hold(|s| show(s));",
        "let r = hold(|s| { for x in s { } 1 });",
        "let r = hold(|s| lookup(\"a\", s));",
        "let r = hold(|s| s + 1);",
    ] {
        let out = build(items, body);
        assert!(out.ok(), "{body}\n{:#?}", out.diagnostics);
        assert!(
            !out.output.contains("__option_to_host(s)"),
            "{body}\n{}",
            out.output
        );
    }
    for body in [
        "let r = hold(|s| s[0].title);",
        "let r = hold(|s| s().title);",
        "let r = hold(|s| { let q = s?; q.title });",
    ] {
        let out = build(items, body);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.message.contains("closure parameter is not known here")),
            "{body}\n{:#?}",
            out.diagnostics
        );
    }
}

/// Closures reached through an index, or in a bare type-parameter field whose
/// argument the call fixes, are hinted.
#[test]
fn indexed_and_bare_field_closures_are_hinted() {
    let out = build(
        "fn run(f: fn(Settings) -> Option<string>) -> Option<string> { f(mk()) }",
        "let r = run((if true { [|s| s.title] } else { [|s| s.title] })[0]);",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(wraps(&out.output, FROM) >= 1, "{}", out.output);
    let out = build(
        "struct W<A> { f: A }\nfn take(w: W<fn(Settings) -> Option<string>>) { }",
        "take(W { f: |s| s.title });",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(wraps(&out.output, FROM) >= 1, "{}", out.output);
}

/// Parts of an unresolved value are unresolved: destructuring or iterating it
/// does not launder it into a readable one.
#[test]
fn destructured_and_iterated_markers_stay_unresolved() {
    for (items, body) in [
        (
            "fn run<T>(fs: [fn(Option<T>) -> bool], g: fn() -> T) -> bool { fs[0](Some(g())) }",
            "run([|s| match s { Some(x) => x.title.is_some(), None => false }], || mk());",
        ),
        (
            "fn run<T>(fs: [fn(Option<T>) -> bool], g: fn() -> T) -> bool { fs[0](Some(g())) }",
            "run([|s| match s { Some(x) => { let y = x; y.title.is_some() }, None => false }], || mk());",
        ),
        (
            "fn run<T>(fs: [fn([T]) -> bool], g: fn() -> T) -> bool { fs[0]([g()]) }",
            "run([|xs| { for x in xs { x.title.is_some(); } true }], || mk());",
        ),
    ] {
        let out = build(items, body);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.message.contains("closure parameter is not known here")),
            "{body}\n{:#?}",
            out.diagnostics
        );
    }
}

/// `?` and indexing nested inside a closure stay linear to check.
#[test]
fn try_and_index_in_closures_stay_linear() {
    let n = 30;
    let mut e = "|s| true".to_string();
    for _ in 0..n {
        e = format!("Some([{e}])?[0]");
    }
    let src = format!(
        "extern {{ struct Settings {{ title: Option<string> }} }}\nfn ap(f: fn(Settings) -> bool) -> bool {{ false }}\nfn f() -> Option<bool> {{ let g = |x: number| ap({e}); None }}\nfn main() {{}}\n"
    );
    let start = std::time::Instant::now();
    let _ = transpile(&src, Target::Lua);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// The caller's own type parameters are known to a closure, and `_` as a
/// written type parameter is not the marker.
#[test]
fn own_type_parameters_and_written_underscore_are_not_the_marker() {
    let out = build(
        "trait Vis { fn vis(self, f: fn(Self) -> bool) -> bool; fn x(self) -> number; }\nstruct P { x: number }\nimpl Vis for P { fn vis(self, f: fn(Self) -> bool) -> bool { f(self) } fn x(self) -> number { 1 } }\nfn dv<V: Vis>(v: V) -> bool { v.vis(|s| s.x() > 0) }",
        "dv(P { x: 1 });",
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    let out = build(
        "extern { fn show(v: Option<string>); }\nfn idu<_>(x: _) -> _ { x }",
        "show(idu(Some(\"a\")));",
    );
    assert!(
        !out.ok() || wraps(&out.output, TO) >= 1,
        "{:#?}\n{}",
        out.diagnostics,
        out.output
    );
}

/// A callee type parameter is open even when the caller reuses its name.
#[test]
fn callee_and_caller_sharing_a_type_parameter_name() {
    // No host `Option` in the module: develop's behaviour is unchanged.
    let out = transpile(
        "extern { fn println(v: Unknown); }\nstruct P { x: number }\nfn call<T>(f: fn(T) -> number, x: Unknown) -> number { f(x) }\nfn g<T>(t: T) -> number { call(|s| s.x, P { x: 1 }) }\npub fn main() { println(g(1)); }\n",
        Target::Lua,
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    // The leak shape under the same collision: a bounded caller `T`.
    let out = build(
        "trait HasT { fn title(self) -> Option<string>; }\nfn call<T>(f: fn(T) -> bool, x: Unknown) -> bool { true }\nfn g<T: HasT>(t: T) -> bool { call(|s| s.title().is_some(), mk()) }",
        "println(1);",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.message.contains("closure parameter is not known here")),
        "{:#?}",
        out.diagnostics
    );
}
