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
