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
