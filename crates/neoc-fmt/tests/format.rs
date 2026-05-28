//! Tests for the neoc source formatter: messy input reaches a canonical form,
//! comments survive, formatting is idempotent, and the hand-written corpus is
//! stable under it.

use std::fs;
use std::path::PathBuf;

use neoc_fmt::format;

/// Formatting twice equals formatting once.
fn assert_idempotent(src: &str) {
    let once = format(src).expect("first format");
    let twice = format(&once).expect("second format");
    assert_eq!(once, twice, "formatting is not idempotent:\n{once}");
}

#[test]
fn struct_reaches_canonical_form() {
    let messy = "struct   Point{x:number,y:number}";
    let out = format(messy).unwrap();
    assert_eq!(out, "struct Point {\n    x: number,\n    y: number,\n}\n");
    assert_idempotent(messy);
}

#[test]
fn extern_block_reaches_canonical_form() {
    let messy = "extern{#[extern(path=\"vnd:rusqlite\")]mod rusqlite{fn open(p:string)->Unknown;}struct Changes{changes:number}impl Changes{fn total(self)->number;}}";
    let out = format(messy).unwrap();
    assert_eq!(
        out,
        "extern {\n    \
         #[extern(path = \"vnd:rusqlite\")] mod rusqlite {\n        \
         fn open(p: string) -> Unknown;\n    \
         }\n    \
         struct Changes {\n        \
         changes: number,\n    \
         }\n    \
         impl Changes {\n        \
         fn total(self) -> number;\n    \
         }\n}\n"
    );
    assert_idempotent(messy);
}

#[test]
fn unit_and_newtype_structs() {
    assert_eq!(format("struct Unit ;").unwrap(), "struct Unit;\n");
    assert_eq!(
        format("struct Wrapper ( number ) ;").unwrap(),
        "struct Wrapper(number);\n"
    );
}

#[test]
fn enum_impl_and_match() {
    let messy = "
enum Dir{N,S,E,W}
impl Dir{
fn name(self)->string{match self{Dir::N=>\"north\",Dir::S=>\"south\",_=>\"other\"}}
}
";
    let out = format(messy).unwrap();
    let expected = "\
enum Dir {
    N,
    S,
    E,
    W,
}

impl Dir {
    fn name(self) -> string {
        match self {
            Dir::N => \"north\",
            Dir::S => \"south\",
            _ => \"other\",
        }
    }
}
";
    assert_eq!(out, expected);
    assert_idempotent(messy);
}

#[test]
fn fn_with_leading_and_trailing_comments() {
    let messy = "\
// leads the function
fn main() {
  let x = 1; // trails the let
  println(x);
}
";
    let out = format(messy).unwrap();
    assert!(
        out.contains("// leads the function"),
        "leading comment lost:\n{out}"
    );
    assert!(
        out.contains("// trails the let"),
        "trailing comment lost:\n{out}"
    );
    assert_idempotent(messy);
}

#[test]
fn comments_inside_a_match_survive() {
    let messy = "\
fn f(n: number) -> number {
  match n {
    // the zero case
    0 => 1,
    _ => 2,
  }
}
";
    let out = format(messy).unwrap();
    assert!(
        out.contains("// the zero case"),
        "match comment lost:\n{out}"
    );
    assert_idempotent(messy);
}

#[test]
fn doc_comments_survive() {
    let messy = "\
/// A point in the plane.
struct Point { x: number, y: number }
";
    let out = format(messy).unwrap();
    assert!(
        out.contains("/// A point in the plane."),
        "doc comment lost:\n{out}"
    );
    assert_idempotent(messy);
}

#[test]
fn blank_line_between_top_level_items() {
    let messy = "struct A;\nstruct B;\n";
    let out = format(messy).unwrap();
    assert_eq!(out, "struct A;\n\nstruct B;\n");
}

#[test]
fn leading_comment_hugs_its_item() {
    let messy = "// about A\nstruct A;\n\n// about B\nstruct B;\n";
    let out = format(messy).unwrap();
    let expected = "// about A\nstruct A;\n\n// about B\nstruct B;\n";
    assert_eq!(out, expected);
}

#[test]
fn parse_error_is_rejected() {
    let err = format("fn main( {").unwrap_err();
    assert_eq!(err, neoc_fmt::FmtError::Parse);
}

#[test]
fn trailing_newline_is_exactly_one() {
    let out = format("struct A;\n\n\n\n").unwrap();
    assert!(out.ends_with("A;\n"));
    assert!(!out.ends_with("\n\n"));
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lang/CONFORMANCE/dynamic")
        .canonicalize()
        .expect("corpus dir")
}

#[test]
fn corpus_is_stable_under_formatting() {
    let dir = corpus_dir();
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("read corpus dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("neoc") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read fixture");
        let once = format(&src).unwrap_or_else(|e| panic!("format {} failed: {e}", path.display()));
        let twice =
            format(&once).unwrap_or_else(|e| panic!("reformat {} failed: {e}", path.display()));
        assert_eq!(once, twice, "formatting not stable for {}", path.display());
        checked += 1;
    }
    assert!(checked >= 20, "expected to check the corpus, saw {checked}");
}

#[test]
fn corpus_is_a_near_no_op() {
    // The corpus is the canonical style. Formatting it should reproduce it
    // verbatim (the files end without a trailing blank but `format` adds one
    // trailing newline, which all fixtures already have or tolerate).
    let dir = corpus_dir();
    for entry in fs::read_dir(&dir).expect("read corpus dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("neoc") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read fixture");
        let out = format(&src).unwrap();
        // Normalise the source's trailing newlines the way `format` does, then
        // compare. This proves fmt is a no-op on the corpus body.
        let mut norm = src.trim_end().to_string();
        norm.push('\n');
        assert_eq!(out, norm, "fmt changed corpus file {}", path.display());
    }
}
