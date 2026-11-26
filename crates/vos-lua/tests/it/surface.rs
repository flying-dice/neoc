#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The app API an app gets is the one its schema describes and `types/generated/vos.d.luau` declares: every key of the
//! live `app` table and of `app.t` is declared, every declared key is there, and every declared member of the values
//! under it (`app.db`, `app.kv`, `app.files`, a location, `app.agents`, a call's `ctx`) is there, so a call that
//! type-checks and then fails (declared, missing) or works unseen by the checker (present, undeclared) fails here.

use crate::common;

use common::{agent_ctx, db, id, package};
use mlua_ir::ir::{Ir, TypeBody};
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

/// The members of an object type of the schema: its getters and its methods.
fn members(ir: &Ir, name: &str) -> BTreeSet<String> {
    let def = ir.type_def(name).unwrap_or_else(|| panic!("no type {name} in the schema"));
    let TypeBody::Object { fields, methods, .. } = &def.body else {
        panic!("{name} is not an object: {def:?}")
    };
    fields.iter().map(|f| f.name.clone()).chain(methods.iter().map(|m| m.name.clone())).collect()
}

/// The statics of an object type of the schema.
fn statics(ir: &Ir, name: &str) -> BTreeSet<String> {
    let def = ir.type_def(name).unwrap_or_else(|| panic!("no type {name} in the schema"));
    let TypeBody::Object { statics, .. } = &def.body else { panic!("{name} is not an object: {def:?}") };
    statics.iter().map(|s| s.name.clone()).collect()
}

/// The values of the app API that are userdata, how an app reaches each, and the type each is declared as. A
/// userdata has no keys to list, so the app is asked for each declared member by name.
const OBJECTS: [(&str, &str, &str); 5] = [
    ("db", "app.db", "vos_app_Db"),
    ("kv", "app.kv", "vos_app_Kv"),
    ("files", "app.files", "vos_app_Files"),
    ("documents", "app.files.documents", "vos_app_Location"),
    ("agents", "app.agents", "vos_app_Agents"),
];

/// An app whose tool lists the keys of `app` and of `app.t`, and which of the names it is given each object has.
fn probe(ir: &Ir) -> String {
    let asked = OBJECTS
        .iter()
        .map(|(key, value, ty)| {
            let names = members(ir, ty).iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
            format!("{key} = has({value}, {{ {names} }}),")
        })
        .collect::<Vec<_>>()
        .join(
            "
        ",
        );
    format!(
        r#"
    local function keys(t)
      local out = {{}}
      for k in pairs(t) do
        table.insert(out, k)
      end
      return out
    end
    local function has(value, names)
      local out = {{}}
      for _, name in ipairs(names) do
        if value[name] ~= nil then
          table.insert(out, name)
        end
      end
      return out
    end
    app.tool {{ name = "keys", readOnly = true, run = function(_, ctx)
      return {{
        app = keys(app), t = keys(app.t), tools = keys(app.tools), same = require("vos:app") == app,
        kinds = {{ db = typeof(app.db), id = typeof(app.id), ctx = typeof(ctx), documents = typeof(app.files.documents) }},
        ctx = has(ctx, {{ "actor", "now", "fail" }}),
        {asked}
      }}
    end }}
    return {{}}
"#
    )
}

#[tokio::test]
async fn the_app_api_an_app_gets_is_the_one_the_schema_and_the_declarations_describe() {
    let ir = vos_lua::declarations::schema();
    // the host's config: no test hooks
    let vm = AppVm::load(&id("probe"), package(&probe(&ir), &[]), db(), VmConfig::default()).await.unwrap();
    let live = vm.call_tool("keys", Value::Object(Map::new()), &agent_ctx(0), false).await.unwrap();

    // the module: every key of the live table is a constant, an exported type or a function of the schema
    let module = ir.modules.iter().find(|m| m.name == "vos:app").unwrap();
    let in_schema: BTreeSet<String> = module
        .constants
        .iter()
        .map(|c| c.name.clone())
        .chain(module.objects.iter().map(|o| o.name.clone()))
        .chain(module.functions.iter().map(|f| f.name.clone()))
        .collect();
    assert_same("app", &strings(&live["app"]), &in_schema);
    // and of the committed declarations, which are written from that schema
    let decls = declarations();
    assert_same("vos_app", &strings(&live["app"]), &declared_keys(&decls, "vos_app"));
    assert_same("t", &strings(&live["t"]), &statics(&ir, "vos_app_t"));
    assert_same("vos_app_tStatics", &strings(&live["t"]), &declared_keys(&decls, "vos_app_tStatics"));
    assert_same("tools", &strings(&live["tools"]), &statics(&ir, "vos_app_tools"));
    assert_same(
        "vos_app_toolsStatics",
        &strings(&live["tools"]),
        &declared_keys(&decls, "vos_app_toolsStatics"),
    );
    for (key, _, ty) in OBJECTS {
        assert_same(key, &strings(&live[key]), &members(&ir, ty));
    }
    assert_same("ctx", &strings(&live["ctx"]), &members(&ir, "vos_app_Context"));

    // the whole surface, spelled out: a member that comes back (or one that goes) changes this list
    let app: Vec<String> = strings(&live["app"]).into_iter().collect();
    assert_eq!(
        app,
        ["agents", "db", "emit", "files", "id", "kv", "log", "notify", "openWindow", "t", "tool", "tools"]
    );
    let db: Vec<String> = strings(&live["db"]).into_iter().collect();
    assert_eq!(db, ["all", "exec", "migrate", "one", "tx"]);
    let t: Vec<String> = strings(&live["t"]).into_iter().collect();
    assert_eq!(t, ["array", "boolean", "enum", "integer", "number", "object", "string"]);
    // the module and the global are one table; the id is a plain string, the values under `app` are userdata, which
    // `typeof` names as the declarations do (what narrows a Luau extern type)
    assert_eq!(live["same"], true);
    assert_eq!(
        live["kinds"],
        serde_json::json!({
            "db": "vos_app_Db", "id": "string", "ctx": "vos_app_Context", "documents": "vos_app_Location"
        })
    );
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
