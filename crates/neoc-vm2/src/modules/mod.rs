//! The `require` registry: every module an mlua-ir `#[module]` declares in this crate, by name.
//!
//! Three namespaces, as in neoc-vm: `std:*` mirrors the Rust standard library, `lib:*` is gap-fillers and neoc's
//! own modules, `vnd:*` is vendored crates named after the crate. A script reaches a module only through
//! `require("namespace:name")`; there is no ambient global.
//!
//! A module appears here by existing: writing `#[mlua_ir::module("lib:base64")] pub mod base64 { ... }` registers it
//! (the macro submits it to `mlua_ir::modules()`). The only line a new module needs in this crate is its `pub mod`
//! in `lib/mod.rs`, `std/mod.rs` or `vnd/mod.rs`, which gets it compiled and linked.

// What mlua-ir's macros imply, for every module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

use ::std::cmp::max;
use ::std::collections::HashMap;

use mlua::{Lua, Table};
use mlua_ir::ModuleEntry;
use mlua_ir::ir::ModuleKind;
use strsim::levenshtein;

pub mod lib;
pub mod std;
pub mod vnd;

/// Soft cap on how many namespace members a `require` error inlines before it truncates with a `(+N more)` tail.
const MAX_AVAILABLE_LIST: usize = 50;

fn entries() -> impl Iterator<Item = &'static ModuleEntry> {
    mlua_ir::modules().into_iter().filter(|entry| entry.kind == ModuleKind::Require)
}

/// The module names (`"lib:json"`, ...), sorted.
pub fn names() -> impl Iterator<Item = &'static str> {
    entries().map(|entry| entry.name)
}

/// Builds the module `name` in `lua`, or `None` when there is no such module. Each call builds a new table: the
/// caller caches it so that every `require` of one module sees the same table. This is the seam for an embedder that
/// wants its own `require` over the subset of [`names`] it offers.
#[must_use]
pub fn build(lua: &Lua, name: &str) -> Option<mlua::Result<Table>> {
    entries().find(|entry| entry.name == name).map(|entry| (entry.install)(lua))
}

/// Installs `require` over every module.
///
/// # Errors
/// Fails when a module cannot be built.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    install_with(lua, |_| true)
}

/// Installs `require` over only the modules named in `allowed`; any other name is unknown.
///
/// # Errors
/// Fails when a module cannot be built.
pub fn install_allowed(lua: &Lua, allowed: &[&str]) -> mlua::Result<()> {
    install_with(lua, |name| allowed.contains(&name))
}

fn install_with(lua: &Lua, allowed: impl Fn(&str) -> bool) -> mlua::Result<()> {
    let mut modules: HashMap<String, Table> = HashMap::new();
    for entry in entries().filter(|entry| allowed(entry.name)) {
        modules.insert(entry.name.to_owned(), (entry.install)(lua)?);
    }
    let known: Vec<String> = modules.keys().cloned().collect();
    let require = lua.create_function(move |_, name: String| -> mlua::Result<Table> {
        modules.get(&name).cloned().ok_or_else(|| {
            let known: Vec<&str> = known.iter().map(String::as_str).collect();
            unknown_module(&name, &known)
        })
    })?;
    lua.globals().set("require", require)
}

fn find_closest<'a>(requested: &str, candidates: &[&'a str], threshold: usize) -> Option<&'a str> {
    candidates
        .iter()
        .filter(|c| **c != requested)
        .map(|c| (*c, levenshtein(requested, c)))
        .filter(|(_, d)| *d <= threshold)
        .min_by(|(a, da), (b, db)| da.cmp(db).then_with(|| a.cmp(b)))
        .map(|(c, _)| c)
}

/// `"json"` matches `"lib:json"`: the shortest candidate with that suffix, ties broken alphabetically.
fn find_by_suffix<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    if name.is_empty() {
        return None;
    }
    let suffix = format!(":{name}");
    candidates
        .iter()
        .filter(|c| c.ends_with(&suffix))
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .copied()
}

fn truncate_list(members: &[&str], cap: usize) -> String {
    match members.split_at(members.len().min(cap)) {
        (shown, []) => shown.join(", "),
        (shown, rest) => format!("{} (+{} more)", shown.join(", "), rest.len()),
    }
}

/// The error `require(name)` raises for a name that is not a module: what was meant, and what exists.
#[must_use]
pub fn unknown_module(name: &str, known: &[&str]) -> mlua::Error {
    let namespace = name.find(':').map(|pos| &name[..pos]);
    let suggestion = if namespace.is_some() {
        find_closest(name, known, max(1, name.len() / 3))
    } else {
        find_by_suffix(name, known)
    };
    let mut message = format!("require: unknown module {name:?}");
    if let Some(suggestion) = suggestion {
        message.push_str(&format!(" — did you mean {suggestion:?}?"));
    }
    let available = match namespace {
        Some(namespace) => {
            let prefix = format!("{namespace}:");
            let mut members: Vec<&str> = known.iter().filter(|k| k.starts_with(&prefix)).copied().collect();
            members.sort_unstable();
            (!members.is_empty()).then(|| truncate_list(&members, MAX_AVAILABLE_LIST))
        }
        None if suggestion.is_none() => Some("std:<name>, lib:<name>, vnd:<name>".to_owned()),
        None => None,
    };
    if let Some(list) = available {
        message.push_str(&format!(" available: {list}"));
    }
    mlua::Error::external(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_engine;

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
        let err =
            lua.load(r#"return require("lib:base46")"#).eval::<Table>().expect_err("unknown").to_string();
        assert!(
            err.contains(r#"did you mean "lib:base64"?"#) && err.contains("available: lib:base64"),
            "{err}"
        );
        let bare = lua.load(r#"return require("base64")"#).eval::<Table>().expect_err("unknown").to_string();
        assert!(bare.contains(r#"did you mean "lib:base64"?"#), "{bare}");
    }

    #[test]
    fn install_allowed_offers_only_those() {
        let lua = crate::make_bare_engine().expect("engine");
        install_allowed(&lua, &["lib:base64"]).expect("install");
        assert!(lua.load(r#"return require("lib:base64")"#).eval::<Table>().is_ok());
        assert!(lua.load(r#"return require("std:time")"#).eval::<Table>().is_err());
    }

    #[test]
    fn the_closest_name_wins_and_ties_break_alphabetically() {
        assert_eq!(find_closest("lib:jsn", &["lib:json", "lib:base64"], 2), Some("lib:json"));
        assert_eq!(find_closest("lib:json", &["lib:json"], 2), None);
        assert_eq!(find_closest("lib:zzz", &["lib:bbb", "lib:aaa"], 3), Some("lib:aaa"));
    }
}
