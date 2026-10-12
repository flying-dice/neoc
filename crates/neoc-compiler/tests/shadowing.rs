//! `lang/DYNAMIC.md` §3.7 / `STATIC.md` §2: no value binding can capture a
//! type's runtime table. The tests cover the invariant, not single repros: every
//! way a program reaches a type's table (struct literal, associated call,
//! derive, enum method, trait default) against every way the value namespace
//! binds the same name (`let`, parameter, closure parameter, `for` variable,
//! `match` binding, module `const` / `fn`, nested `fn` parameter, `impl`
//! method parameter). Each program must compile clean and run to the same value.

mod common;

use std::fmt::Write as _;

/// A type named `T` and an expression `E` that evaluates to `1` by going
/// through `T`'s runtime table.
struct Kind {
    name: &'static str,
    decl: &'static str,
    expr: &'static str,
}

const KINDS: &[Kind] = &[
    Kind {
        name: "struct literal",
        decl: "struct T { v: number }",
        expr: "(T { v: 1 }).v",
    },
    Kind {
        name: "struct associated fn",
        decl: "struct T { v: number }\nimpl T { fn mk() -> number { 1 } }",
        expr: "T::mk()",
    },
    Kind {
        name: "derive Clone",
        decl: "#[derive(Clone)]\nstruct T { v: number }",
        expr: "(T { v: 1 }).clone().v",
    },
    Kind {
        name: "struct update",
        decl: "struct T { v: number, w: number }",
        expr: "(T { v: 1, ..T { v: 0, w: 2 } }).v",
    },
    Kind {
        name: "enum method",
        decl: "enum T { A }\nimpl T { fn code(self) -> number { 1 } }",
        expr: "T::A.code()",
    },
    Kind {
        name: "trait default",
        decl: "trait T { fn d(self) -> number { 1 } }\nstruct S { v: number }\nimpl T for S {}",
        expr: "(S { v: 0 }).d()",
    },
];

/// A way to bind the value `T = 7` around `E`, as the module's items.
struct Binder {
    name: &'static str,
    /// The module items after the type declaration. `{E}` is the expression.
    items: &'static str,
}

const BINDERS: &[Binder] = &[
    Binder {
        name: "let",
        items: "fn probe() -> number { let T = 7; {E} + T }",
    },
    Binder {
        name: "parameter",
        items: "fn probe() -> number { go(7) }\nfn go(T: number) -> number { {E} + T }",
    },
    Binder {
        name: "closure parameter",
        items: "fn probe() -> number { let f = |T: number| {E} + T; f(7) }",
    },
    Binder {
        name: "for variable",
        items: "fn probe() -> number { let acc = 0; for T in 7..8 { acc = acc + {E} + T; } acc }",
    },
    Binder {
        name: "match binding",
        items: "fn probe() -> number { match Some(7) { Some(T) => {E} + T, None => 0 } }",
    },
    Binder {
        name: "nested fn parameter",
        items: "fn probe() -> number { fn inner(T: number) -> number { {E} + T } inner(7) }",
    },
    Binder {
        name: "impl method parameter",
        items: "struct H { v: number }\nimpl H { fn go(T: number) -> number { {E} + T } }\n\
                fn probe() -> number { H::go(7) }",
    },
    Binder {
        name: "module const",
        items: "const T: number = 7;\nfn probe() -> number { {E} + T }",
    },
    Binder {
        name: "module fn",
        items: "fn T() -> number { 7 }\nfn probe() -> number { {E} + T() }",
    },
];

fn run(source: &str) -> Result<String, String> {
    let out = neoc_compiler::transpile(source, neoc_compiler::Target::Lua);
    if !out.ok() {
        return Err(format!("diagnostics: {:#?}", out.diagnostics));
    }
    let program = format!("{}\nmain()\n", out.output);
    common::run_lua(&program).map_err(|e| format!("lua failed:\n{e}\n--- program ---\n{program}"))
}

#[test]
fn no_value_binding_captures_a_type_table() {
    let mut ran = 0;
    let mut failures = String::new();
    for kind in KINDS {
        for binder in BINDERS {
            let items = binder.items.replace("{E}", kind.expr);
            let source = format!(
                "extern {{ fn print(n: number); }}\n{}\n{items}\nfn main() {{ print(probe()); }}\n",
                kind.decl
            );
            match run(&source) {
                Ok(out) if out.trim() == "8" => ran += 1,
                Ok(out) => {
                    let _ = writeln!(
                        failures,
                        "{} x {}: printed {:?}\n{source}",
                        kind.name,
                        binder.name,
                        out.trim()
                    );
                }
                Err(e) => {
                    let _ = writeln!(failures, "{} x {}: {e}\n{source}", kind.name, binder.name);
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures}");
    assert_eq!(ran, KINDS.len() * BINDERS.len());
}

/// A fresh local never collides with a name the user wrote.
#[test]
fn fresh_type_local_avoids_user_identifiers() {
    let src = "extern { fn print(n: number); }\n\
               struct T { v: number }\n\
               fn main() { let T = 7; let __type_T = 3; let t = T { v: T }; print(t.v + __type_T); }\n";
    assert_eq!(run(src).expect("runs").trim(), "10");
}

/// Two captured types get distinct fresh locals.
#[test]
fn two_captured_local_types_get_distinct_locals() {
    let src = "extern { fn print(n: number); }\n\
               struct A { v: number }\nstruct B { v: number }\n\
               fn main() { let A = 1; let B = 2; let a = A { v: A }; let b = B { v: B }; \
               print(a.v * 10 + b.v); }\n";
    assert_eq!(run(src).expect("runs").trim(), "12");
}

/// A type not captured by any value keeps its plain name.
#[test]
fn uncaptured_type_keeps_its_name() {
    let out = neoc_compiler::transpile(
        "struct T { v: number }\nfn main() { let t = T { v: 1 }; }\n",
        neoc_compiler::Target::Lua,
    );
    assert!(out.ok(), "{:#?}", out.diagnostics);
    assert!(out.output.contains("local T = {}"), "{}", out.output);
    assert!(!out.output.contains("__type_T"), "{}", out.output);
}
