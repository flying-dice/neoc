#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Agents over MCP: a small app with a read-only tool that tries to write.

use super::{publish_app_ref, temp, Running};
use serde_json::json;

const LEDGER: &str = r#"
-- ledger
local db = app.db
db:migrate { [1] = "create table entries(id integer primary key, who text not null)" }

RPC.on_request("add", function(_, ctx)
  db:exec("insert into entries(who) values (:who)", { who = ctx.actor.kind .. ":" .. ctx.actor.id .. ":" .. ctx.actor.name })
  return { count = db:one("select count(*) as n from entries").n }
end)

RPC.on_request("peek", function()
  return { who = db:all("select who from entries order by id") }
end)

-- the contract says it is read-only, but it tries to write by every route: the host must stop each of them
RPC.on_request("sneaky", function()
  local blocked = {}
  local function attempt(name, f)
    local ok, err = pcall(f)
    blocked[name] = (not ok) and tostring(err) or false
  end
  attempt("exec", function() db:exec("insert into entries(who) values ('sneaky')") end)
  attempt("returning", function() db:all("insert into entries(who) values ('sneaky') returning id") end)
  attempt("kv", function() app.kv:set("k", 1) end)
  return { blocked = blocked, count = db:one("select count(*) as n from entries").n }
end)
"#;

/// The contract of the ledger: what an agent is shown of each tool, and that `peek` and `sneaky` change nothing.
const CONTRACT: &str = r#"{
  "openrpc": "1.4.0", "info": { "title": "Ledger", "version": "1.0.0" },
  "methods": [
    { "name": "add", "summary": "Add", "description": "Add an entry as the calling actor", "params": [],
      "result": { "name": "added", "schema": { "type": "object" } } },
    { "name": "peek", "summary": "Peek", "description": "Read the entries", "x-readOnly": true, "params": [],
      "result": { "name": "entries", "schema": { "type": "object" } } },
    { "name": "sneaky", "summary": "Sneaky", "description": "A read-only tool that attempts writes",
      "x-readOnly": true, "params": [], "result": { "name": "report", "schema": { "type": "object" } } }
  ]
}"#;

/// Install the `ledger` app (see above) into a running host over its service; returns nothing, panics on failure.
pub async fn install_ledger(r: &Running) {
    // each install has a directory of its own: the tests that install the ledger run side by side
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let manifest = json!({
        "id": "ledger", "name": "ledger", "main": "app.luau", "ui": { "root": "static" }, "contract": "openrpc.json"
    });
    let url = publish_app_ref(
        &temp(&format!("ledger-{n}")).join("pkg"),
        manifest,
        &[("app.luau", LEDGER.as_bytes().to_vec()), ("openrpc.json", CONTRACT.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
}
