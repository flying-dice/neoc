//! `extern { }` lowering end-to-end (`lang/EXTERN.md`). A `mod` imports its
//! target handle; an extern `struct` literal is a bare target object; an
//! `#[extern(constructor)]` call constructs; field/method access lowers
//! directly to the target value (no wrapper). `ambient` emits no import.
//!
//! These are emit-level assertions: the programs reference external modules, so
//! they are not run.

use neoc_compiler::{Target, transpile};

/// An extern `mod` with a target `path`, referenced as `mod::item`.
const MOD_SOURCE: &str = r#"
extern {
    #[extern(path = "vnd:rusqlite")]
    mod rusqlite {
        fn open(path: string) -> Unknown;
    }
}
fn main() { let _ = rusqlite::open(":memory:"); }
"#;

#[test]
fn lua_mod_imports_via_require_and_reads_member() {
    let out = transpile(MOD_SOURCE, Target::Luau).output;
    assert!(
        out.contains("local rusqlite = require(\"vnd:rusqlite\")"),
        "missing Lua require for the mod path:\n{out}"
    );
    assert!(
        out.contains("rusqlite.open(\":memory:\")"),
        "mod member call should read the imported handle:\n{out}"
    );
}

#[test]
fn js_mod_imports_namespace_and_reads_member() {
    let out = transpile(MOD_SOURCE, Target::Ts).output;
    assert!(
        out.contains("import * as rusqlite from \"vnd:rusqlite\";"),
        "missing JS namespace import for the mod path:\n{out}"
    );
    assert!(
        out.contains("rusqlite.open(\":memory:\")"),
        "mod member call should read the imported handle:\n{out}"
    );
}

/// An extern `struct` with fields and an `#[extern(constructor)]` impl.
const STRUCT_SOURCE: &str = r#"
extern {
    struct Changes { changes: number }
    struct Database;
    impl Database {
        #[extern(constructor)] fn new(filename: string);
        fn run(self, sql: string) -> Changes;
    }
}
fn main() {
    let db = Database::new(":memory:");
    let c = db.run("VACUUM");
    let n = c.changes;
    let _ = Changes { changes: n };
}
"#;

#[test]
fn js_extern_struct_literal_is_bare_object_and_constructor_uses_new() {
    let out = transpile(STRUCT_SOURCE, Target::Ts).output;
    // Constructor → `new T(args)`.
    assert!(
        out.contains("new Database(\":memory:\")"),
        "constructor should lower to `new`:\n{out}"
    );
    // A method call on an extern-struct receiver lowers to a native call — the
    // value carries no neoc tag, so registry dispatch is pure noise.
    assert!(
        out.contains("db.run(\"VACUUM\")"),
        "method call on an extern receiver should be native:\n{out}"
    );
    assert!(
        !out.contains("__call"),
        "no `__call` for a wholly-foreign module:\n{out}"
    );
    // Field read is direct.
    assert!(out.contains("c.changes"), "field read changed:\n{out}");
    // Extern struct literal is a bare object — no `__struct` tag.
    assert!(
        out.contains("{ changes: n }") && !out.contains("__struct: \"Changes\""),
        "extern struct literal should be a bare object:\n{out}"
    );
}

#[test]
fn lua_extern_struct_literal_is_bare_table_and_constructor_is_static() {
    let out = transpile(STRUCT_SOURCE, Target::Luau).output;
    // Lua has no `new` operator: a constructor is a static/namespaced call.
    assert!(
        out.contains("Database.new(\":memory:\")"),
        "constructor should be a static call:\n{out}"
    );
    // Method call uses the colon-call (runtime UserData convention).
    assert!(
        out.contains("db:run(\"VACUUM\")"),
        "method call should be a colon call:\n{out}"
    );
    assert!(out.contains("c.changes"), "field read changed:\n{out}");
    // Extern struct literal is a bare table — no `.new`, no `__struct`.
    assert!(
        out.contains("{ changes = n }") && !out.contains("Changes.new"),
        "extern struct literal should be a bare table:\n{out}"
    );
}

