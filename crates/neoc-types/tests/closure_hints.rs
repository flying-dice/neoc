//! Closure-parameter typing audit (`lang/EXTERN.md` §Option marshalling).
//!
//! Every unannotated closure parameter in a call argument must be typed
//! whenever the finished call fixes its type; otherwise an `Option` crossing
//! through it reaches the host unmarshalled.

use std::path::Path;

fn misses(src: &str) -> Vec<String> {
    let parsed = neoc_parser::parse(src);
    neoc_types::closure_hint_misses(&parsed.module)
}

fn neoc_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            neoc_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "neoc") {
            out.push(p);
        }
    }
}

#[test]
fn conformance_corpus_has_no_missed_hints() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang/CONFORMANCE");
    let mut files = Vec::new();
    neoc_files(&root, &mut files);
    assert!(!files.is_empty());
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap();
        let m = misses(&src);
        assert!(m.is_empty(), "{}: {m:?}", f.display());
    }
}

const HOST: &str = r"
extern {
    struct Settings { title: Option<string>, size: number }
    fn mk() -> Settings;
    fn println(v: Unknown);
}
fn id<T>(x: T) -> T { x }
fn apply<T>(x: T, f: fn(T) -> number) -> number { f(x) }
fn first<T>(x: T, f: fn(T) -> Option<string>) -> T { x }
fn both<T>(seed: Unknown, f: fn(T) -> Option<string>, g: fn(T) -> T) -> Option<string> { f(g(seed)) }
";

#[test]
fn known_shapes_have_no_missed_hints() {
    for body in [
        "apply(mk(), |s| s.size);",
        "first(mk(), |s| s.title);",
        "println(both(mk(), |s| s.title, |n: Settings| n));",
        "println(both(mk(), |s| s.title, id(|n: Settings| n)));",
        "let r = apply(id(mk()), |s| s.size);",
    ] {
        let src = format!("{HOST}\npub fn main() {{ {body} }}\n");
        let m = misses(&src);
        assert!(m.is_empty(), "{body}: {m:?}");
    }
}

/// The audit itself fires: a callee whose parameter the call cannot fix leaves
/// nothing to miss, so only a real miss is reported.
#[test]
fn audit_reports_nothing_when_the_call_fixes_nothing() {
    let src = format!("{HOST}\npub fn main() {{ let f = |s| s; }}\n");
    assert!(misses(&src).is_empty());
}
