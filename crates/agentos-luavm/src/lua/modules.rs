//! `require` registry — wires every module into the Lua engine and
//! exposes a custom `require("namespace:name")` resolver.
//!
//! Three namespaces, picked at module-registration time:
//!
//! - **`std:*`** — mirrors the Rust standard library (and tightly-scoped
//!   extensions on top of it). Anything you'd reach for in `std::` lives
//!   here: `std:collections`, `std:fs`, `std:io`, `std:net`, `std:thread`.
//! - **`lib:*`** — blessed gap-fillers and our own custom modules.
//!   Either plugs a hole Rust std doesn't cover (`lib:base64`) or is
//!   something we wrote (`lib:vcr`).
//! - **`vnd:*`** — vendored crates. Module name **must** match the
//!   upstream crate name so the import is unambiguous: `vnd:hyper`,
//!   `vnd:serde_json`, `vnd:rusqlite`, `vnd:toml`. No renaming, no
//!   ambiguity.
//!
//! Scripts pull modules in via `require`:
//!
//! ```lua
//! local fs   = require("std:fs")
//! local log  = require("lib:log")
//! local json = require("vnd:serde_json")
//! ```
//!
//! `require` is the only way to reach a module — there is no ambient `lib`
//! global, no auto-imported `Schema`. Resolution is a flat
//! `HashMap` built once at engine boot; module tables are shared (all callers
//! get the same handle), so writing into one observably mutates it for
//! everyone — same contract as Lua's own require.

use std::cmp::max;
use std::collections::HashMap;

use mlua::{Lua, Table};
use strsim::levenshtein;

use crate::lua::facade::Module;
use crate::lua::{lib, std as stdmod, vnd};

/// Soft cap on how many namespace members a `require` error inlines before it
/// truncates with a `(+N more)` tail. Unlike a hard threshold it degrades
/// gracefully — crossing it shortens the list, never suppresses it — so the
/// value need not track namespace growth: adding a module can never silently
/// drop the catalogue. Sized to inline today's largest namespace (`lib:*`, 31)
/// in full; only a pathologically large namespace truncates.
const MAX_AVAILABLE_LIST: usize = 50;

fn find_closest<'a>(requested: &str, candidates: &[&'a str], threshold: usize) -> Option<&'a str> {
    candidates
        .iter()
        .filter(|c| **c != requested)
        .map(|c| (*c, levenshtein(requested, c)))
        .filter(|(_, d)| *d <= threshold)
        .min_by(|(a, da), (b, db)| da.cmp(db).then_with(|| a.cmp(b)))
        .map(|(c, _)| c)
}

/// Suffix match for a bare module name — `"json"` matches `"lib:json"`.
/// Picks the shortest candidate, ties broken alphabetically.
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

/// Joins a namespace's members for a `require` error, inlining at most `cap`
/// names and summarising any remainder as `(+N more)`. Bounds the message
/// length without ever collapsing to an empty list — a named namespace always
/// yields at least its first `cap` members.
fn truncate_list(members: &[&str], cap: usize) -> String {
    match members.split_at(members.len().min(cap)) {
        (shown, []) => shown.join(", "),
        (shown, rest) => format!("{} (+{} more)", shown.join(", "), rest.len()),
    }
}

/// Install the `require` global over every module.
///
/// # Errors
/// Fails when a module cannot be built.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    install_filtered(lua, None)
}

/// Install the `require` global over only the modules named in `allowed` (`"lib:json"`, ...); any other
/// spec fails as unknown. This is the seam for exposing a subset of the modules to untrusted scripts.
///
/// # Errors
/// Fails when a module cannot be built.
pub fn install_allowed(lua: &Lua, allowed: &[&str]) -> mlua::Result<()> {
    install_filtered(lua, Some(allowed))
}

/// How a module's table is built.
type Build = fn(&Lua) -> mlua::Result<Table>;

