//! Tests for the neoc source formatter: messy input reaches a canonical form,
//! comments survive, formatting is idempotent, and the hand-written corpus is
//! stable under it.

use std::fs;
use std::path::{Path, PathBuf};

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
         #[extern(path = \"vnd:rusqlite\")]\n    \
         mod rusqlite {\n        \
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
fn consecutive_uses_group_without_blank_lines() {
    // Author-written blanks between consecutive uses are collapsed.
    let messy = "use a::B;\n\nuse c::D;\n\nuse e::F;\n";
    let out = format(messy).unwrap();
    assert_eq!(out, "use a::B;\nuse c::D;\nuse e::F;\n");
    assert_idempotent(messy);
    // And no blank is introduced when there wasn't one.
    assert_eq!(
        format("use a::B;\nuse c::D;\n").unwrap(),
        "use a::B;\nuse c::D;\n"
    );
}

#[test]
fn use_block_separated_from_other_items_by_blank() {
    let messy = "use a::B;\nuse c::D;\nstruct G;\n";
    let out = format(messy).unwrap();
    assert_eq!(out, "use a::B;\nuse c::D;\n\nstruct G;\n");
    assert_idempotent(messy);
}

#[test]
fn comment_between_uses_is_preserved_and_not_joined_across() {
    let messy = "use a::B;\n// keep me\nuse c::D;\n";
    let out = format(messy).unwrap();
    assert_eq!(out, "use a::B;\n// keep me\nuse c::D;\n");
    assert_idempotent(messy);
}

#[test]
fn extern_block_item_attribute_breaks_onto_own_line() {
    let messy = "extern { #[extern(ambient)] struct Response; }";
    let out = format(messy).unwrap();
    assert_eq!(
        out,
        "extern {\n    #[extern(ambient)]\n    struct Response;\n}\n"
    );
    assert_idempotent(messy);
}

#[test]
fn extern_impl_method_attribute_breaks_onto_own_line() {
    let messy = "extern { impl D { #[extern(constructor)] fn new(x: number); } }";
    let out = format(messy).unwrap();
    assert_eq!(
        out,
        "extern {\n    impl D {\n        #[extern(constructor)]\n        fn new(x: number);\n    }\n}\n"
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
fn associated_types_in_trait_and_impl() {
    // Regression (#82): a trait's `type Item;`, an impl's `type Item = number;`,
    // and a `Self::Item` projection must all survive formatting. Before the fix
    // the first panicked, the second was silently dropped, and the third lost
    // its `::Item`.
    let messy = "\
trait Container{type Item;fn first(self)->Self::Item;}
impl Container for Bag{type Item=number;fn first(self)->Self::Item{self.v}}
";
    let out = format(messy).unwrap();
    let expected = "\
trait Container {
    type Item;

    fn first(self) -> Self::Item;
}

impl Container for Bag {
    type Item = number;

    fn first(self) -> Self::Item {
        self.v
    }
}
";
    assert_eq!(out, expected);
    assert_idempotent(messy);
}

#[test]
fn supertrait_bounds_survive_formatting() {
    // Regression: `neoc fmt` dropped a trait's supertrait bound — `trait B: A`
    // round-tripped to `trait B` because `trait_declaration` ignored the parsed
    // `supertraits` field. Same dropped-parsed-field class as the #82
    // associated-type bug (which got `associated_types_in_trait_and_impl`); this
    // fix had only the corpus no-op gate, lost the moment 8-04 is renamed.
    let single = "trait Greet:Show{fn hi(self)->string;}";
    assert_eq!(
        format(single).unwrap(),
        "trait Greet: Show {\n    fn hi(self) -> string;\n}\n"
    );
    assert_idempotent(single);

    // Generic trait, multiple bounds: order and `+` join preserved.
    let multi = "trait B<T>:A+C{}";
    assert_eq!(format(multi).unwrap(), "trait B<T>: A + C {}\n");
    assert_idempotent(multi);
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

/// Layers whose accept cases are written in canonical style.
const CANONICAL_LAYERS: &[&str] = &["dynamic", "static"];

/// Every corpus layer that carries parseable programs. `syntax/` accept cases
/// exercise valid but non-canonical surface forms (optional separators,
/// layout), so they must survive formatting without having to match it.
const PARSEABLE_LAYERS: &[&str] = &["dynamic", "static", "syntax"];

/// Every accepted `.neoc` source in `layers`, including the modules of a
/// multi-module case (a subdirectory of the layer). A `.reject` case is a
/// deliberate parse failure and carries a different extension, so the `.neoc`
/// filter exempts it by the corpus's own marker; no case is listed by hand.
fn corpus_files(layers: &[&str]) -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lang/CONFORMANCE");
    let mut files = Vec::new();
    for layer in layers {
        collect_neoc(
            &root.join(layer).canonicalize().expect("corpus dir"),
            &mut files,
        );
    }
    files.sort();
    files
}

/// Append every `.neoc` file under `dir`, descending into subdirectories.
fn collect_neoc(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read corpus dir") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_neoc(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("neoc") {
            files.push(path);
        }
    }
}

#[test]
fn corpus_is_stable_under_formatting() {
    let mut checked = 0;
    for path in corpus_files(PARSEABLE_LAYERS) {
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
    // The canonical layers define the house style. Formatting them should
    // reproduce them verbatim (the files end without a trailing blank but `format` adds one
    // trailing newline, which all fixtures already have or tolerate).
    for path in corpus_files(CANONICAL_LAYERS) {
        let src = fs::read_to_string(&path).expect("read fixture");
        let out = format(&src).unwrap();
        // Normalise the source's trailing newlines the way `format` does, then
        // compare. This proves fmt is a no-op on the corpus body.
        let mut norm = src.trim_end().to_string();
        norm.push('\n');
        assert_eq!(out, norm, "fmt changed corpus file {}", path.display());
    }
}
