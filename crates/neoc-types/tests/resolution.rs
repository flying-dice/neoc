//! Scope-aware resolution (`STATIC.md` §2): a use resolves to the *binding* it
//! refers to, respecting lexical scope and shadowing — the basis for
//! scope-precise references/rename/goto (`tasks/scope-aware-resolution.md`).

use neoc_types::{BindingKind, Resolved, resolution};

/// Parse `src` and resolve it.
fn resolve(src: &str) -> neoc_types::Resolution {
    let module = neoc_parser::parse(src).module;
    resolution(&module)
}

/// The start byte of the `occ`-th (1-based) occurrence of `needle` — a position
/// inside that identifier.
fn at(src: &str, needle: &str, occ: usize) -> usize {
    let mut from = 0;
    let mut start = 0;
    for _ in 0..occ {
        start = src[from..].find(needle).expect("occurrence") + from;
        from = start + 1;
    }
    start
}

#[test]
fn a_param_use_resolves_to_the_param() {
    let src = "fn f(x: number) -> number { x }\n";
    let r = resolve(src);
    let params: Vec<_> = r
        .bindings()
        .iter()
        .filter(|b| b.kind == BindingKind::Param)
        .collect();
    assert_eq!(params.len(), 1, "one param binding");
    let id = params[0].id;
    // The use of `x` in the body resolves to the param.
    assert_eq!(r.at(at(src, "x", 2)), Some(Resolved::Local(id)));
    // Occurrences are the def site plus the one use.
    assert_eq!(r.local_occurrences(id).len(), 2);
}

#[test]
fn a_shadowing_local_and_what_it_shadows_are_distinct_bindings() {
    // The `x` in `let y = x` is the param; the trailing `x` is the inner `let`.
    let src = "fn f(x: number) -> number {\n  let y = x;\n  let x = y;\n  x\n}\n";
    let r = resolve(src);

    let Some(Resolved::Local(param)) = r.at(at(src, "x", 2)) else {
        panic!("`x` in `let y = x` should resolve to the param");
    };
    let Some(Resolved::Local(inner)) = r.at(at(src, "x", 4)) else {
        panic!("the trailing `x` should resolve to the inner `let`");
    };
    assert_ne!(param, inner, "the shadow is a different binding");

    // Renaming the param touches its def + the `let y = x` use, not the inner x.
    assert_eq!(r.local_occurrences(param).len(), 2);
    // Renaming the inner binding touches its def + the trailing use only.
    assert_eq!(r.local_occurrences(inner).len(), 2);
}

#[test]
fn a_local_shadowing_a_top_level_item_wins() {
    // `g` the local shadows `g` the function; the use is the local, and the
    // item has no surviving use — so renaming the item would not touch it.
    let src = "fn g() -> number { 1 }\nfn main() -> number {\n  let g = 2;\n  g\n}\n";
    let r = resolve(src);
    assert!(
        matches!(r.at(at(src, "g", 3)), Some(Resolved::Local(_))),
        "the shadowed use resolves to the local, not the item"
    );
    assert!(
        r.item_use_occurrences("g").is_empty(),
        "the item `g` has no surviving use to rename"
    );
}

#[test]
fn an_unshadowed_item_use_resolves_to_the_item() {
    let src = "fn g() -> number { 1 }\nfn main() -> number { g() }\n";
    let r = resolve(src);
    assert!(
        matches!(r.at(at(src, "g", 2)), Some(Resolved::Item(name)) if name == "g"),
        "the call resolves to the top-level item"
    );
    assert_eq!(r.item_use_occurrences("g").len(), 1);
}
