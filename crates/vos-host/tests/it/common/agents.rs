#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Agents over MCP: a small app with a read-only tool that tries to write.

use super::{publish_package, temp, Running};

const LEDGER: &str = r#"
-- ledger
local db = app.db
db:migrate { [1] = "create table entries(id integer primary key, who text not null)" }

RPC.on_request("add", function(_, ctx)
  db:exec("insert into entries(who) values (:who)", { who = ctx.actor.kind .. ":" .. ctx.actor.id .. ":" .. ctx.actor.name })
  return { count = db:one("select count(*) as n from entries").n }
end, { description = "Add an entry as the calling actor" })

RPC.on_request("peek", function()
  return { who = db:all("select who from entries order by id") }
end, { description = "Read the entries", read_only = true })

-- declared read_only, but tries to write by every route: the host must stop each of them
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
end, { description = "A read-only tool that attempts writes", read_only = true })
"#;

/// Install the `ledger` app (see above) into a running host over its service; returns nothing, panics on failure.
pub async fn install_ledger(r: &Running) {
    let url = publish_package(
        &temp("ledger").join("pkg"),
        "ledger",
        "app.luau",
        &[("app.luau", LEDGER.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
}
