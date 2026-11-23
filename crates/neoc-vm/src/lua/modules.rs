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
use std::sync::Arc;

use mlua::{Lua, Table};
use strsim::levenshtein;

use crate::lua::facade::{Module, luau_decls};
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

/// How a module's table is built.
type Build = fn(&Lua) -> mlua::Result<Table>;

/// Every module a script can `require`, by registry key. The one list: the engine's own `require`, an
/// embedder's `require` ([`names`], [`build`]) and the Luau declarations all follow it.
const REGISTRY: &[(&str, Build)] = &[
    // std:* — mirrors Rust standard library.
    ("std:collections", stdmod::collections::module),
    ("std:env", stdmod::env::module),
    ("std:fs", stdmod::fs::module),
    ("std:io", stdmod::io::module),
    ("std:net", stdmod::net::module),
    ("std:path", stdmod::path::module),
    ("std:task", stdmod::task::module),
    ("std:thread", stdmod::thread::module),
    ("std:time", stdmod::time::module),
    ("std:workers", stdmod::workers::module),
    // lib:* — blessed gap-fillers and our own custom modules.
    ("lib:agui", lib::agui::module),
    ("lib:archive", lib::archive::module),
    ("lib:args", lib::args::module),
    ("lib:base64", lib::base64::module),
    ("lib:cache", lib::cache::module),
    ("lib:chrono", lib::chrono::module),
    ("lib:compile", lib::compile::module),
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
    ("lib:test", lib::test::module),
    ("lib:text", lib::text::module),
    ("lib:unicode", lib::unicode::module),
    ("lib:url", lib::url::module),
    ("lib:vcr", lib::vcr::module),
    // vnd:* — vendored crates, named after the upstream crate.
    ("vnd:bollard", vnd::bollard::module),
    ("vnd:hyper", vnd::hyper::module),
    ("vnd:jsonschema", vnd::jsonschema::module),
    ("vnd:keyring", vnd::keyring::module),
    ("vnd:portable_pty", vnd::portable_pty::module),
    ("vnd:quick_xml", vnd::quick_xml::module),
    ("vnd:rusqlite", vnd::rusqlite::module),
    ("vnd:serde_json", vnd::serde_json::module),
    ("vnd:serde_yaml", vnd::serde_yaml::module),
    ("vnd:sqlx_mysql", vnd::sqlx_mysql::module),
    ("vnd:sqlx_postgres", vnd::sqlx_postgres::module),
    (
        "vnd:testcontainers_modules",
        vnd::testcontainers_modules::module,
    ),
    ("vnd:toml", vnd::toml::module),
];

/// The registry keys (`"lib:json"`, ...), in registry order.
pub fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|(name, _)| *name)
}

/// Build the module `name` in `lua`, or `None` when there is no such module. Each call builds a new table:
/// the caller caches it, so that every `require` of one module sees the same handle. This is the seam for an
/// embedder that wants its own `require`, over the subset of [`names`] it offers.
///
/// # Errors
/// Fails when the module cannot be built.
#[must_use]
pub fn build(lua: &Lua, name: &str) -> Option<mlua::Result<Table>> {
    REGISTRY
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, build)| build(lua))
}

