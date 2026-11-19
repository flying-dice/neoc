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

/// Builds every module table, keyed by its `require` name.
fn registry(lua: &Lua) -> mlua::Result<HashMap<String, Table>> {
    let mut modules: HashMap<String, Table> = HashMap::new();

    // std:* — mirrors Rust standard library.
    modules.insert("std:collections".into(), stdmod::collections::module(lua)?);
    modules.insert("std:env".into(), stdmod::env::module(lua)?);
    modules.insert("std:fs".into(), stdmod::fs::module(lua)?);
    modules.insert("std:io".into(), stdmod::io::module(lua)?);
    modules.insert("std:net".into(), stdmod::net::module(lua)?);
    modules.insert("std:task".into(), stdmod::task::module(lua)?);
    modules.insert("std:thread".into(), stdmod::thread::module(lua)?);
    modules.insert("std:time".into(), stdmod::time::module(lua)?);
    modules.insert("std:workers".into(), stdmod::workers::module(lua)?);

    // lib:* — blessed gap-fillers and our own custom modules.
    modules.insert("lib:archive".into(), lib::archive::module(lua)?);
    modules.insert("lib:args".into(), lib::args::module(lua)?);
    modules.insert("lib:base64".into(), lib::base64::module(lua)?);
    modules.insert("lib:cache".into(), lib::cache::module(lua)?);
    modules.insert("lib:chrono".into(), lib::chrono::module(lua)?);
    modules.insert("lib:compile".into(), lib::compile::module(lua)?);
    modules.insert("lib:compress".into(), lib::compress::module(lua)?);
    modules.insert("lib:crypto".into(), lib::crypto::module(lua)?);
    modules.insert("lib:csv".into(), lib::csv::module(lua)?);
    modules.insert("lib:diff".into(), lib::diff::module(lua)?);
    modules.insert("lib:glob".into(), lib::glob::module(lua)?);
    modules.insert("lib:hash".into(), lib::hash::module(lua)?);
    modules.insert("lib:hex".into(), lib::hex::module(lua)?);
    modules.insert("lib:hmac".into(), lib::hmac::module(lua)?);
    modules.insert("lib:html".into(), lib::html::module(lua)?);
    modules.insert("lib:http".into(), lib::http::module(lua)?);
    modules.insert("lib:ini".into(), lib::ini::module(lua)?);
    modules.insert("lib:json".into(), lib::json::module(lua)?);
    modules.insert("lib:log".into(), lib::log::module(lua)?);
    modules.insert("lib:mime".into(), lib::mime::module(lua)?);
    modules.insert("lib:pkey".into(), lib::pkey::module(lua)?);
    modules.insert("lib:random".into(), lib::random::module(lua)?);
    modules.insert("lib:regex".into(), lib::regex::module(lua)?);
    modules.insert("lib:semver".into(), lib::semver::module(lua)?);
    modules.insert("lib:template".into(), lib::template::module(lua)?);
    modules.insert("lib:tempfile".into(), lib::tempfile::module(lua)?);
    modules.insert("lib:test".into(), lib::test::module(lua)?);
    modules.insert("lib:text".into(), lib::text::module(lua)?);
    modules.insert("lib:unicode".into(), lib::unicode::module(lua)?);
    modules.insert("lib:url".into(), lib::url::module(lua)?);
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

    Ok(modules)
}