/// Every module an app (or a worker) can `require`, by registry key. The one list: the engine's own
/// `require`, the apps' `require` and the `LuaCATS` definitions all follow it.
///
/// Left out on purpose: `lib:test` (a test harness that prints to stdout), `lib:compile` (builds standalone
/// executables) and `std:workers` (a process-wide shutdown signal): none fits a server-side app.
const REGISTRY: &[(&str, Build)] = &[
    // std:* — mirrors Rust standard library.
    ("std:collections", stdmod::collections::module),
    ("std:env", stdmod::env::module),
    ("std:fs", stdmod::fs::module),
    ("std:io", stdmod::io::module),
    ("std:net", stdmod::net::module),
    ("std:task", stdmod::task::module),
    ("std:thread", stdmod::thread::module),
    ("std:time", stdmod::time::module),
    // lib:* — blessed gap-fillers and our own custom modules.
    ("lib:archive", lib::archive::module),
    ("lib:args", lib::args::module),
    ("lib:base64", lib::base64::module),
    ("lib:cache", lib::cache::module),
    ("lib:chrono", lib::chrono::module),
    ("lib:compress", lib::compress::module),
    ("lib:crypto", lib::crypto::module),
    ("lib:csv", lib::csv::module),
    ("lib:diff", lib::diff::module),
    ("lib:glob", lib::glob::module),
    ("lib:hash", lib::hash::module),
    ("lib:hex", lib::hex::module),
    ("lib:hmac", lib::hmac::module),
    ("lib:html", lib::html::module),
    ("lib:http", lib::http::module),
    ("lib:ini", lib::ini::module),
    ("lib:json", lib::json::module),
    ("lib:log", lib::log::module),
    ("lib:mime", lib::mime::module),
    ("lib:pkey", lib::pkey::module),
    ("lib:random", lib::random::module),
    ("lib:regex", lib::regex::module),
    ("lib:semver", lib::semver::module),
    ("lib:template", lib::template::module),
    ("lib:tempfile", lib::tempfile::module),
    ("lib:text", lib::text::module),
    ("lib:unicode", lib::unicode::module),
    ("lib:url", lib::url::module),
    ("lib:vcr", lib::vcr::module),
    // vnd:* — vendored crates, named after the upstream crate.
    ("vnd:hyper", vnd::hyper::module),
    ("vnd:jsonschema", vnd::jsonschema::module),
    ("vnd:quick_xml", vnd::quick_xml::module),
    ("vnd:rusqlite", vnd::rusqlite::module),
    ("vnd:serde_json", vnd::serde_json::module),
    ("vnd:serde_yaml", vnd::serde_yaml::module),
    ("vnd:sqlx_mysql", vnd::sqlx_mysql::module),
    ("vnd:sqlx_postgres", vnd::sqlx_postgres::module),
    #[cfg(feature = "testcontainers")]
    ("vnd:testcontainers_modules", vnd::testcontainers_modules::module),
    ("vnd:toml", vnd::toml::module),
];

/// The registry keys (`"lib:json"`, ...), in registry order.
pub fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|(name, _)| *name)
}

/// Build the module `name` in `lua`, or `None` when there is no such module. Each call builds a new table:
/// the caller caches it, so that every `require` of one module sees the same handle.
///
/// # Errors
/// Fails when the module cannot be built.
pub fn build(lua: &Lua, name: &str) -> Option<mlua::Result<Table>> {
    REGISTRY.iter().find(|(key, _)| *key == name).map(|(_, build)| build(lua))
}

/// The error `require(name)` raises for a name that is not a module: what was meant, and what exists.
#[must_use]
pub fn unknown_module(name: &str, known: &[&str]) -> mlua::Error {
    let ns = name.find(':').map(|pos| &name[..pos]);

    // Bare names get suffix matching first — `require("json")` should land on `lib:json` even though
    // their Levenshtein distance (4) is far past any sensible threshold.
    let suggestion = if ns.is_some() {
        let threshold = max(1, name.len() / 3);
        find_closest(name, known, threshold).map(String::from)
    } else {
        find_by_suffix(name, known).map(String::from)
    };

    let mut msg = format!("require: unknown module {name:?}");
    if let Some(ref s) = suggestion {
        msg.push_str(&format!(" — did you mean {s:?}?"));
    }

    // Namespace catalogue: when the user named a namespace, list its members. When they didn't and we have
    // no suggestion to offer, fall back to the three namespace prefixes — dumping every module helps nobody.
    let available = match ns {
        Some(ns) => {
            let prefix = format!("{ns}:");
            let mut members: Vec<&str> = known.iter().filter(|k| k.starts_with(&prefix)).copied().collect();
            members.sort_unstable();
            (!members.is_empty()).then(|| truncate_list(&members, MAX_AVAILABLE_LIST))
        }
        None if suggestion.is_none() => Some("std:<name>, lib:<name>, vnd:<name>".to_string()),
        None => None,
    };
    if let Some(list) = available {
        msg.push_str(&format!(" available: {list}"));
    }
    mlua::Error::external(msg)
}