/// The error `require(name)` raises for a name that is not a module: what was meant, and what exists.
#[must_use]
pub fn unknown_module(name: &str, known: &[&str]) -> mlua::Error {
    let ns = name.find(':').map(|pos| &name[..pos]);

    // Bare names get suffix matching first — `require("json")` should
    // land on `lib:json` even though their Levenshtein distance (4) is
    // far past any sensible threshold.
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

    // Namespace catalogue: when the user named a namespace, list its
    // members. When they didn't and we have no suggestion to offer,
    // fall back to the three namespace prefixes — dumping all 50
    // modules helps nobody.
    let available = match ns {
        Some(ns) => {
            let prefix = format!("{ns}:");
            let mut members: Vec<&str> = known
                .iter()
                .filter(|k| k.starts_with(&prefix))
                .copied()
                .collect();
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

/// Install the `require` global over every module.
///
/// # Errors
/// Fails when a module cannot be built.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    install_filtered(lua, None)
}

/// Install the `require` global over only the modules named in `allowed` (`"lib:json"`, ...); any other
/// spec fails as unknown. A seam for exposing a subset of the modules to a script. A `std:thread` worker the
/// script starts is built with the same subset (this installs the engine's
/// [worker engine](crate::lua::sandbox::set_worker_engine); call that afterwards to replace it).
///
/// # Errors
/// Fails when a module cannot be built.
pub fn install_allowed(lua: &Lua, allowed: &[&str]) -> mlua::Result<()> {
    install_filtered(lua, Some(allowed))?;
    let allowed: Vec<String> = allowed.iter().map(|name| (*name).to_owned()).collect();
    crate::lua::sandbox::set_worker_engine(
        lua,
        Arc::new(move |life| {
            let names: Vec<&str> = allowed.iter().map(String::as_str).collect();
            crate::lua::sandbox::make_engine_with(life, &names)
        }),
    );
    Ok(())
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

/// Declares every module through the facade, in registry order, so each one's
/// recorded surface can be read.
///
/// Mirrors [`REGISTRY`] (a test holds the two together): a module appears here
/// by its `declare`, which both builds its table and records its signatures,
/// docs and types.
fn declared(lua: &Lua) -> mlua::Result<Vec<Module<'_>>> {
    Ok(vec![
        stdmod::collections::declare(lua)?,
        stdmod::env::declare(lua)?,
        stdmod::fs::declare(lua)?,
        stdmod::io::declare(lua)?,
        stdmod::net::declare(lua)?,
        stdmod::path::declare(lua)?,
        stdmod::task::declare(lua)?,
        stdmod::thread::declare(lua)?,
        stdmod::time::declare(lua)?,
        stdmod::workers::declare(lua)?,
        lib::agui::declare(lua)?,
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
        vnd::bollard::declare(lua)?,
        vnd::hyper::declare(lua)?,
        vnd::jsonschema::declare(lua)?,
        vnd::keyring::declare(lua)?,
        vnd::portable_pty::declare(lua)?,
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

/// Where the Luau declarations go, relative to the repository root: one definitions file for every module.
/// A colon is not allowed in a Windows file name, so nothing is named after a registry key; the overloaded
/// `require` in the file maps `require("lib:json")` to its type.
pub const LUAU_DECLS_PATH: &str = "types/neoc.d.luau";

/// The Luau declarations of every module, as `(registry_key, section)` in registry order.
///
/// A section is the `export type` aliases of one module (its resource types and its table): each function's
/// parameters, returns and docs. [`luau_decls`] joins them into the file.
///
/// # Errors
/// Fails when a module cannot be built, or its recording disagrees with the
/// live table of one of its types.
pub fn luau_decl_sections(lua: &Lua) -> mlua::Result<Vec<(String, String)>> {
    declared(lua)?
        .iter()
        .map(|m| Ok((m.name().to_string(), m.luau_decls()?)))
        .collect()
}

/// The whole Luau definitions file (`cargo run -p neoc-vm --example luau_decls` writes it to
/// [`LUAU_DECLS_PATH`]): a header, the section of every module in registry order, then the `require`
/// overloads that make `require("lib:json")` have the type of that module in luau-lsp.
///
/// # Errors
/// As [`luau_decl_sections`].
pub fn luau_decls(lua: &Lua) -> mlua::Result<String> {
    let sections = luau_decl_sections(lua)?;
    let mut out = luau_decls::file_header();
    for (_, section) in &sections {
        out.push_str(section);
    }
    let keys: Vec<&str> = sections.iter().map(|(key, _)| key.as_str()).collect();
    out.push_str(&luau_decls::require_declaration(&keys));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::sandbox::make_engine;
    use mlua::Value;

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

        let registered_names: Vec<&str> = names().collect();
        let registered_set: BTreeSet<&str> = registered_names.iter().copied().collect();
        assert_eq!(
            registered_set.len(),
            registered_names.len(),
            "duplicate registered module: {registered_names:?}"
        );

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
        assert_eq!(registered_set.len(), 55);

        for key in &registered_set {
            let kind: String = lua
                .load(format!("return type(require({key:?}))"))
                .eval()
                .unwrap_or_else(|e| panic!("require({key:?}) failed: {e}"));
            assert_eq!(kind, "table", "{key} must be a table");
        }
    }

    /// The properties of the alias `name` in a section of the declarations, as `(property, type text)`:
    /// the lines with exactly one tab of indent between `export type <name> = {` and the closing brace. A
    /// doc comment line (`\t---`) and a line indented deeper are not properties, and a reserved word is the
    /// quoted key `["end"]`, read back as the bare name.
    fn alias_props(section: &str, name: &str) -> Vec<(String, String)> {
        let open = format!("export type {name} = {{");
        let mut lines = section.lines().skip_while(|l| *l != open);
        assert!(lines.next().is_some(), "no `{open}` in:\n{section}");
        lines
            .take_while(|l| *l != "}")
            .filter(|l| l.starts_with('\t') && !l.starts_with("\t\t") && !l.starts_with("\t---"))
            .map(|l| {
                let l = &l[1..];
                let (name, ty) = l.split_once(": ").expect("`name: type,`");
                let name = name
                    .strip_prefix("[\"")
                    .and_then(|n| n.strip_suffix("\"]"))
                    .unwrap_or(name);
                (name.to_string(), ty.trim_end_matches(',').to_string())
            })
            .collect()
    }

    /// A quoted key (a reserved word) is read back as the bare name; a doc line and a line indented deeper
    /// (a nested one) are not properties.
    #[test]
    fn alias_props_reads_quoted_keys() {
        let section = "-- ===== lib:x =====\n\nexport type lib_x = {\n\t--- doc: not a property\n\t[\"end\"]: number,\n\t\tnested line without a separator,\n\tf: (a: number) -> (),\n}\n\n";
        assert_eq!(
            alias_props(section, "lib_x"),
            [
                ("end".to_string(), "number".to_string()),
                ("f".to_string(), "(a: number) -> ()".to_string())
            ]
        );
    }

    /// Every module yields a section of the declarations: a header naming its key, an exported alias for its
    /// table named after the key, and an overload of `require` that returns that alias; and the whole file
    /// parses with the grammar of what the emitter writes (`facade::luau_decls::grammar`, which needs no
    /// luau-lsp: it refuses a control character, a tuple in a result, an unbalanced type, a stray line).
    #[test]
    fn every_module_yields_valid_declarations() {
        let lua = make_engine().expect("engine");
        let sections = luau_decl_sections(&lua).expect("sections");
        let file = luau_decls(&lua).expect("file");
        assert_eq!(sections.len(), 55);
        for (key, section) in &sections {
            let scope = luau_decls::scope(key);
            assert!(
                section.starts_with(&format!("-- ===== {key} =====\n")),
                "{key}: no header"
            );
            assert!(
                section.contains(&format!("\nexport type {scope} = {{")),
                "{key}: no module alias {scope}"
            );
            assert!(
                file.contains(&format!("((path: \"{key}\") -> {scope})")),
                "{key}: no `require` overload"
            );
        }
        assert_eq!(luau_decls::grammar::validate(&file), Ok(()));
        assert!(file.starts_with("-- GENERATED by neoc-vm"));
        assert!(file.contains("declare require: \n\t((path: \"std:collections\")"));
        assert_eq!(
            file.matches("((path: \"").count(),
            55,
            "one `require` overload per module"
        );
    }

    /// The identifiers of the text of the declarations in type position, as `(word, is_declaration)`:
    /// comments and strings are dropped, a word followed by a colon is a label (a property or parameter
    /// name) and not a type, and the word after `type` is a declaration.
    fn type_words(text: &str) -> Vec<(String, bool)> {
        let code: String = text
            .lines()
            .map(|l| l.split("--").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let mut stripped = String::new();
        let mut in_string = false;
        for c in code.chars() {
            if c == '"' {
                in_string = !in_string;
            } else if !in_string {
                stripped.push(c);
            }
        }
        let chars: Vec<char> = stripped.chars().collect();
        let mut words = Vec::new();
        let mut previous = String::new();
        let mut i = 0;
        while i < chars.len() {
            if !(chars[i].is_ascii_alphabetic() || chars[i] == '_') {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let label = chars[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
            if !label && !matches!(word.as_str(), "export" | "type" | "declare" | "require") {
                words.push((word.clone(), previous == "type"));
            }
            previous = word;
        }
        words
    }

    #[test]
    fn type_words_skip_labels_comments_and_strings() {
        let words: Vec<String> = type_words(
            "export type a_B = {\n\t--- a doc, with Words\n\t[\"end\"]: number,\n\tf: (self: a_B, x: c_D?) -> (string, ...any),\n}\ndeclare require: ((path: \"lib:json\") -> a_B)",
        )
        .into_iter()
        .map(|(w, declared)| if declared { format!("={w}") } else { w })
        .collect();
        assert_eq!(
            words,
            ["=a_B", "number", "a_B", "c_D", "string", "any", "a_B"]
        );
    }

    /// Every type the declarations name is a primitive or an alias some section declares (its own, or another
    /// module's such as `std_net_TcpListener`): a recorded type name with no alias would be an unknown type
    /// in luau-lsp, and the emitter turns any unknown name into `<module>_<Name>` without complaint.
    #[test]
    fn every_named_type_is_declared() {
        let lua = make_engine().expect("engine");
        let file = luau_decls(&lua).expect("file");
        let words = type_words(&file);
        let declared: Vec<&str> = words
            .iter()
            .filter(|(_, declaration)| *declaration)
            .map(|(w, _)| w.as_str())
            .collect();
        let mut checked = 0;
        for (word, declaration) in &words {
            if *declaration {
                continue;
            }
            let known = matches!(
                word.as_str(),
                "string" | "number" | "boolean" | "any" | "nil" | "buffer"
            ) || declared.contains(&word.as_str());
            assert!(
                known,
                "the declarations name `{word}`, which no alias declares"
            );
            checked += 1;
        }
        // a changed layout would make the scan skip every word and pass
        assert!(declared.len() > 100, "only {} aliases", declared.len());
        assert!(checked > 1000, "only {checked} type names checked");
    }

    /// One Lua expression per recorded resource type, building a value that has every field set, so the test below
    /// can hold the records to the implementation.
    const INSTANCES: &[(&str, &str, &str)] = &[
        (
            "lib:cache",
            "Cache",
            r#"require("lib:cache").Cache.new({ max_size = 2 })"#,
        ),
        (
            "lib:chrono",
            "Date",
            r#"require("lib:chrono").Date.new(2026, 5, 20)"#,
        ),
        (
            "lib:chrono",
            "Time",
            r#"require("lib:chrono").Time.new(1, 2, 3)"#,
        ),
        (
            "lib:chrono",
            "DateTime",
            r#"require("lib:chrono").DateTime.new(2026, 5, 20, 1, 2, 3)"#,
        ),
        (
            "lib:chrono",
            "ZonedDateTime",
            r#"require("lib:chrono").ZonedDateTime.parse("2026-05-20T01:02:03+01:00[Europe/London]")"#,
        ),
        (
            "lib:chrono",
            "Duration",
            r#"require("lib:chrono").Duration.of({ hours = 1 })"#,
        ),
        (
            "lib:csv",
            "CsvReader",
            r#"require("lib:csv").reader(require("std:io").from_string("a\n1\n"), { headers = true })"#,
        ),
        (
            "lib:csv",
            "CsvWriter",
            r#"require("lib:csv").writer({ headers = { "a" } })"#,
        ),
        (
            "lib:glob",
            "Glob",
            r#"require("lib:glob").Glob.new("*.lua")"#,
        ),
        (
            "lib:glob",
            "GlobSet",
            r#"require("lib:glob").GlobSet.new({ "*.lua" })"#,
        ),
        (
            "lib:hash",
            "Hasher",
            r#"require("lib:hash").Hasher.new("sha256")"#,
        ),
        (
            "lib:hmac",
            "Hmac",
            r#"require("lib:hmac").Hmac.new("sha256", "key")"#,
        ),
        ("lib:json", "Object", r#"require("lib:json").object()"#),
        ("lib:json", "Array", r#"require("lib:json").array()"#),
        ("lib:json", "JsonNull", r#"require("lib:json").null"#),
        ("lib:log", "Logger", r#"require("lib:log").new()"#),
        (
            "lib:pkey",
            "KeyPair",
            r#"require("lib:pkey").KeyPair.generate("ed25519")"#,
        ),
        (
            "lib:pkey",
            "PublicKey",
            r#"require("lib:pkey").KeyPair.generate("ed25519"):public_key()"#,
        ),
        (
            "lib:regex",
            "Pattern",
            r#"require("lib:regex").Pattern.new("a+")"#,
        ),
        (
            "lib:semver",
            "Version",
            r#"require("lib:semver").Version.parse("1.2.3-alpha.1+build.5")"#,
        ),
        (
            "lib:semver",
            "VersionReq",
            r#"require("lib:semver").VersionReq.parse(">=1.2")"#,
        ),
        ("lib:test", "Matcher", r#"require("lib:test").expect(1)"#),
        (
            "lib:tempfile",
            "TempFile",
            r#"require("lib:tempfile").TempFile.new()"#,
        ),
        (
            "lib:tempfile",
            "TempDir",
            r#"require("lib:tempfile").TempDir.new()"#,
        ),
        (
            "lib:template",
            "Template",
            r#"require("lib:template").compile("hello")"#,
        ),
        (
            "lib:url",
            "Url",
            r#"require("lib:url").Url.parse("https://u:p@example.com:8443/a/b?q=1#frag")"#,
        ),
        (
            "std:collections",
            "Map",
            r#"require("std:collections").Map.new()"#,
        ),
        (
            "std:collections",
            "Counter",
            r#"require("std:collections").Counter.new(1)"#,
        ),
        (
            "std:collections",
            "Mutex",
            r#"require("std:collections").Mutex.new()"#,
        ),
        (
            "std:fs",
            "Metadata",
            r#"assert(require("std:fs").metadata("."))"#,
        ),
        (
            "std:fs",
            "DirEntry",
            r#"assert(require("std:fs").read_dir("."))[1]"#,
        ),
        (
            "std:fs",
            "Watcher",
            r#"(function()
                local watcher = assert(require("std:fs").watch("."))
                return watcher, function() watcher:close() end
            end)()"#,
        ),
        (
            "std:io",
            "ReaderHandle",
            r#"require("std:io").from_string("x")"#,
        ),
        (
            "std:net",
            "TcpListener",
            r#"require("std:net").TcpListener.bind("127.0.0.1:0")"#,
        ),
        (
            "std:task",
            "Task",
            r#"require("std:task").spawn(function() end)"#,
        ),
        (
            "std:thread",
            "WorkerHandle",
            r#"require("std:thread").spawn({ source = "return 1" })"#,
        ),
        (
            "std:io",
            "WriterHandle",
            r#"(function()
                local dir = require("lib:tempfile").TempDir.new()
                local writer = assert(require("std:fs").open({ path = dir:path() .. "/w.txt", mode = "w" }))
                return writer, function() dir:close() end
            end)()"#,
        ),
        // a client sends nothing until its first call, and a stream's request is its task's: neither needs a daemon
        (
            "vnd:bollard",
            "Client",
            r#"require("vnd:bollard").connect({ host = "tcp://127.0.0.1:1" })"#,
        ),
        (
            "vnd:bollard",
            "Stream",
            r#"(function()
                local docker = assert(require("vnd:bollard").connect({ host = "tcp://127.0.0.1:1" }))
                local events = assert(docker:events())
                return events, function() events:close() end
            end)()"#,
        ),
        // a real terminal with a real process on it, ended by the cleanup
        (
            "vnd:portable_pty",
            "Pty",
            if cfg!(windows) {
                r#"(function()
                    local pty = assert(require("vnd:portable_pty").spawn_command({ program = "cmd.exe" }))
                    return pty, function() pty:close() end
                end)()"#
            } else {
                r#"(function()
                    local pty = assert(require("vnd:portable_pty").spawn_command({ program = "/bin/sh" }))
                    return pty, function() pty:close() end
                end)()"#
            },
        ),
        (
            "vnd:hyper",
            "ServerHandle",
            r#"(function()
                local listener = assert(require("std:net").TcpListener.bind("127.0.0.1:0"))
                local server = assert(require("vnd:hyper").serve(listener, function() return "ok" end))
                return server, function() server:close() end
            end)()"#,
        ),
        (
            "vnd:hyper",
            "Request",
            r#"(function()
                local hyper = require("vnd:hyper")
                local listener = assert(require("std:net").TcpListener.bind("127.0.0.1:0"))
                local seen
                local server = assert(hyper.serve(listener, function(req) seen = req return "ok" end))
                assert(hyper.post({ url = "http://" .. listener:addr() .. "/probe", body = "{}" }))
                server:close()
                return seen
            end)()"#,
        ),
        (
            "std:time",
            "Duration",
            r#"require("std:time").Duration.from_secs(1)"#,
        ),
        (
            "std:workers",
            "ShutdownSignal",
            r#"require("std:workers").on_shutdown()"#,
        ),
        (
            "std:time",
            "Instant",
            r#"require("std:time").Instant.now()"#,
        ),
        (
            "vnd:jsonschema",
            "Schema",
            r#"require("vnd:jsonschema").compile("{}")"#,
        ),
        (
            "vnd:jsonschema",
            "ValidationResult",
            r#"require("vnd:jsonschema").compile("{}"):validate(1)"#,
        ),
        // a mock store: nothing here reaches the operating system's
        ("vnd:keyring", "Store", r#"require("vnd:keyring").mock()"#),
        (
            "vnd:keyring",
            "Entry",
            r#"assert(require("vnd:keyring").mock():entry("example.com", "ada"))"#,
        ),
        (
            "vnd:rusqlite",
            "Database",
            r#"require("vnd:rusqlite").open(":memory:")"#,
        ),
        (
            "vnd:rusqlite",
            "Statement",
            r#"require("vnd:rusqlite").open(":memory:"):prepare("select 1")"#,
        ),
    ];

    /// The recorded types nothing here builds, each with why: a database server, Docker or an AG-UI endpoint, or a
    /// table that is the script's to write. Their records are the
    /// only ones not held to the implementation. (`std:thread` `Channel` exists only inside a worker and is checked
    /// there, see `CHANNEL_PROBE`.)
    const NOT_BUILT: &[(&str, &str, &str)] = &[
        (
            "lib:agui",
            "Run",
            "needs an AG-UI endpoint (the module's own tests run one)",
        ),
        (
            "lib:agui",
            "Event",
            "sent by an AG-UI endpoint (the module's own tests read every type)",
        ),
        (
            "lib:agui",
            "Interrupt",
            "a part of an event an AG-UI endpoint sends",
        ),
        (
            "lib:agui",
            "RunAgentInput",
            "a table the script writes, read by `agui.run` (the module's own tests send one)",
        ),
        (
            "lib:agui",
            "Message",
            "a part of the input the script writes",
        ),
        (
            "lib:agui",
            "ToolCall",
            "a part of the input the script writes",
        ),
        ("lib:agui", "Tool", "a part of the input the script writes"),
        (
            "lib:agui",
            "Context",
            "a part of the input the script writes",
        ),
        ("vnd:sqlx_mysql", "Database", "needs a MySQL server"),
        ("vnd:sqlx_mysql", "Statement", "needs a MySQL server"),
        ("vnd:sqlx_postgres", "Database", "needs a PostgreSQL server"),
        (
            "vnd:testcontainers_modules",
            "MysqlContainer",
            "needs a Docker daemon",
        ),
        (
            "vnd:testcontainers_modules",
            "PostgresContainer",
            "needs a Docker daemon",
        ),
        (
            "vnd:testcontainers_modules",
            "GenericContainer",
            "needs a Docker daemon",
        ),
    ];

    /// Run inside a spawned worker, where `std:thread`'s `parent` channel exists: the recorded members of a
    /// `Channel` that the live channel lacks, as a string. The names arrive as the worker's argument.
    const CHANNEL_PROBE: &str = r#"
        local thread = require("std:thread")
        local missing = {}
        for _, name in ipairs((...)) do
          if thread.parent[name] == nil then missing[#missing + 1] = name end
        end
        return table.concat(missing, ", ")
    "#;

    /// Every method and field a module records for a resource type exists on a live value of that type, and
    /// (for a value that is a plain table or a userdata whose methods sit in a table) every live method is
    /// recorded. A record with no implementation behind it (`Time:add`, once) is a call luau-lsp
    /// accepts and the VM refuses; a live method nobody recorded is a false "key not found" for every script
    /// that uses it. A userdata with fields keeps its members in a function, so only the first direction holds
    /// for it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn recorded_members_exist_on_live_values() {
        let lua = make_engine().expect("engine");
        let mut seen = Vec::new();
        let mut both_ways = 0;
        for module in declared(&lua).expect("declared") {
            for (type_name, methods, fields) in module.recorded_members() {
                let key = module.name();
                seen.push((key.to_string(), type_name.clone()));
                let names: Vec<String> = methods.iter().chain(fields.iter()).cloned().collect();

                if (key, type_name.as_str()) == ("std:thread", "Channel") {
                    let source = lua
                        .create_table()
                        .and_then(|opts| {
                            opts.set("source", CHANNEL_PROBE)?;
                            opts.set("args", names.clone())?;
                            Ok(opts)
                        })
                        .expect("opts");
                    let (missing, err): (Option<String>, Option<String>) = lua
                        .load(
                            r#"local opts = ...
                            return assert(require("std:thread").spawn(opts)):await()"#,
                        )
                        .call_async(source)
                        .await
                        .expect("worker");
                    let missing = missing.unwrap_or_else(|| panic!("worker failed: {err:?}"));
                    assert!(
                        missing.is_empty(),
                        "{key}: `Channel` records {missing}, which a live channel does not have"
                    );
                    continue;
                }

                let Some((_, _, expr)) = INSTANCES
                    .iter()
                    .find(|(k, t, _)| *k == key && *t == type_name)
                else {
                    assert!(
                        NOT_BUILT
                            .iter()
                            .any(|(k, t, _)| *k == key && *t == type_name),
                        "{key}: `{type_name}` is neither built by INSTANCES nor listed in NOT_BUILT"
                    );
                    continue;
                };

                // the value, and optionally a second value: a function that cleans up after it
                let (value, cleanup): (Value, Value) = lua
                    .load(format!("return {expr}"))
                    .eval_async()
                    .await
                    .unwrap_or_else(|e| {
                        panic!("{key}: building a `{type_name}` with `{expr}`: {e}")
                    });
                let probe = "local v, names = ...\nlocal missing = {}\nfor _, name in ipairs(names) do\n  if v[name] == nil then missing[#missing + 1] = name end\nend\nreturn table.concat(missing, \", \")";
                let missing: String = lua
                    .load(probe)
                    .call_async((value.clone(), names.clone()))
                    .await
                    .unwrap_or_else(|e| panic!("{key}: probing a `{type_name}`: {e}"));
                assert!(
                    missing.is_empty(),
                    "{key}: `{type_name}` records {missing}, which a live value does not have"
                );

                // the other direction: what the value has, other than what was recorded
                let live: Option<Vec<String>> = match &value {
                    Value::Table(t) => Some(
                        t.clone()
                            .pairs::<String, Value>()
                            .map(|p| p.expect("pair").0)
                            .collect(),
                    ),
                    Value::UserData(ud) => match ud
                        .metatable()
                        .and_then(|mt| mt.get::<Value>(mlua::MetaMethod::Index))
                    {
                        Ok(Value::Table(t)) => Some(
                            t.pairs::<String, Value>()
                                .map(|p| p.expect("pair").0)
                                .collect(),
                        ),
                        _ => None,
                    },
                    other => panic!("{key}: `{type_name}` built as {other:?}"),
                };
                if let Some(live) = live {
                    let unrecorded: Vec<&String> =
                        live.iter().filter(|n| !names.contains(n)).collect();
                    assert!(
                        unrecorded.is_empty(),
                        "{key}: a live `{type_name}` has {unrecorded:?}, which is not recorded"
                    );
                    both_ways += 1;
                }

                if let Value::Function(cleanup) = cleanup {
                    cleanup.call_async::<()>(()).await.expect("cleanup");
                }
            }
        }
        for (key, type_name, _) in INSTANCES.iter().chain(NOT_BUILT.iter()) {
            let known = seen.iter().any(|(k, t)| k == key && t == type_name);
            assert!(
                known,
                "{key}: `{type_name}` is listed but no module records it"
            );
        }
        assert!(both_ways > 15, "only {both_ways} types held both ways");
    }

    /// Builds every facade-migrated module's runtime `require(...)` table,
    /// paired with its registry key, in the same order as [`luau_decl_sections`].
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
            "std:path" => stdmod::path::module(lua),
            "std:task" => stdmod::task::module(lua),
            "std:thread" => stdmod::thread::module(lua),
            "std:time" => stdmod::time::module(lua),
            "std:workers" => stdmod::workers::module(lua),
            "lib:agui" => lib::agui::module(lua),
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
            "vnd:bollard" => vnd::bollard::module(lua),
            "vnd:hyper" => vnd::hyper::module(lua),
            "vnd:jsonschema" => vnd::jsonschema::module(lua),
            "vnd:keyring" => vnd::keyring::module(lua),
            "vnd:portable_pty" => vnd::portable_pty::module(lua),
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

    /// The names of the module's own functions and of its other members (a type's statics, a constant),
    /// from the properties of its table type: a function is the property whose type is a function type (an
    /// arrow outside every bracket: `luau_decls::is_function`).
    fn module_members(key: &str, section: &str) -> (Vec<String>, Vec<String>) {
        let props = alias_props(section, &luau_decls::scope(key));
        let (functions, fields): (Vec<_>, Vec<_>) = props
            .into_iter()
            .partition(|(_, ty)| luau_decls::is_function(ty));
        let names = |props: Vec<(String, String)>| props.into_iter().map(|(n, _)| n).collect();
        (names(functions), names(fields))
    }

    /// Module keys the definitions record that the base `require(...)` table does not have:
    /// `std:thread`'s `parent` and `shared` are injected only inside a spawned worker.
    const WORKER_ONLY: &[(&str, &str)] = &[("std:thread", "parent"), ("std:thread", "shared")];

    /// A module's live `require(...)` table and its definitions describe the same keys, in both
    /// directions.
    ///
    /// - Every live key is recorded: a function as a property of the module's table type with a function
    ///   type, anything else (a constructor sub-table such as `chrono.Date`, a constant such as `json.null`)
    ///   as another property of it. A free function registered with `set` and never `record_fn`ed fails here.
    /// - Every recorded function is a live function and every other recorded property is a live key (the
    ///   worker-only ones above excepted). A `record_fn` with no `set` behind it, a call the editor accepts
    ///   and the VM refuses, fails here.
    #[test]
    fn definitions_cover_runtime_table_keys() {
        let lua = make_engine().expect("engine");
        let metas = luau_decl_sections(&lua).expect("metas");
        let tables = runtime_tables(&lua);

        assert_eq!(
            metas.len(),
            tables.len(),
            "definitions and runtime tables drifted out of sync"
        );

        let mut checked = 0;
        for ((mkey, meta), (tkey, table)) in metas.iter().zip(tables.iter()) {
            assert_eq!(mkey, tkey, "definition / table ordering drifted");
            let (functions, fields) = module_members(mkey, meta);

            let mut live = Vec::new();
            for pair in table.clone().pairs::<String, mlua::Value>() {
                let (key, value) = pair.expect("module table pair");
                let recorded = match value {
                    mlua::Value::Function(_) => functions.contains(&key),
                    _ => fields.contains(&key),
                };
                assert!(
                    recorded,
                    "{mkey}: key {key:?} is live but not recorded as a function or a module field:\n{meta}"
                );
                live.push(key);
                checked += 1;
            }

            for name in functions.iter().chain(fields.iter()) {
                let worker_only = WORKER_ONLY.contains(&(mkey.as_str(), name.as_str()));
                assert!(
                    live.contains(name) || worker_only,
                    "{mkey}: `{name}` is recorded but the live module table has no such key"
                );
                if let Some(mlua::Value::Function(_)) = table
                    .get::<Option<mlua::Value>>(name.as_str())
                    .expect("get")
                {
                    assert!(
                        functions.contains(name),
                        "{mkey}: `{name}` is a live function but recorded as a field"
                    );
                } else if functions.contains(name) {
                    panic!("{mkey}: `{name}` is recorded as a function but is not one live");
                }
            }
        }
        assert!(checked > 200, "only {checked} live keys checked");
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
