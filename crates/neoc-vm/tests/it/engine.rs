//! The engine with every module of the runtime linked: what a worker is offered, and what an `assert` of a module's
//! failed call keeps. (The engine itself is `neoc-vm-core`'s, which links no module.)

use neoc_vm::mlua::{self, Lua};
use neoc_vm::{Life, make_bare_engine, make_engine, modules, worker_engine};

/// A worker is offered what its parent was: everything for a full engine, the same subset after
/// `install_allowed`, and nothing at all (no worker) on a bare engine.
#[test]
fn a_worker_engine_offers_what_its_parent_was_offered() {
    let require = |lua: &Lua, name: &str| lua.load(format!("return require({name:?})")).eval::<mlua::Table>();

    let full = make_engine().expect("engine");
    let worker =
        worker_engine(&full).expect("a full engine has a worker engine")(Life::new()).expect("worker");
    assert!(require(&worker, "std:time").is_ok() && require(&worker, "lib:base64").is_ok());
    assert!(worker_engine(&worker).is_some(), "a worker can start workers of its own");

    let narrow = make_bare_engine().expect("engine");
    assert!(worker_engine(&narrow).is_none(), "a bare engine has no worker engine");
    modules::install_allowed(&narrow, &["lib:base64"]).expect("install");
    let worker = worker_engine(&narrow).expect("install_allowed sets one")(Life::new()).expect("worker");
    assert!(require(&worker, "lib:base64").is_ok());
    assert!(require(&worker, "std:time").is_err(), "a worker is not offered more than its parent");
    let grandchild = worker_engine(&worker).expect("and so on down")(Life::new()).expect("worker");
    assert!(require(&grandchild, "std:time").is_err());
}

/// `assert` keeps the typed error of a failed call: the table itself is raised, and `pcall` hands it back.
#[test]
fn assert_raises_a_typed_error_as_it_is() {
    let lua = make_engine().expect("engine");
    let (ok, kind, is_table, op): (bool, String, bool, String) = lua
        .load(
            r#"
            local fs = require("std:fs")
            local ok, err = pcall(function() return assert(fs.read("no/such/file/anywhere")) end)
            return ok, err.kind, type(err) == "table", err.op
            "#,
        )
        .eval()
        .expect("script");
    assert!(!ok && is_table);
    assert_eq!((kind.as_str(), op.as_str()), ("not_found", "std:fs.read"));
}