fn install_filtered(lua: &Lua, allowed: Option<&[&str]>) -> mlua::Result<()> {
    let mut modules: HashMap<String, Table> = HashMap::new();
    for (name, build) in REGISTRY {
        if allowed.is_none_or(|allowed| allowed.contains(name)) {
            modules.insert((*name).to_string(), build(lua)?);
        }
    }

    // Custom require — strict `namespace:name` form, no path search, no
    // file I/O. Unknown specs error with the namespace catalogue so typos
    // surface immediately.
    let module_names: Vec<String> = modules.keys().cloned().collect();
    let require_fn = lua.create_function(move |_, name: String| -> mlua::Result<Table> {
        modules.get(&name).cloned().ok_or_else(|| {
            let known: Vec<&str> = module_names.iter().map(String::as_str).collect();
            unknown_module(&name, &known)
        })
    })?;
    lua.globals().set("require", require_fn)?;
    Ok(())
}

/// Declares every module through the facade, in registry order, so each one's recorded surface can be read.
///
/// Mirrors [`REGISTRY`] (a test holds the two together): a module appears here by its `declare`, which both
/// builds its table and records its signatures, docs and types.
fn declared<'l>(lua: &'l Lua) -> mlua::Result<Vec<Module<'l>>> {
    let mut out = Vec::new();
    let mut push = |m: mlua::Result<Module<'l>>| -> mlua::Result<()> {
        out.push(m?);
        Ok(())
    };

    push(stdmod::collections::declare(lua))?;
    push(stdmod::env::declare(lua))?;
    push(stdmod::fs::declare(lua))?;
    push(stdmod::io::declare(lua))?;
    push(stdmod::net::declare(lua))?;
    push(stdmod::task::declare(lua))?;
    push(stdmod::thread::declare(lua))?;
    push(stdmod::time::declare(lua))?;

    push(lib::archive::declare(lua))?;
    push(lib::args::declare(lua))?;
    push(lib::base64::declare(lua))?;
    push(lib::cache::declare(lua))?;
    push(lib::chrono::declare(lua))?;
    push(lib::compress::declare(lua))?;
    push(lib::crypto::declare(lua))?;
    push(lib::csv::declare(lua))?;
    push(lib::diff::declare(lua))?;
    push(lib::glob::declare(lua))?;
    push(lib::hash::declare(lua))?;
    push(lib::hex::declare(lua))?;
    push(lib::hmac::declare(lua))?;
    push(lib::html::declare(lua))?;
    push(lib::http::declare(lua))?;
    push(lib::ini::declare(lua))?;
    push(lib::json::declare(lua))?;
    push(lib::log::declare(lua))?;
    push(lib::mime::declare(lua))?;
    push(lib::pkey::declare(lua))?;
    push(lib::random::declare(lua))?;
    push(lib::regex::declare(lua))?;
    push(lib::semver::declare(lua))?;
    push(lib::template::declare(lua))?;
    push(lib::tempfile::declare(lua))?;
    push(lib::text::declare(lua))?;
    push(lib::unicode::declare(lua))?;
    push(lib::url::declare(lua))?;
    push(lib::vcr::declare(lua))?;

    push(vnd::hyper::declare(lua))?;
    push(vnd::jsonschema::declare(lua))?;
    push(vnd::quick_xml::declare(lua))?;
    push(vnd::rusqlite::declare(lua))?;
    push(vnd::serde_json::declare(lua))?;
    push(vnd::serde_yaml::declare(lua))?;
    push(vnd::sqlx_mysql::declare(lua))?;
    push(vnd::sqlx_postgres::declare(lua))?;
    #[cfg(feature = "testcontainers")]
    push(vnd::testcontainers_modules::declare(lua))?;
    push(vnd::toml::declare(lua))?;

    Ok(out)
}

