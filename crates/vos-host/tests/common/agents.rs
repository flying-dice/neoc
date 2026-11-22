#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Agents over MCP: a small app with a readOnly tool that tries to write.

use super::{publish_package, temp, Running};

const LEDGER: &str = r#"
app.manifest { id = "ledger", name = "Ledger", icon = "gear" }
local db = app.db
local t = app.t
db:migrate { [1] = "create table entries(id integer primary key, who text not null)" }

app.tool {
  name = "add",
  description = "Add an entry as the calling actor",
  run = function(_, ctx)
    db:exec("insert into entries(who) values (:who)", { who = ctx.actor.kind .. ":" .. ctx.actor.id .. ":" .. ctx.actor.name })
    return { count = db:one("select count(*) as n from entries").n }
  end,
}

app.tool {
  name = "peek",
  readOnly = true,
  description = "Read the entries",
  run = function()
    return { who = db:all("select who from entries order by id") }
  end,
}

-- declared readOnly, but tries to write by every route: the host must stop each of them
app.tool {
  name = "sneaky",
  readOnly = true,
  description = "A readOnly tool that attempts writes",
  run = function()
    local blocked = {}
    local function attempt(name, f)
      local ok, err = pcall(f)
      blocked[name] = (not ok) and tostring(err) or false
    end
    attempt("exec", function() db:exec("insert into entries(who) values ('sneaky')") end)
    attempt("returning", function() db:all("insert into entries(who) values ('sneaky') returning id") end)
    attempt("kv", function() app.kv:set("k", 1) end)
    return { blocked = blocked, count = db:one("select count(*) as n from entries").n }
  end,
}
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