/// An extern `struct` declared *inside* a path `mod`: its value (constructor,
/// the constructed identity) is reached through the `mod`'s import, not a bare
/// name (`lang/EXTERN.md` §Lowering). Free functions already qualify; this pins
/// that struct values do too.
const MOD_STRUCT_SOURCE: &str = r#"
extern {
    #[extern(path = "bun:sqlite")]
    mod sqlite {
        struct Changes { changes: number }
        struct Database;
        impl Database {
            #[extern(constructor)] fn new(filename: string);
            fn run(self, sql: string) -> Changes;
        }
    }
}
fn main() {
    let db = Database::new(":memory:");
    let c = db.run("VACUUM");
    let n = c.changes;
    let _ = Changes { changes: n };
}
"#;

#[test]
fn js_extern_struct_in_mod_constructs_through_namespace_import() {
    let out = transpile(MOD_STRUCT_SOURCE, Target::Ts).output;
    // The constructor qualifies through the imported namespace; only `sqlite`
    // is bound, so a bare `new Database(...)` would not resolve.
    assert!(
        out.contains("new sqlite.Database(\":memory:\")"),
        "in-mod constructor should qualify through the namespace import:\n{out}"
    );
    assert!(
        !out.contains("new Database("),
        "bare `new Database(...)` is unbound — only `sqlite` is imported:\n{out}"
    );
    // Instance method on an extern receiver lowers to a native call.
    assert!(
        out.contains("db.run(\"VACUUM\")"),
        "method call on an extern receiver should be native:\n{out}"
    );
    assert!(
        !out.contains("__call"),
        "no `__call` for a wholly-foreign module:\n{out}"
    );
    // The struct *literal* stays a bare object — no qualifier, no tag.
    assert!(
        out.contains("{ changes: n }") && !out.contains("__struct: \"Changes\""),
        "extern struct literal should stay a bare object:\n{out}"
    );
}

#[test]
fn lua_extern_struct_in_mod_constructs_through_require_binding() {
    let out = transpile(MOD_STRUCT_SOURCE, Target::Luau).output;
    // The constructor qualifies through the `require` binding; only `sqlite` is
    // bound, so a bare `Database.new(...)` would read a nil global.
    assert!(
        out.contains("sqlite.Database.new(\":memory:\")"),
        "in-mod constructor should qualify through the require binding:\n{out}"
    );
    assert!(
        !out.contains("\nlocal db = Database.new("),
        "bare `Database.new(...)` reads a nil global — only `sqlite` is bound:\n{out}"
    );
    assert!(
        out.contains("db:run(\"VACUUM\")"),
        "method call should be a colon call:\n{out}"
    );
    // The struct *literal* stays a bare table.
    assert!(
        out.contains("{ changes = n }") && !out.contains("Changes.new"),
        "extern struct literal should stay a bare table:\n{out}"
    );
}

#[test]
fn mod_struct_block_checks_clean() {
    for target in [Target::Ts, Target::Luau] {
        let out = transpile(MOD_STRUCT_SOURCE, target);
        assert!(
            out.ok(),
            "{target} slice should check clean: {:#?}",
            out.diagnostics
        );
    }
}

#[test]
fn extern_block_checks_clean() {
    for target in [Target::Ts, Target::Luau] {
        let out = transpile(STRUCT_SOURCE, target);
        assert!(
            out.ok(),
            "{target} slice should check clean: {:#?}",
            out.diagnostics
        );
    }
}

#[test]
fn ambient_mod_emits_no_import_in_either_target() {
    // An ambient mod is reached through the target global; no import emitted.
    let src = r#"
extern {
    #[extern(ambient)]
    mod console { fn log(s: string); }
}
fn main() { console::log("hi"); }
"#;
    for target in [Target::Ts, Target::Luau] {
        let out = transpile(src, target).output;
        assert!(
            !out.contains("import") && !out.contains("require("),
            "ambient mod should emit no import for {target}:\n{out}"
        );
        assert!(
            out.contains("console.log(\"hi\")"),
            "ambient mod member access changed for {target}:\n{out}"
        );
    }
}

#[test]
fn top_level_extern_fn_is_a_bare_global() {
    // A top-level extern `fn` (not in a mod) is a host global: no import, bare
    // call.
    let src = "extern { fn print(s: string); }\nfn main() { print(\"hi\"); }\n";
    for target in [Target::Ts, Target::Luau] {
        let out = transpile(src, target).output;
        assert!(
            !out.contains("import") && !out.contains("require("),
            "top-level extern fn should emit no import for {target}:\n{out}"
        );
        assert!(
            out.contains("print(\"hi\")"),
            "extern fn call changed for {target}:\n{out}"
        );
    }
}

