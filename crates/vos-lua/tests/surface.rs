#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The app API an app gets is the one `types/generated/vos.d.luau` declares: every key of the live `app` table and
//! of the tables under it (`app.db`, `app.kv`, `app.files`, `app.agents`, `app.tools`, `app.t`) is
//! declared, and every declared key is there, so a call that type-checks and then fails (declared, missing) or works
//! unseen by the checker (present, undeclared) fails here.

mod common;

use common::{agent_ctx, db, id, package};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, path::Path};
use vos_lua::{AppVm, VmConfig};

/// The keys of the table type `export type <name> = { ... }` in the declarations: its members at the top level,
/// not those of a table or function type inside one, and not its doc comments.
fn declared_keys(decls: &str, name: &str) -> BTreeSet<String> {
    let head = format!("export type {name} = {{");
    let mut lines = decls.lines().skip_while(|line| line.trim_end() != head);
    assert!(lines.next().is_some(), "no `{head}` in the declarations");
    let (mut braces, mut parens) = (1_i32, 0_i32);
    let mut keys = BTreeSet::new();
    for line in lines {
        let code = line.trim();
        if code.starts_with("--") {
            continue;
        }
        if (braces, parens) == (1, 0) {
            if let Some((key, _)) = code.split_once(':') {
                let key = key.trim();
                let key = key.strip_prefix("[\"").and_then(|k| k.strip_suffix("\"]")).unwrap_or(key);
                assert!(
                    !key.starts_with('['),
                    "`{name}` has an indexer (`{code}`): every key would be declared"
                );
                assert!(
                    key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$'),
                    "`{name}`: cannot read the member `{code}`"
                );
                keys.insert(key.to_owned());
            }
        }
        for c in code.chars() {
            match c {
                '{' => braces += 1,
                '}' => braces -= 1,
                '(' => parens += 1,
                ')' => parens -= 1,
                _ => {}
            }
        }
        if braces == 0 {
            return keys;
        }
    }
    panic!("`{name}` is never closed in the declarations");
}

fn declarations() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../types/generated/vos.d.luau");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strings(value: &Value) -> BTreeSet<String> {
    value.as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_owned()).collect()
}

/// Both ways: what is live and not declared, and what is declared and not live.
fn assert_same(what: &str, live: &BTreeSet<String>, declared: &BTreeSet<String>) {
    let undeclared: Vec<_> = live.difference(declared).collect();
    let missing: Vec<_> = declared.difference(live).collect();
    assert!(
        undeclared.is_empty() && missing.is_empty(),
        "{what}: live but not declared {undeclared:?}; declared but not live {missing:?}"
    );
}

/// The tables of the app API and the type each is declared as.
const TABLES: [(&str, &str); 7] = [
    ("app", "vos_AppApi"),
    ("db", "vos_Db"),
    ("kv", "vos_KvApi"),
    ("files", "vos_FilesApi"),
    ("agents", "vos_AgentsApi"),
    ("tools", "vos_ToolRegistry"),
    ("t", "vos_SchemaBuilders"),
];

/// An app whose tool lists the keys of `app` and of every table under it.
const APP: &str = r#"
    local function keys(t)
      local out = {}
      for k in pairs(t) do
        table.insert(out, k)
      end
      return out
    end
    app.tool { name = "keys", readOnly = true, run = function()
      return {
        app = keys(app), db = keys(app.db), kv = keys(app.kv), files = keys(app.files), agents = keys(app.agents),
        tools = keys(app.tools), t = keys(app.t),
      }
    end }
    return {}
"#;

#[tokio::test]
async fn the_app_table_and_the_tables_under_it_have_exactly_the_declared_keys() {
    // the host's config: no test hooks
    let vm = AppVm::load(&id("probe"), package(APP, &[]), db(), VmConfig::default()).await.unwrap();
    let live = vm.call_tool("keys", Value::Object(Map::new()), &agent_ctx(0), false).await.unwrap();

    let decls = declarations();
    for (field, ty) in TABLES {
        assert_same(field, &strings(&live[field]), &declared_keys(&decls, ty));
    }
    // the whole surface, spelled out: a member that comes back (or one that goes) changes this list
    let app: Vec<String> = strings(&live["app"]).into_iter().collect();
    assert_eq!(
        app,
        ["agents", "db", "emit", "files", "id", "kv", "log", "notify", "openWindow", "t", "tool", "tools"]
    );
    let db: Vec<String> = strings(&live["db"]).into_iter().collect();
    assert_eq!(db, ["all", "exec", "migrate", "one", "tx"]);
}

#[test]
fn the_declaration_reader_takes_top_level_members_only() {
    let decls = "\
export type vos_Other = { nope: string }

--- a doc comment
export type vos_Probe = {
\t--- not a key: { braces: in a comment }
\tid: string,
\tnested: { inner: number, deeper: { x: string } },
\tcall: (self: vos_Probe, arg: string) -> (),
\twrapped: (
\t\tlabel: string
\t) -> (),
\t[\"$source\"]: string,
\tlast: <T>(initial: T) -> T,
}

export type vos_After = { later: string }
";
    let keys: Vec<String> = declared_keys(decls, "vos_Probe").into_iter().collect();
    assert_eq!(keys, ["$source", "call", "id", "last", "nested", "wrapped"]);
}

#[test]
#[should_panic(expected = "has an indexer")]
fn a_declared_indexer_is_refused_since_it_would_declare_every_key() {
    declared_keys("export type vos_Probe = {\n\t[string]: any,\n}\n", "vos_Probe");
}