pub fn install(lua: &Lua) -> mlua::Result<()> {
    let modules = registry(lua)?;

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
            // fall back to the three namespace prefixes — dumping all 50
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
                    (!members.is_empty()).then(|| truncate_list(&members, MAX_AVAILABLE_LIST))
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
    use crate::lua::sandbox::make_engine;

    /// Declares every facade module, in registry order. The recorded surface
    /// stays available to tooling; tests use it to check that the recording and
    /// the live tables agree.
    fn declared(lua: &Lua) -> mlua::Result<Vec<crate::lua::facade::Module<'_>>> {
        Ok(vec![
            stdmod::collections::declare(lua)?,
            stdmod::env::declare(lua)?,
            stdmod::fs::declare(lua)?,
            stdmod::io::declare(lua)?,
            stdmod::net::declare(lua)?,
            stdmod::task::declare(lua)?,
            stdmod::thread::declare(lua)?,
            stdmod::time::declare(lua)?,
            stdmod::workers::declare(lua)?,
            lib::archive::declare(lua)?,
            lib::args::declare(lua)?,
            lib::base64::declare(lua)?,
            lib::cache::declare(lua)?,
            lib::chrono::declare(lua)?,
            lib::compile::declare(lua)?,
            lib::compress::declare(lua)?,
            lib::crypto::declare(lua)?,
            lib::csv::declare(lua)?,
            lib::diff::declare(lua)?,
            lib::glob::declare(lua)?,
            lib::hash::declare(lua)?,
            lib::hex::declare(lua)?,
            lib::hmac::declare(lua)?,
            lib::html::declare(lua)?,
            lib::http::declare(lua)?,
            lib::ini::declare(lua)?,
            lib::json::declare(lua)?,
            lib::log::declare(lua)?,
            lib::mime::declare(lua)?,
            lib::pkey::declare(lua)?,
            lib::random::declare(lua)?,
            lib::regex::declare(lua)?,
            lib::semver::declare(lua)?,
            lib::template::declare(lua)?,
            lib::tempfile::declare(lua)?,
            lib::test::declare(lua)?,
            lib::text::declare(lua)?,
            lib::unicode::declare(lua)?,
            lib::url::declare(lua)?,
            lib::vcr::declare(lua)?,
            vnd::hyper::declare(lua)?,
            vnd::jsonschema::declare(lua)?,
            vnd::quick_xml::declare(lua)?,
            vnd::rusqlite::declare(lua)?,
            vnd::serde_json::declare(lua)?,
            vnd::serde_yaml::declare(lua)?,
            vnd::sqlx_mysql::declare(lua)?,
            vnd::sqlx_postgres::declare(lua)?,
            vnd::testcontainers_modules::declare(lua)?,
            vnd::toml::declare(lua)?,
        ])
    }

    /// The declared (facade) set equals the registered `require` keys exactly:
    /// no duplicates in either list, none missing either way, 50 in all. Each
    /// key resolves to a table through `require`.
    #[test]
    fn declared_modules_match_registered_keys() {
        use std::collections::BTreeSet;

        let lua = make_engine().expect("engine");
        let declared = declared(&lua).expect("declared");
        let declared_names: Vec<&str> = declared.iter().map(|m| m.name()).collect();
        let declared_set: BTreeSet<&str> = declared_names.iter().copied().collect();
        assert_eq!(
            declared_set.len(),
            declared_names.len(),
            "duplicate declared module: {declared_names:?}"
        );

        let registered = registry(&lua).expect("registry");
        let registered_set: BTreeSet<&str> = registered.keys().map(String::as_str).collect();

        let undeclared: Vec<_> = registered_set.difference(&declared_set).collect();
        let unregistered: Vec<_> = declared_set.difference(&registered_set).collect();
        assert!(
            undeclared.is_empty(),
            "registered but not declared: {undeclared:?}"
        );
        assert!(
            unregistered.is_empty(),
            "declared but not registered: {unregistered:?}"
        );
        assert_eq!(registered_set.len(), 50);

        for key in &registered_set {
            let kind: String = lua
                .load(format!("return type(require({key:?}))"))
                .eval()
                .unwrap_or_else(|e| panic!("require({key:?}) failed: {e}"));
            assert_eq!(kind, "table", "{key} must be a table");
        }
    }

    /// Builds every facade-migrated module's runtime `require(...)` table,
    /// paired with its registry key, in the same order as [`declared`].
    /// The two lists line up by index so a test can cross-check a module's
    /// live surface against its recorded surface.
    fn runtime_tables(lua: &Lua) -> Vec<(String, Table)> {
        macro_rules! tables {
            ($($key:literal => $module:expr),+ $(,)?) => {
                vec![$(($key.to_string(), $module.expect($key))),+]
            };
        }
        tables! {
            "std:collections" => stdmod::collections::module(lua),
            "std:env" => stdmod::env::module(lua),
            "std:fs" => stdmod::fs::module(lua),
            "std:io" => stdmod::io::module(lua),
            "std:net" => stdmod::net::module(lua),
            "std:task" => stdmod::task::module(lua),
            "std:thread" => stdmod::thread::module(lua),
            "std:time" => stdmod::time::module(lua),
            "std:workers" => stdmod::workers::module(lua),
            "lib:archive" => lib::archive::module(lua),
            "lib:args" => lib::args::module(lua),
            "lib:base64" => lib::base64::module(lua),
            "lib:cache" => lib::cache::module(lua),
            "lib:chrono" => lib::chrono::module(lua),
            "lib:compile" => lib::compile::module(lua),
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
            "lib:test" => lib::test::module(lua),
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
            "vnd:testcontainers_modules" => vnd::testcontainers_modules::module(lua),
            "vnd:toml" => vnd::toml::module(lua),
        }
    }

    /// Every top-level key on a module's live `require(...)` table must be
    /// covered by that module's recorded surface. This is the guard that turns
    /// a missing `record_fn` into a CI failure: register a free function with
    /// `set` but forget to record its signature and the key is present at
    /// runtime yet absent from the recording, so this fails.
    ///
    /// The check is one-directional (live keys ⊆ recorded surface): the
    /// recording may carry extra entries the base table lacks — most notably
    /// `std:thread`'s `parent` / `shared`, injected only inside a spawned
    /// worker — and that is fine.
    #[test]
    fn recorded_surface_covers_runtime_table_keys() {
        let lua = make_engine().expect("engine");
        let declared = declared(&lua).expect("declared");
        let tables = runtime_tables(&lua);

        assert_eq!(
            declared.len(),
            tables.len(),
            "declared modules and runtime tables drifted out of sync"
        );

        for (module, (tkey, table)) in declared.iter().zip(tables.iter()) {
            assert_eq!(module.name(), tkey, "declared / table ordering drifted");
            let names = module.declared_names();

            for pair in table.clone().pairs::<String, mlua::Value>() {
                let (key, _) = pair.expect("module table pair");
                assert!(
                    names.contains(&key),
                    "{tkey}: key {key:?} is on the live table but not in its recorded surface"
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
