//! `require` registry — wires every module into the Lua engine and
//! exposes a custom `require("namespace:name")` resolver.
//!
//! Three namespaces, picked at module-registration time:
//!
//! - **`std:*`** — mirrors the Rust standard library (and tightly-scoped
//!   extensions on top of it). Anything you'd reach for in `std::` lives
//!   here: `std:collections`, `std:fs`, `std:io`, `std:net`, `std:thread`.
//!   `std:workers` is the tokio exception — worker-pool substrate, no
//!   `std::` equivalent.
//! - **`lib:*`** — blessed gap-fillers and our own custom modules.
//!   Either plugs a hole Rust std doesn't cover (`lib:base64`) or is
//!   something we wrote (`lib:test`, `lib:vcr`).
//! - **`vnd:*`** — vendored crates. Module name **must** match the
//!   upstream crate name so the import is unambiguous: `vnd:hyper`,
//!   `vnd:serde_json`, `vnd:rusqlite`, `vnd:toml`. No renaming, no
//!   ambiguity.
//!
//! Scripts pull modules in via `require`:
//!
//! ```lua
//! local fs   = require("std:fs")
//! local test = require("lib:test")
//! local json = require("vnd:serde_json")
//! ```
//!
//! `require` is the only way to reach a module — there is no ambient `lib`
//! global, no auto-imported `Schema`. Resolution is a flat
//! HashMap built once at engine boot; module tables are shared (all callers
//! get the same handle), so writing into one observably mutates it for
//! everyone — same contract as Lua's own require.

use std::cmp::max;
use std::collections::HashMap;

use mlua::{Lua, Table};
use strsim::levenshtein;

use crate::lua::{lib, std as stdmod, vnd};

/// Upper bound on the namespace catalogue we'll dump into a `require` error.
/// Keeps small namespaces actionable without paging the user when the catalogue
/// grows. Bumped past the largest current namespace (`vnd:*`, 10) so adding a
/// module doesn't silently suppress the list.
const MAX_AVAILABLE_LIST: usize = 20;

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