/// The `LuaCATS` definition file of every module, as `(registry_key, text)` in registry order.
///
/// The text is a `---@meta` file for lua-language-server (`bun run codegen` writes it to
/// `types/generated/luals/<key>.lua`, so `require("lib:json")` resolves to it): each function's parameters,
/// returns and docs, and the resource types it hands out.
///
/// # Errors
/// Fails when a module cannot be built.
pub fn luals_metas(lua: &Lua) -> mlua::Result<Vec<(String, String)>> {
    declared(lua)?.iter().map(|m| Ok((m.name().to_string(), m.luals_meta()?))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::sandbox::make_engine;

    /// `declared` follows the registry: the same modules, in the same order. A module added to one list and
    /// not the other fails here.
    #[test]
    fn declared_follows_the_registry() {
        let lua = make_engine().expect("engine");
        let declared: Vec<String> =
            declared(&lua).expect("declared").iter().map(|m| m.name().to_string()).collect();
        let registry: Vec<String> = names().map(String::from).collect();
        assert_eq!(declared, registry);
        let expected = if cfg!(feature = "testcontainers") { 47 } else { 46 };
        assert_eq!(registry.len(), expected, "expected {expected} modules, got {registry:?}");
    }

    /// Every module yields a definition file: `---@meta`, a module class named after its key, the
    /// `require` return, and valid Lua 5.1 syntax (lua-language-server drops a file it cannot parse).
    #[test]
    fn every_module_yields_valid_definitions() {
        let lua = make_engine().expect("engine");
        let metas = luals_metas(&lua).expect("metas");
        assert_eq!(metas.len(), names().count());
        for (key, meta) in &metas {
            let class = key.replace(':', "_");
            assert!(meta.starts_with("---@meta\n"), "{key}: not a meta file");
            assert!(meta.contains(&format!("---@class {class}\n")), "{key}: no module class {class}");
            assert!(
                meta.ends_with(&format!("---@type {class}\nreturn M\n")),
                "{key}: does not return the module"
            );
            lua.load(meta.as_str())
                .set_name(key)
                .into_function()
                .unwrap_or_else(|e| panic!("{key}: {e}\n{meta}"));
        }
    }

    /// Every type a definition file names is a primitive or a class the same file declares: a recorded
    /// type name with no class would be an unknown type in lua-language-server.
    #[test]
    fn every_named_type_is_declared() {
        let lua = make_engine().expect("engine");
        for (key, meta) in luals_metas(&lua).expect("metas") {
            let declared: Vec<&str> = meta.lines().filter_map(|l| l.strip_prefix("---@class ")).collect();
            for line in meta.lines() {
                let Some(rest) = line.strip_prefix("---@param ").or_else(|| line.strip_prefix("---@return "))
                else {
                    continue;
                };
                let ty =
                    rest.split_whitespace().nth(usize::from(line.starts_with("---@param "))).unwrap_or("");
                let ty = ty.trim_end_matches('?');
                let known = matches!(ty, "string" | "number" | "boolean" | "any" | "nil" | "err")
                    || declared.contains(&ty);
                assert!(known, "{key}: `{line}` names `{ty}`, which no class declares");
            }
        }
    }

    /// Builds every facade-migrated module's runtime `require(...)` table,
    /// paired with its registry key, in the same order as [`super::luals_metas`].
    /// The two lists line up by index so a test can cross-check a module's
    /// live surface against its emitted definitions.
    fn runtime_tables(lua: &Lua) -> Vec<(String, Table)> {
        macro_rules! tables {
            ($($key:literal => $module:expr),+ $(,)?) => {
                vec![$(($key.to_string(), $module.expect($key))),+]
            };
        }
        #[allow(unused_mut)]
        let mut all = tables! {
            "std:collections" => stdmod::collections::module(lua),
            "std:env" => stdmod::env::module(lua),
            "std:fs" => stdmod::fs::module(lua),
            "std:io" => stdmod::io::module(lua),
            "std:net" => stdmod::net::module(lua),
            "std:task" => stdmod::task::module(lua),
            "std:thread" => stdmod::thread::module(lua),
            "std:time" => stdmod::time::module(lua),
            "lib:archive" => lib::archive::module(lua),
            "lib:args" => lib::args::module(lua),
            "lib:base64" => lib::base64::module(lua),
            "lib:cache" => lib::cache::module(lua),
            "lib:chrono" => lib::chrono::module(lua),
            "lib:compress" => lib::compress::module(lua),
            "lib:crypto" => lib::crypto::module(lua),
            "lib:csv" => lib::csv::module(lua),
            "lib:diff" => lib::diff::module(lua),
            "lib:glob" => lib::glob::module(lua),
            "lib:hash" => lib::hash::module(lua),
            "lib:hex" => lib::hex::module(lua),
            "lib:hmac" => lib::hmac::module(lua),
            "lib:html" => lib::html::module(lua),
            "lib:http" => lib::http::module(lua),
            "lib:ini" => lib::ini::module(lua),
            "lib:json" => lib::json::module(lua),
            "lib:log" => lib::log::module(lua),
            "lib:mime" => lib::mime::module(lua),
            "lib:pkey" => lib::pkey::module(lua),
            "lib:random" => lib::random::module(lua),
            "lib:regex" => lib::regex::module(lua),
            "lib:semver" => lib::semver::module(lua),
            "lib:template" => lib::template::module(lua),
            "lib:tempfile" => lib::tempfile::module(lua),
            "lib:text" => lib::text::module(lua),
            "lib:unicode" => lib::unicode::module(lua),
            "lib:url" => lib::url::module(lua),
            "lib:vcr" => lib::vcr::module(lua),
            "vnd:hyper" => vnd::hyper::module(lua),
            "vnd:jsonschema" => vnd::jsonschema::module(lua),
            "vnd:quick_xml" => vnd::quick_xml::module(lua),
            "vnd:rusqlite" => vnd::rusqlite::module(lua),
            "vnd:serde_json" => vnd::serde_json::module(lua),
            "vnd:serde_yaml" => vnd::serde_yaml::module(lua),
            "vnd:sqlx_mysql" => vnd::sqlx_mysql::module(lua),
            "vnd:sqlx_postgres" => vnd::sqlx_postgres::module(lua),
            "vnd:toml" => vnd::toml::module(lua),
        };
        #[cfg(feature = "testcontainers")]
        all.push((
            "vnd:testcontainers_modules".to_string(),
            vnd::testcontainers_modules::module(lua).expect("vnd:testcontainers_modules"),
        ));
        all
    }

    /// Every top-level key on a module's live `require(...)` table must be
    /// covered by that module's definitions. This is the guard that turns a
    /// missing `record_fn` into a failure: register a free function with
    /// `set` but forget to record its signature and the key is present at
    /// runtime yet absent from the definition file, so this fails.
    ///
    /// The check is one-directional (live keys ⊆ definitions): the definitions
    /// may carry extra entries the base table lacks — most notably
    /// `std:thread`'s `parent` / `shared`, injected only inside a spawned
    /// worker — and that is fine. The shape of the assertion per key:
    ///
    /// - a function value must appear as `function M.<key>(`;
    /// - a table value (a constructor sub-table such as `chrono.Date`) must
    ///   appear as a `---@field <key> ` on the module class;
    /// - any other value (a former const such as `json.null`) must appear as
    ///   a `---@field <key> `.
    #[test]
    fn definitions_cover_runtime_table_keys() {
        let lua = make_engine().expect("engine");
        let metas = luals_metas(&lua).expect("metas");
        let tables = runtime_tables(&lua);

        assert_eq!(metas.len(), tables.len(), "definitions and runtime tables drifted out of sync");

        for ((mkey, meta), (tkey, table)) in metas.iter().zip(tables.iter()) {
            assert_eq!(mkey, tkey, "definition / table ordering drifted");

            for pair in table.clone().pairs::<String, mlua::Value>() {
                let (key, value) = pair.expect("module table pair");

                let needle = match value {
                    mlua::Value::Function(_) => format!("function M.{key}("),
                    _ => format!("---@field {key} "),
                };
                assert!(
                    meta.contains(&needle),
                    "{mkey}: key {key:?} has no `{needle}` in its definitions:\n{meta}"
                );
            }
        }
    }

    #[test]
    fn find_closest_returns_within_threshold() {
        let candidates = ["lib:json", "lib:base64"];
        assert_eq!(find_closest("lib:jsn", &candidates, 2), Some("lib:json"));
    }

    #[test]
    fn find_closest_returns_none_when_distance_exceeds_threshold() {
        let candidates = ["lib:json", "std:fs"];
        assert_eq!(find_closest("json", &candidates, 1), None);
    }

    #[test]
    fn find_closest_excludes_exact_match() {
        let candidates = ["lib:json"];
        assert_eq!(find_closest("lib:json", &candidates, 2), None);
    }

    #[test]
    fn find_closest_prefers_closer_distance() {
        // "lib:json" → "lib:jsonz" is distance 1 (append).
        // "lib:json" → "lib:jsno"  is distance 2 (two substitutions on the tail).
        let candidates = ["lib:jsno", "lib:jsonz"];
        assert_eq!(find_closest("lib:json", &candidates, 2), Some("lib:jsonz"));
    }

    #[test]
    fn find_closest_ties_break_alphabetically() {
        let candidates = ["lib:bbb", "lib:aaa"];
        // Both at the same distance from "lib:zzz" — alphabetical wins.
        assert_eq!(find_closest("lib:zzz", &candidates, 5), Some("lib:aaa"));
    }

    #[test]
    fn find_by_suffix_picks_namespace_match() {
        let candidates = ["lib:json", "std:fs", "vnd:hyper"];
        assert_eq!(find_by_suffix("json", &candidates), Some("lib:json"));
    }

    #[test]
    fn find_by_suffix_returns_none_for_empty_name() {
        let candidates = ["lib:json"];
        assert_eq!(find_by_suffix("", &candidates), None);
    }

    #[test]
    fn find_by_suffix_picks_shortest_when_multiple_match() {
        let candidates = ["foo:json", "longer:json"];
        assert_eq!(find_by_suffix("json", &candidates), Some("foo:json"));
    }

    #[test]
    fn find_by_suffix_no_match_returns_none() {
        let candidates = ["lib:json", "std:fs"];
        assert_eq!(find_by_suffix("nope", &candidates), None);
    }

    #[test]
    fn truncate_list_inlines_when_within_cap() {
        let members = ["lib:a", "lib:b", "lib:c"];
        assert_eq!(truncate_list(&members, 3), "lib:a, lib:b, lib:c");
    }

    #[test]
    fn truncate_list_summarises_overflow_without_dropping_to_empty() {
        let members = ["lib:a", "lib:b", "lib:c", "lib:d"];
        assert_eq!(truncate_list(&members, 2), "lib:a, lib:b (+2 more)");
    }

    #[test]
    fn require_unknown_bare_name_suggests_via_suffix() {
        let lua = Lua::new();
        install(&lua).expect("install");
        let err = lua.load(r#"return require("json")"#).exec().expect_err("must fail");
        let msg = format!("{err}");
        assert!(msg.contains(r#"did you mean "lib:json"?"#), "expected suffix-match suggestion, got: {msg}");
    }

    #[test]
    fn require_unknown_bare_name_hints_namespace_prefixes_when_no_match() {
        let lua = Lua::new();
        install(&lua).expect("install");
        let err = lua.load(r#"return require("zzznonsense")"#).exec().expect_err("must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("available: std:<name>, lib:<name>, vnd:<name>"),
            "expected namespace-prefix hint, got: {msg}"
        );
    }

    #[test]
    fn require_unknown_namespaced_name_lists_namespace_members() {
        let lua = Lua::new();
        install(&lua).expect("install");
        let err = lua.load(r#"return require("lib:nonsense")"#).exec().expect_err("must fail");
        let msg = format!("{err}");
        assert!(msg.contains("available: lib:"), "expected lib:* catalogue, got: {msg}");
        // Must not leak modules from other namespaces.
        assert!(!msg.contains("std:fs"), "leaked std catalogue into lib error: {msg}");
    }
}
