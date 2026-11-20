#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Agents over AG-UI and MCP: the scripted ACP agent, SSE helpers and a small app with a readOnly tool that
//! tries to write.

use super::{agent_with, publish_package, temp, Running};
use agentos_domain::{Agent, AgentRuntime, Grants};
use serde_json::{json, Value};
use std::path::PathBuf;

/// The scripted ACP agent of `agentos-acp` (a workspace binary): built on demand when this test run did not
/// already build it, so `cargo nextest run -p agentos-host` works on its own.
pub fn fake_agent_path() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            // target/<profile>/deps/<test> -> target/<profile> (with cargo's newer build-dir layout the test sits in
            // target/<profile>/build/<package>/<hash>/out: the profile directory is the nearest `debug`/`release`)
            let profile_dir = exe
                .ancestors()
                .find(|p| p.file_name().is_some_and(|n| n == "debug" || n == "release"))
                .unwrap();
            let path = profile_dir.join(if cfg!(windows) { "fake_agent.exe" } else { "fake_agent" });
            if !path.exists() {
                let status = std::process::Command::new(env!("CARGO"))
                    .args(["build", "-q", "-p", "agentos-acp", "--bin", "fake_agent"])
                    .status()
                    .unwrap();
                assert!(status.success(), "building the fake agent");
            }
            assert!(path.exists(), "no fake agent at {}", path.display());
            path
        })
        .clone()
}

/// An agent the host can launch: the scripted fake.
pub fn launchable(id: &str, name: &str, grants: Grants) -> Agent {
    let mut agent = agent_with(id, name, grants);
    agent.runtime =
        AgentRuntime { command: fake_agent_path().to_string_lossy().into_owned(), ..AgentRuntime::default() };
    agent
}

/// The events of an SSE body, in order.
pub fn sse_events(body: &str) -> Vec<Value> {
    body.split("\n\n")
        .filter_map(|frame| frame.lines().find_map(|l| l.strip_prefix("data:")))
        .map(|d| serde_json::from_str(d.trim()).unwrap())
        .collect()
}

pub fn event_types(events: &[Value]) -> Vec<String> {
    events.iter().map(|e| e["type"].as_str().unwrap().to_owned()).collect()
}

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
        "app.lua",
        &[("app.lua", LEDGER.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
}

/// `{"threadId", "runId", "messages": [user text], "forwardedProps": {"agentId"}}`
pub fn run_body(thread: &str, run: &str, text: &str, agent: &str) -> Value {
    json!({
        "threadId": thread, "runId": run,
        "messages": [{"id": format!("u-{run}"), "role": "user", "content": text}],
        "forwardedProps": {"agentId": agent},
    })
}

pub fn resume_body(thread: &str, run: &str, interrupt: &str, option: Option<&str>) -> Value {
    json!({
        "threadId": thread, "runId": run,
        "resume": [{
            "interruptId": interrupt,
            "status": if option.is_some() { "resolved" } else { "cancelled" },
            "payload": option.map(|o| json!({"optionId": o})),
        }],
    })
}
