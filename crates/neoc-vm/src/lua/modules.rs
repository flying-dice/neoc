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
/// grows. Bumped past the largest current namespace (`lib:*`, 21) so adding a
/// module doesn't silently suppress the list.
const MAX_AVAILABLE_LIST: usize = 30;

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
    modules.insert("std:time".into(), stdmod::time::module(lua)?);
    modules.insert("std:workers".into(), stdmod::workers::module(lua)?);

    // lib:* — blessed gap-fillers and our own custom modules.
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

/// Collects the emitted `.neoc` header for every module that declares its
/// surface through the facade.
///
/// Each entry is `(registry_key, header_text)` — for example
/// `("lib:base64", "// @generated …\nextern {\n    #[extern(path = \"lib:base64\")]\n    mod base64 { … }\n}\n")`.
/// Only modules migrated to the facade appear here; the rest still register
/// through a hand-written `module()` and ship a hand-written `.d.luau`.
pub fn headers(lua: &Lua) -> mlua::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut push = |m: mlua::Result<crate::lua::facade::Module<'_>>| -> mlua::Result<()> {
        let m = m?;
        out.push((m.name().to_string(), m.neoc_header()));
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
    push(stdmod::workers::declare(lua))?;

    push(lib::args::declare(lua))?;
    push(lib::base64::declare(lua))?;
    push(lib::cache::declare(lua))?;
    push(lib::chrono::declare(lua))?;
    push(lib::compile::declare(lua))?;
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
    push(lib::test::declare(lua))?;
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
    push(vnd::testcontainers_modules::declare(lua))?;
    push(vnd::toml::declare(lua))?;

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::sandbox::make_engine;

    /// Every facade-migrated module reports an `extern { … }` header keyed by a
    /// `#[extern(path = "<key>")] mod <name>` declaration. Guards the facade's
    /// extern block shape and the collector wiring; the count tracks the modules
    /// migrated so far (the rest follow in later batches).
    #[test]
    fn headers_cover_migrated_modules() {
        let lua = make_engine().expect("engine");
        let headers = super::headers(&lua).expect("headers");

        let keys: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"lib:args"));
        assert!(keys.contains(&"lib:base64"));
        assert!(keys.contains(&"lib:cache"));
        assert!(keys.contains(&"lib:chrono"));
        assert!(keys.contains(&"lib:compile"));
        assert!(keys.contains(&"lib:compress"));
        assert!(keys.contains(&"lib:crypto"));
        assert!(keys.contains(&"lib:csv"));
        assert!(keys.contains(&"lib:diff"));
        assert!(keys.contains(&"lib:glob"));
        assert!(keys.contains(&"lib:hash"));
        assert!(keys.contains(&"lib:hex"));
        assert!(keys.contains(&"lib:hmac"));
        assert!(keys.contains(&"lib:html"));
        assert!(keys.contains(&"lib:http"));
        assert!(keys.contains(&"lib:ini"));
        assert!(keys.contains(&"lib:json"));
        assert!(keys.contains(&"lib:log"));
        assert!(keys.contains(&"lib:mime"));
        assert!(keys.contains(&"lib:pkey"));
        assert!(keys.contains(&"lib:random"));
        assert!(keys.contains(&"lib:regex"));
        assert!(keys.contains(&"lib:semver"));
        assert!(keys.contains(&"lib:template"));
        assert!(keys.contains(&"lib:tempfile"));
        assert!(keys.contains(&"lib:test"));
        assert!(keys.contains(&"lib:text"));
        assert!(keys.contains(&"lib:unicode"));
        assert!(keys.contains(&"lib:url"));
        assert!(keys.contains(&"lib:vcr"));
        assert!(keys.contains(&"std:collections"));
        assert!(keys.contains(&"std:env"));
        assert!(keys.contains(&"std:fs"));
        assert!(keys.contains(&"std:io"));
        assert!(keys.contains(&"std:net"));
        assert!(keys.contains(&"std:task"));
        assert!(keys.contains(&"std:thread"));
        assert!(keys.contains(&"std:time"));
        assert!(keys.contains(&"std:workers"));
        assert!(keys.contains(&"vnd:hyper"));
        assert!(keys.contains(&"vnd:jsonschema"));
        assert!(keys.contains(&"vnd:quick_xml"));
        assert!(keys.contains(&"vnd:rusqlite"));
        assert!(keys.contains(&"vnd:serde_json"));
        assert!(keys.contains(&"vnd:serde_yaml"));
        assert!(keys.contains(&"vnd:sqlx_mysql"));
        assert!(keys.contains(&"vnd:sqlx_postgres"));
        assert!(keys.contains(&"vnd:testcontainers_modules"));
        assert!(keys.contains(&"vnd:toml"));
        assert_eq!(keys.len(), 49, "expected 49 migrated modules, got {keys:?}");

        for (key, header) in &headers {
            let name = key.split_once(':').map_or(key.as_str(), |(_, s)| s);
            assert!(
                header.contains("extern {"),
                "{key} header missing `extern {{` block:\n{header}"
            );
            assert!(
                header.contains(&format!("#[extern(path = \"{key}\")]")),
                "{key} header missing #[extern(path = …)]:\n{header}"
            );
            assert!(
                header.contains(&format!("mod {name} {{")),
                "{key} header missing `mod {name} {{`:\n{header}"
            );
        }
    }

    /// Builds every facade-migrated module's runtime `require(...)` table,
    /// paired with its registry key, in the same order as [`super::headers`].
    /// The two lists line up by index so a test can cross-check a module's
    /// live surface against its emitted header.
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
    /// covered by that module's emitted header. This is the guard that turns a
    /// missing `record_fn` into a CI failure: register a free function with
    /// `set` but forget to record its signature and the key is present at
    /// runtime yet absent from the `mod` block, so this fails.
    ///
    /// The check is one-directional (live keys ⊆ header surface): the header
    /// may carry extra entries the base table lacks — most notably
    /// `std:thread`'s `parent` / `shared`, injected only inside a spawned
    /// worker — and that is fine. The shape of the assertion per key:
    ///
    /// - a function value must appear as a free `fn <key>(` in the `mod` block;
    /// - a table value (a constructor sub-table such as `chrono.Date`) must
    ///   appear as `struct <Key>;`;
    /// - any other value (a former const such as `json.null`) must appear as a
    ///   zero-argument accessor `fn <key>()`.
    #[test]
    fn header_surface_covers_runtime_table_keys() {
        let lua = make_engine().expect("engine");
        let headers = super::headers(&lua).expect("headers");
        let tables = runtime_tables(&lua);

        assert_eq!(
            headers.len(),
            tables.len(),
            "headers and runtime tables drifted out of sync"
        );

        for ((hkey, header), (tkey, table)) in headers.iter().zip(tables.iter()) {
            assert_eq!(hkey, tkey, "header / table ordering drifted");

            for pair in table.clone().pairs::<String, mlua::Value>() {
                let (key, value) = pair.expect("module table pair");

                match value {
                    mlua::Value::Function(_) => {
                        // Free functions and former-const accessors sit directly
                        // in the `mod` at one indent level (eight spaces); a
                        // same-named method inside an `impl` is indented deeper
                        // (twelve spaces), so the leading indent disambiguates.
                        let needle = format!("        fn {key}(");
                        assert!(
                            header.contains(&needle),
                            "{hkey}: function key {key:?} has no free `fn {key}(` in its header surface:\n{header}"
                        );
                    }
                    mlua::Value::Table(_) => {
                        let needle = format!("struct {key};");
                        assert!(
                            header.contains(&needle),
                            "{hkey}: sub-table key {key:?} has no `{needle}` in its header surface:\n{header}"
                        );
                    }
                    _ => {
                        // A former module constant lowers to a zero-argument
                        // accessor `fn <key>()` (the extern item set has no
                        // `const`).
                        let needle = format!("        fn {key}()");
                        assert!(
                            header.contains(&needle),
                            "{hkey}: value key {key:?} has no accessor `fn {key}()` in its header surface:\n{header}"
                        );
                    }
                }
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
