//! A `#[link(name = "...")]` `extern "<host>" { ... }` block lowers to a native
//! import in the backend its host maps to, bringing the block's value items
//! (`fn`/`const`) into scope so call sites stay bare. A block hosted by a
//! different runtime emits nothing in this backend. An un-hosted `ext` item
//! (here `print`) keeps its bare-global behaviour and produces no import.
//!
//! These are emit-level assertions: the program references an external module,
//! so it is not run.

use neoc_compiler::{Target, transpile};

/// One hosted block per backend, each `#[link]`ed to a module, plus a stray
/// un-hosted `ext fn`. Each backend should emit the import for the block whose
/// host maps to it and skip the other two.
const SOURCE: &str = r#"
#[link(name = "color")]
extern "node" {
    fn lighten(c: string, amount: number) -> string;
    const WHITE: string;
}

#[link(name = "palette")]
extern "neoc" {
    fn darken(c: string) -> string;
}

#[link(name = "huelib")]
extern "python" {
    fn invert(c: string) -> string;
}

ext fn print(s: string);

fn main() { print(lighten(WHITE, 0.1)); }
"#;

#[test]
fn js_emits_named_import_for_its_host_only() {
    let out = transpile(SOURCE, Target::Js).output;
    assert!(
        out.contains("import { lighten, WHITE } from \"color\";"),
        "missing JS import:\n{out}"
    );
    // The Lua- and Python-hosted blocks emit nothing in the JS backend.
    assert!(
        !out.contains("palette") && !out.contains("huelib"),
        "mismatched-host block leaked into JS output:\n{out}"
    );
    // `print` is un-hosted: a bare call, no import generated for it.
    assert!(
        !out.contains("print } from"),
        "print should not be imported:\n{out}"
    );
    assert!(
        out.contains("print(lighten(WHITE, 0.1));"),
        "call site changed:\n{out}"
    );
}

#[test]
fn python_emits_from_import_for_its_host_only() {
    let out = transpile(SOURCE, Target::Python).output;
    assert!(
        out.contains("from huelib import invert"),
        "missing Python import:\n{out}"
    );
    assert!(
        !out.contains("color") && !out.contains("palette"),
        "mismatched-host block leaked into Python output:\n{out}"
    );
    assert!(
        !out.contains("import print"),
        "print should not be imported:\n{out}"
    );
}

#[test]
fn lua_emits_require_for_its_host_only() {
    let out = transpile(SOURCE, Target::Lua).output;
    assert!(
        out.contains("require(\"palette\")"),
        "missing Lua require:\n{out}"
    );
    assert!(
        out.contains("local darken = "),
        "missing darken local:\n{out}"
    );
    assert!(
        !out.contains("color") && !out.contains("huelib"),
        "mismatched-host block leaked into Lua output:\n{out}"
    );
    // `print` is un-hosted: no require, no local, bare call.
    assert!(
        !out.contains("local print ="),
        "print should not be a local:\n{out}"
    );
}

#[test]
fn positional_link_attr_also_emits() {
    // The positional `#[link("m")]` form is accepted alongside the key-value one.
    let src = r#"
#[link("color")]
extern "node" { fn f(x: number) -> number; }
fn main() { let _ = f(1); }
"#;
    let out = transpile(src, Target::Js).output;
    assert!(
        out.contains("import { f } from \"color\";"),
        "positional #[link] should emit:\n{out}"
    );
}

#[test]
fn extern_without_link_emits_no_import() {
    // A hosted block without `#[link]` declares host globals: no import.
    let src = r#"
extern "neoc" { fn host_fn(n: number) -> number; }
fn main() { let _ = host_fn(1); }
"#;
    let out = transpile(src, Target::Lua).output;
    assert!(
        !out.contains("require("),
        "no-link extern should not require:\n{out}"
    );
}

#[test]
fn un_hosted_ext_emits_no_import() {
    // A module with only an un-hosted `ext fn` emits no import preamble.
    let src = "ext fn print(s: string);\nfn main() { print(\"hi\"); }\n";
    for target in [Target::Js, Target::Python, Target::Lua] {
        let out = transpile(src, target).output;
        assert!(
            !out.contains("import"),
            "unexpected import for {target}:\n{out}"
        );
        assert!(
            !out.contains("require("),
            "unexpected require for {target}:\n{out}"
        );
    }
}
