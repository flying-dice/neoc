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
    let out = transpile(MOD_SOURCE, Target::Lua).output;
    assert!(
        out.contains("local rusqlite = require(\"vnd:rusqlite\")"),
        "missing Lua require for the mod path:\n{out}"
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
fn lua_extern_struct_literal_is_bare_table_and_constructor_is_static() {
    let out = transpile(STRUCT_SOURCE, Target::Lua).output;
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
fn lua_extern_struct_in_mod_constructs_through_require_binding() {
    let out = transpile(MOD_STRUCT_SOURCE, Target::Lua).output;
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
    let out = transpile(MOD_STRUCT_SOURCE, Target::Lua);
    assert!(out.ok(), "slice should check clean: {:#?}", out.diagnostics);
}

#[test]
fn extern_block_checks_clean() {
    let out = transpile(STRUCT_SOURCE, Target::Lua);
    assert!(out.ok(), "slice should check clean: {:#?}", out.diagnostics);
}

#[test]
fn ambient_mod_emits_no_import() {
    // An ambient mod is reached through the target global; no import emitted.
    let src = r#"
extern {
    #[extern(ambient)]
    mod console { fn log(s: string); }
}
fn main() { console::log("hi"); }
"#;
    let out = transpile(src, Target::Lua).output;
    assert!(
        !out.contains("require("),
        "ambient mod should emit no import:\n{out}"
    );
    assert!(
        out.contains("console.log(\"hi\")"),
        "ambient mod member access changed:\n{out}"
    );
}

#[test]
fn top_level_extern_fn_is_a_bare_global() {
    // A top-level extern `fn` (not in a mod) is a host global: no import, bare
    // call.
    let src = "extern { fn print(s: string); }\nfn main() { print(\"hi\"); }\n";
    let out = transpile(src, Target::Lua).output;
    assert!(
        !out.contains("require("),
        "top-level extern fn should emit no import:\n{out}"
    );
    assert!(
        out.contains("print(\"hi\")"),
        "extern fn call changed:\n{out}"
    );
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
fn lua_ambient_mod_fn_called_bare_qualifies_through_its_namespace() {
    let out = transpile(MOD_FN_SOURCE, Target::Lua).output;
    assert!(
        !out.contains("require("),
        "ambient mod emits no require:\n{out}"
    );
    assert!(
        out.contains("Bun.serve(0)"),
        "in-mod fn called bare should qualify through its namespace:\n{out}"
    );
}

// ---- Option marshalling (lang/EXTERN.md §Option marshalling) ----------------

const OPTION_SOURCE: &str = r#"
extern {
    #[extern(path = "std:collections")]
    mod collections {
        struct Counter;
        impl Counter {
            #[extern(constructor)]
            fn new(initial: Option<number>);
            fn find(self, key: string) -> Option<string>;
        }
    }
    struct Settings { title: Option<string> }
}
fn main(s: Settings) {
    let c = Counter::new(Some(5));
    let hit = c.find("a");
    s.title = None;
    let lit = Settings { title: hit };
    let read = s.title;
}
"#;

#[test]
fn option_marshals_through_constructor_method_and_fields() {
    let out = transpile(OPTION_SOURCE, Target::Lua);
    assert!(out.ok(), "should check clean: {:#?}", out.diagnostics);
    let out = out.output;
    for needle in [
        "collections.Counter.new(__option_to_host(Some(5)))",
        "__option_from_host(c:find(\"a\"))",
        "s.title = __option_to_host(None)",
        "{ title = __option_to_host(hit) }",
        "__option_from_host(s.title)",
    ] {
        assert!(out.contains(needle), "missing `{needle}`:\n{out}");
    }
}

#[test]
fn neoc_option_function_emits_no_marshalling() {
    let out = transpile(
        "fn keep(v: Option<number>) -> Option<number> { v }\nfn main() { let _ = keep(Some(1)); }",
        Target::Lua,
    )
    .output;
    assert!(
        !out.contains("__option_"),
        "pure neoc call marshalled:\n{out}"
    );
}

#[test]
fn extern_fn_passed_as_value_marshals_when_called() {
    let src = r"
extern {
    #[extern(ambient)]
    mod host { fn put(v: Option<number>) -> Option<number>; }
}
fn main() { let f = put; let _ = f(None); }
";
    let out = transpile(src, Target::Lua).output;
    assert!(
        out.contains(
            "function(__a0) return __option_from_host(host.put(__option_to_host(__a0))) end"
        ),
        "extern fn value should wrap in a marshalling closure:\n{out}"
    );
}
