//! Cross-module visibility primitives (`STATIC.md` §2).
//!
//! [`collect`] records which names a module declares `pub`; [`import_from`]
//! copies a named symbol from one module's table into another; and
//! [`check_with_table`] checks a module's bodies against a table augmented with
//! such imports. These back the multi-file driver in `neoc-compiler`.

use neoc_types::{check_with_table, collect};

fn module(src: &str) -> neoc_ast::Module {
    let parsed = neoc_parser::parse(src);
    assert!(
        parsed.diagnostics.is_empty(),
        "parser rejected the test program: {:?}",
        parsed.diagnostics
    );
    parsed.module
}

#[test]
fn collect_records_pub_items_and_omits_private_ones() {
    let m = module(
        "pub fn add(a: number, b: number) -> number { a + b }\n\
         fn secret(x: number) -> number { x }\n\
         pub struct Point { x: number, y: number }\n\
         struct Hidden { v: number }\n",
    );
    let table = collect(&m);

    assert!(table.value_is_public("add"));
    assert!(!table.value_is_public("secret"));
    assert!(table.type_is_public("Point"));
    assert!(!table.type_is_public("Hidden"));
}

#[test]
fn import_from_brings_a_pub_fn_into_scope() {
    let util = collect(&module(
        "pub fn add(a: number, b: number) -> number { a + b }\n",
    ));
    let main = module("fn main() -> number { add(2, 3) }\n");

    // Without the import, `add` is unknown.
    let mut table = collect(&main);
    assert!(
        check_with_table(&main, &table)
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unknown name `add`")),
        "expected `add` to be unresolved before import"
    );

    // After importing the `pub fn`, the body type-checks.
    assert!(table.import_from(&util, "add", "add"));
    assert!(
        check_with_table(&main, &table).ok(),
        "expected a clean check after importing `add`"
    );
}

#[test]
fn collect_records_pub_extern_items_at_the_file_module_level() {
    // A `pub` extern `struct`/`fn` is `use`-able across files exactly as a plain
    // `pub` item is (`lang/EXTERN.md` §Visibility). The enclosing extern `mod` is
    // an import-path concern, not a `use`-namespace segment, so the items export
    // at the file-module level. The `extern` block itself is private.
    let table = collect(&module(
        "extern {\n  \
         #[extern(path = \"bun:sqlite\")]\n  \
         pub mod sqlite {\n    \
         pub struct Database;\n    \
         pub fn open(path: string) -> Database;\n    \
         struct Hidden;\n  \
         }\n}\n",
    ));

    assert!(
        table.type_is_public("Database"),
        "a `pub` extern struct exports at the file level"
    );
    assert!(
        table.value_is_public("open"),
        "a `pub` extern fn exports at the file level"
    );
    assert!(
        !table.type_is_public("Hidden"),
        "a non-`pub` extern struct stays private"
    );
}

#[test]
fn import_from_brings_a_pub_extern_struct_and_its_constructor() {
    let db = collect(&module(
        "extern {\n  \
         #[extern(path = \"bun:sqlite\")]\n  \
         pub mod sqlite {\n    \
         pub struct Database;\n    \
         impl Database {\n      \
         #[extern(constructor)] pub fn new(filename: string);\n    \
         }\n  \
         }\n}\n",
    ));
    assert!(db.type_is_public("Database"));

    // Importing the `pub` extern struct carries its type and its methods (the
    // constructor rides along as a method), so `Database::new` resolves.
    let mut table = collect(&module("fn main() {}\n"));
    assert!(table.import_from(&db, "Database", "Database"));
    assert!(table.types.contains_key("Database"));
    assert!(
        table
            .methods
            .contains_key(&("Database".to_string(), "new".to_string())),
        "the imported extern struct carries its constructor"
    );
}

#[test]
fn import_from_reports_a_missing_symbol() {
    let util = collect(&module(
        "pub fn add(a: number, b: number) -> number { a + b }\n",
    ));
    let mut table = collect(&module("fn main() {}\n"));
    assert!(
        !table.import_from(&util, "nope", "nope"),
        "importing an absent symbol should fail"
    );
}
