//! The tree, read from Luau and written back: every shape of type the declarations use comes out as it went in,
//! through JSON too, and what the reader does not have is an error that says where.

use neoc_decl::{Section, Ty, luau, parse};

const FILE: &str = "-- ===== lib:probe =====

export type lib_probe_Kind = \"a\" | \"b\"

--- A row.
--- Of a table.
export type lib_probe_Row = {
\t--- Its number.
\t---
\t--- Counted from one.
\tid: number,
\ttag: string?,
\t[\"end\"]: number?,
\tinline: { status: number?, [string]: any },
}

export type lib_probe_Empty = {}

export type lib_probe = {
\trows: { lib_probe_Row },
\tnames: { [string]: lib_probe_Kind }?,
\tsize: (self: lib_probe) -> number,
\tjoin: (first: string, ...string) -> string,
\tspawn: (fn: any, ...any) -> ...any,
\tfirst: (path: string | number, ...(string | number)) -> (any?, ...any),
\ttx: <T>(body: () -> T) -> T,
\tserve: (handler: (req: lib_probe_Row) -> ...(string | { status: number? } | nil)) -> (() -> ())?,
\tnotify: ((note: lib_probe_Row) -> ()) & ((self: lib_probe, note: lib_probe_Row) -> ()),
\topen: (path: string, end_: boolean) -> (lib_probe_Row?, string?),
\tlater: () -> ((value: any) -> ()),
\teither: (string | number)?,
\tflag: true | false,
\tquoted: \"a\\\"b\" | \"\\u{41}\\z\",
}

-- ===== globals =====

--- The probe.
declare probe: lib_probe

";

#[test]
fn what_is_read_is_written_back_as_it_was() {
    let written: String = parse::sections(FILE)
        .unwrap()
        .iter()
        .map(luau::section)
        .collect();
    assert_eq!(written, FILE);
}

#[test]
fn the_tree_is_the_same_through_json() {
    let sections = parse::sections(FILE).unwrap();
    let json = serde_json::to_string(&sections).unwrap();
    let read: Vec<Section> = serde_json::from_str(&json).unwrap();
    assert_eq!(read, sections);
    // what is empty is left out of the JSON
    assert!(
        !json.contains("\"doc\":\"\"") && !json.contains("\"generics\":[]"),
        "{json}"
    );
}

#[test]
fn a_name_is_kept_as_the_runtime_has_it_and_escaped_when_written() {
    let sections = parse::sections(FILE).unwrap();
    let Ty::Table { props, .. } = &sections[0].aliases[1].ty else {
        panic!("a table")
    };
    // the quoted key of Luau is the name `end` in the tree
    assert_eq!(props[2].name, "end");
    assert_eq!(props[0].doc, "Its number.\n\nCounted from one.");
    assert_eq!(sections[0].aliases[1].doc, "A row.\nOf a table.");
}

#[test]
fn one_type_and_one_return_are_read_alone() {
    let ty = parse::ty("{ [string]: number }?").unwrap();
    assert_eq!(luau::ty(&ty), "{ [string]: number }?");
    for returns in [
        "()",
        "string",
        "(string?, string?)",
        "...any",
        "(any?, ...any)",
        "((a: string) -> ())",
    ] {
        assert_eq!(luau::returns(&parse::ret(returns).unwrap()), returns);
    }
    assert!(
        parse::ty("string number")
            .unwrap_err()
            .contains("goes on after the type")
    );
    assert!(
        parse::ret("(string, string) junk")
            .unwrap_err()
            .contains("goes on after the type")
    );
}

#[test]
fn what_the_reader_does_not_have_is_an_error_that_says_where() {
    let failed =
        |body: &str| parse::sections(&format!("-- ===== lib:probe =====\n{body}")).unwrap_err();
    assert!(
        failed("export type a = typeof(print)\n").contains("line 2: `typeof` is not translated")
    );
    assert!(
        failed("export type a = lib_other.Thing\n").contains("line 2: `lib_other` is followed by")
    );
    assert!(failed("export type a = \"a\n").contains("line 2: a string does not end"));
    assert!(failed("local x = y\n").contains("expected `export type` or `declare`"));
    assert!(
        parse::sections("export type a = string\n")
            .unwrap_err()
            .contains("before the first")
    );
}
