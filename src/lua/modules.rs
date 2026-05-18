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

fn find_closest<'a>(requested: &str, candidates: &[&'a str], threshold: usize) -> Option<&'a str> {
    candidates
        .iter()
        .filter(|c| **c != requested)
        .map(|c| (*c, levenshtein(requested, c)))
        .filter(|(_, d)| *d <= threshold)
        .min_by(|(a, da), (b, db)| da.cmp(db).then_with(|| a.cmp(b)))
        .map(|(c, _)| c)
}

pub fn install(lua: &Lua) -> mlua::Result<()> {
    let mut modules: HashMap<String, Table> = HashMap::new();

    // std:* — mirrors Rust standard library.
    modules.insert("std:collections".into(), stdmod::collections::module(lua)?);
    modules.insert("std:env".into(), stdmod::env::module(lua)?);
    modules.insert("std:fs".into(), stdmod::fs::module(lua)?);
    modules.insert("std:io".into(), stdmod::io::module(lua)?);
    modules.insert("std:net".into(), stdmod::net::module(lua)?);
    modules.insert("std:thread".into(), stdmod::thread::module(lua)?);
    modules.insert("std:workers".into(), stdmod::workers::module(lua)?);

    // lib:* — blessed gap-fillers and our own custom modules.
    modules.insert("lib:base64".into(), lib::base64::module(lua)?);
    modules.insert("lib:compile".into(), lib::compile::module(lua)?);
    modules.insert("lib:http".into(), lib::http::module(lua)?);
    modules.insert("lib:ini".into(), lib::ini::module(lua)?);
    modules.insert("lib:json".into(), lib::json::module(lua)?);
    modules.insert("lib:mime".into(), lib::mime::module(lua)?);
    modules.insert("lib:test".into(), lib::test::module(lua)?);
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
            let threshold = max(1, name.len() / 3);
            let suggestion = find_closest(&name, &keys, threshold);

            let ns = name.find(':').map(|pos| &name[..pos]);
            let mut ns_modules: Vec<&str> = if let Some(ns) = ns {
                let prefix = format!("{ns}:");
                keys.iter()
                    .filter(|k| k.starts_with(&prefix))
                    .copied()
                    .collect()
            } else {
                keys.clone()
            };
            ns_modules.sort_unstable();

            let mut msg = format!("require: unknown module {name:?}");
            if let Some(s) = suggestion {
                msg.push_str(&format!(" — did you mean {s:?}?"));
            }
            if !ns_modules.is_empty() && ns_modules.len() <= 10 {
                msg.push_str(&format!(" available: {}", ns_modules.join(", ")));
            }
            mlua::Error::external(msg)
        })
    })?;
    lua.globals().set("require", require_fn)?;
    Ok(())
}
