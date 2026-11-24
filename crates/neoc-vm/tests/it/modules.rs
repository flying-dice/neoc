//! The `require` registry over every module the runtime links.

use neoc_vm::make_engine;
use neoc_vm::mlua::Table;
use neoc_vm::modules::{install_allowed, names};

#[test]
fn every_registered_module_requires_to_a_table() {
    let lua = make_engine().expect("engine");
    let names: Vec<&str> = names().collect();
    assert!(names.contains(&"lib:base64") && names.contains(&"std:time"), "{names:?}");
    for name in names {
        let kind: String = lua
            .load(format!("return type(require({name:?}))"))
            .eval()
            .unwrap_or_else(|e| panic!("require({name:?}) failed: {e}"));
        assert_eq!(kind, "table", "{name}");
    }
}

#[test]
fn an_unknown_module_says_what_was_meant_and_what_exists() {
    let lua = make_engine().expect("engine");
    let err = lua.load(r#"return require("lib:base46")"#).eval::<Table>().expect_err("unknown").to_string();
    assert!(
        err.contains(r#"did you mean "lib:base64"?"#)
            && err.contains("available: lib:")
            && err.contains(", lib:base64,"),
        "{err}"
    );
    let bare = lua.load(r#"return require("base64")"#).eval::<Table>().expect_err("unknown").to_string();
    assert!(bare.contains(r#"did you mean "lib:base64"?"#), "{bare}");
}

#[test]
fn install_allowed_offers_only_those() {
    let lua = neoc_vm::make_bare_engine().expect("engine");
    install_allowed(&lua, &["lib:base64"]).expect("install");
    assert!(lua.load(r#"return require("lib:base64")"#).eval::<Table>().is_ok());
    assert!(lua.load(r#"return require("std:time")"#).eval::<Table>().is_err());
}