pub fn install(lua: &Lua) -> mlua::Result<()> {
    let mut modules: HashMap<String, Table> = HashMap::new();

    // std:* — mirrors Rust standard library.
    modules.insert("std:collections".into(), stdmod::collections::module(lua)?);
    modules.insert("std:env".into(), stdmod::env::module(lua)?);
    modules.insert("std:fs".into(), stdmod::fs::module(lua)?);
    modules.insert("std:io".into(), stdmod::io::module(lua)?);
    modules.insert("std:net".into(), stdmod::net::module(lua)?);
    modules.insert("std:task".into(), stdmod::task::module(lua)?);
    modules.insert("std:thread".into(), stdmod::thread::module(lua)?);
    modules.insert("std:workers".into(), stdmod::workers::module(lua)?);

    // lib:* — blessed gap-fillers and our own custom modules.
    modules.insert("lib:base64".into(), lib::base64::module(lua)?);
    modules.insert("lib:compile".into(), lib::compile::module(lua)?);
    modules.insert("lib:compress".into(), lib::compress::module(lua)?);
    modules.insert("lib:csv".into(), lib::csv::module(lua)?);
    modules.insert("lib:glob".into(), lib::glob::module(lua)?);
    modules.insert("lib:http".into(), lib::http::module(lua)?);
    modules.insert("lib:ini".into(), lib::ini::module(lua)?);
    modules.insert("lib:json".into(), lib::json::module(lua)?);
    modules.insert("lib:mime".into(), lib::mime::module(lua)?);
    modules.insert("lib:test".into(), lib::test::module(lua)?);
    modules.insert("lib:text".into(), lib::text::module(lua)?);
    modules.insert("lib:vcr".into(), lib::vcr::module(lua)?);

    // vnd:* — vendored crates, named after the upstream crate.
    modules.insert("vnd:hyper".into(), vnd::hyper::module(lua)?);
    modules.insert("vnd:jsonschema".into(), vnd::jsonschema::module(lua)?);
    modules.insert("vnd:quick_xml".into(), vnd::quick_xml::module(lua)?);
    modules.insert("vnd:rusqlite".into(), vnd::rusqlite::module(lua)?);
    modules.insert("vnd:serde_json".into(), vnd::serde_json::module(lua)?);
    modules.insert("vnd:serde_yaml".into(), vnd::serde_yaml::module(lua)?);
    modules.insert("vnd:sqlx_mysql".into(), vnd::sqlx_mysql::module(lua)?);
    modules.insert("vnd:sqlx_postgres".into(), vnd::sqlx_postgres::module(lua)?);
    modules.insert(
        "vnd:testcontainers_modules".into(),
        vnd::testcontainers_modules::module(lua)?,
    );
    modules.insert("vnd:toml".into(), vnd::toml::module(lua)?);

    // Custom require — strict `namespace:name` form, no path search, no
    // file I/O. Unknown specs error with the namespace catalogue so typos
    // surface immediately.
    let module_names: Vec<String> = modules.keys().cloned().collect();
    let require_fn = lua.create_function(move |_, name: String| -> mlua::Result<Table> {
        modules.get(&name).cloned().ok_or_else(|| {
            let keys: Vec<&str> = module_names.iter().map(|k| k.as_str()).collect();
            let ns = name.find(':').map(|pos| &name[..pos]);

            // Bare names get suffix matching first — `require("json")` should
            // land on `lib:json` even though their Levenshtein distance (4) is
            // far past any sensible threshold.
            let suggestion = if ns.is_some() {
                let threshold = max(1, name.len() / 3);
                find_closest(&name, &keys, threshold).map(String::from)
            } else {
                find_by_suffix(&name, &keys).map(String::from)
            };

            let mut msg = format!("require: unknown module {name:?}");
            if let Some(ref s) = suggestion {
                msg.push_str(&format!(" — did you mean {s:?}?"));
            }

            // Namespace catalogue: when the user named a namespace, list its
            // members. When they didn't and we have no suggestion to offer,
            // fall back to the three namespace prefixes — dumping all 25
            // modules helps nobody.
            let available = match ns {
                Some(ns) => {
                    let prefix = format!("{ns}:");
                    let mut members: Vec<&str> = keys
                        .iter()
                        .filter(|k| k.starts_with(&prefix))
                        .copied()
                        .collect();
                    members.sort_unstable();
                    (!members.is_empty() && members.len() <= MAX_AVAILABLE_LIST)
                        .then(|| members.join(", "))
                }
                None if suggestion.is_none() => {
                    Some("std:<name>, lib:<name>, vnd:<name>".to_string())
                }
                None => None,
            };
            if let Some(list) = available {
                msg.push_str(&format!(" available: {list}"));
            }
            mlua::Error::external(msg)
        })
    })?;
    lua.globals().set("require", require_fn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn require_unknown_bare_name_suggests_via_suffix() {
        let lua = Lua::new();
        install(&lua).expect("install");
        let err = lua
            .load(r#"return require("json")"#)
            .exec()
            .expect_err("must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains(r#"did you mean "lib:json"?"#),
            "expected suffix-match suggestion, got: {msg}"
        );
    }

    #[test]
    fn require_unknown_bare_name_hints_namespace_prefixes_when_no_match() {
        let lua = Lua::new();
        install(&lua).expect("install");
        let err = lua
            .load(r#"return require("zzznonsense")"#)
            .exec()
            .expect_err("must fail");
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
        let err = lua
            .load(r#"return require("lib:nonsense")"#)
            .exec()
            .expect_err("must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("available: lib:"),
            "expected lib:* catalogue, got: {msg}"
        );
        // Must not leak modules from other namespaces.
        assert!(
            !msg.contains("std:fs"),
            "leaked std catalogue into lib error: {msg}"
        );
    }
}