/// A free extern `fn` declared *inside* a `mod` is reached through the `mod`'s
/// binding, not a bare name (`lang/EXTERN.md` §Lowering): a call `serve(opts)`
/// to a `fn serve` in an ambient `mod Bun` lowers to `Bun.serve(opts)`. Mirrors
/// how an in-mod extern struct's constructor qualifies through its namespace.
/// The fn name flattens to the file level (the `mod` is a target-path scope, not
/// a neoc namespace), so it is *called* bare and qualified at emit.
const MOD_FN_SOURCE: &str = r#"
extern {
    #[extern(ambient, path = "Bun")]
    mod Bun {
        fn serve(options: Unknown) -> Unknown;
    }
}
fn main() { let _ = serve(0); }
"#;

#[test]
fn js_ambient_mod_fn_called_bare_qualifies_through_its_namespace() {
    let out = transpile(MOD_FN_SOURCE, Target::Ts).output;
    // No import for an ambient mod; the call qualifies through the global root.
    assert!(
        !out.contains("import"),
        "ambient mod emits no import:\n{out}"
    );
    assert!(
        out.contains("Bun.serve(0)"),
        "in-mod fn called bare should qualify through its namespace:\n{out}"
    );
}

#[test]
fn lua_ambient_mod_fn_called_bare_qualifies_through_its_namespace() {
    let out = transpile(MOD_FN_SOURCE, Target::Luau).output;
    assert!(
        !out.contains("require("),
        "ambient mod emits no require:\n{out}"
    );
    assert!(
        out.contains("Bun.serve(0)"),
        "in-mod fn called bare should qualify through its namespace:\n{out}"
    );
}

/// A method call on a dynamic `Unknown` receiver lowers to a native JS call: the
/// foreign value carries no neoc tag, so registry dispatch is noise. A *chained*
/// foreign call — `a.b(x).c()`, where `b` returns `Unknown` — is native at both
/// links.
#[test]
fn js_method_call_on_unknown_receiver_is_native_and_chains() {
    let src = r#"
extern {
    fn fetch(url: string) -> Unknown;
}
fn main() {
    let res = fetch("/x");
    let _ = res.json().rows;
}
"#;
    let out = transpile(src, Target::Ts).output;
    assert!(
        out.contains("res.json()"),
        "method call on an `Unknown` receiver should be native:\n{out}"
    );
    assert!(
        !out.contains("__call"),
        "no `__call` should remain for a foreign-only program:\n{out}"
    );
    assert!(
        !out.contains("__m"),
        "no `__m` registry for a program with no neoc structs:\n{out}"
    );
}

/// A method call on a *neoc* `struct` is a native property call resolving up the
/// value's prototype chain — uniform with a foreign receiver. The struct's
/// prototype is `Counter$`, its method lands there, and the constructed value
/// links to it via `__proto__` (`lang/DYNAMIC.md` §3.1, §3.3, §4.7).
#[test]
fn js_method_call_on_neoc_struct_is_native_prototype_dispatch() {
    let src = r"
struct Counter { n: number }
impl Counter {
    fn bump(self) -> number { self.n + 1 }
}
fn main() {
    let c = Counter { n: 0 };
    let _ = c.bump();
}
";
    let out = transpile(src, Target::Ts).output;
    assert!(
        out.contains("c.bump()") && !out.contains("__call") && !out.contains("__m"),
        "method call on a neoc struct should be native prototype dispatch:\n{out}"
    );
    // The TS target types the prototype `Record<string, Function>` and the
    // method's implicit `this`, so the body type-checks against the interface
    // (the runtime lowering is identical to JS — see the codegen-js JS goldens).
    assert!(
        out.contains("const Counter$: Record<string, Function> = {};")
            && out.contains("Counter$.bump = function(this: Counter) {"),
        "the struct should get a typed prototype with its method:\n{out}"
    );
    // The inherent method is carried on the struct interface so `c.bump()`
    // type-checks on the widened value.
    assert!(
        out.contains("interface Counter { n: number; bump(): number; }"),
        "the interface should carry the inherent method:\n{out}"
    );
    assert!(
        out.contains("__proto__: Counter$"),
        "the constructed value should link to its prototype:\n{out}"
    );
}
