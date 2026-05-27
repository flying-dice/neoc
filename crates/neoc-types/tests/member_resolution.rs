//! Member resolution (#17): the checker records each `recv.field` / `recv.m()`
//! access against the member's declaration, so tooling can goto/reference it
//! through the resolution map rather than a name-based CST scan.

use neoc_types::resolution;

fn resolve(src: &str) -> neoc_types::Resolution {
    resolution(&neoc_parser::parse(src).module)
}

#[test]
fn a_field_access_resolves_to_the_field_declaration() {
    let src = "struct Point {\n  x: number,\n}\nfn get(p: Point) -> number {\n  p.x\n}\n";
    let r = resolve(src);
    // Cursor on `x` in `p.x` (the access, the last `x`).
    let cur = src.rfind("p.x").unwrap() + 2;
    let m = r.member_at(cur).expect("member resolved");
    assert_eq!(m.owner, "Point");
    assert_eq!(m.member, "x");
    // The def-span points at the field name in the struct declaration.
    assert_eq!(&src[m.def.0..m.def.1], "x");
    assert!(
        m.def.0 < src.find("fn get").unwrap(),
        "def is in the struct"
    );
}

#[test]
fn a_method_call_resolves_to_the_method_declaration() {
    let src = "struct Counter {\n  n: number,\n}\nimpl Counter {\n  fn get(self) -> number {\n    self.n\n  }\n}\nfn run(c: Counter) -> number {\n  c.get()\n}\n";
    let r = resolve(src);
    let cur = src.find("c.get()").unwrap() + 2;
    let m = r.member_at(cur).expect("method resolved");
    assert_eq!(m.owner, "Counter");
    assert_eq!(m.member, "get");
    assert_eq!(&src[m.def.0..m.def.1], "get");
}

#[test]
fn a_trait_method_on_a_trait_typed_receiver_resolves_to_the_trait() {
    let src = "trait Show {\n  fn show(self) -> string;\n}\nfn render(x: Show) -> string {\n  x.show()\n}\n";
    let r = resolve(src);
    let cur = src.find("x.show()").unwrap() + 2;
    let m = r.member_at(cur).expect("trait method resolved");
    assert_eq!(m.owner, "Show");
    assert_eq!(m.member, "show");
    assert_eq!(&src[m.def.0..m.def.1], "show");
}

#[test]
fn member_occurrences_group_same_typed_accesses() {
    let src = "struct P {\n  x: number,\n}\nfn a(p: P) -> number {\n  p.x\n}\nfn b(q: P) -> number {\n  q.x\n}\n";
    let r = resolve(src);
    let occ = r.member_occurrences("P", "x");
    // The field declaration plus both `.x` accesses.
    assert_eq!(occ.len(), 3, "def + two uses: {occ:?}");
}
