#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Per-agent identity and permissions across the real wiring: the roster, `/mcp` attribution and grants, the
//! `read-only` rule, and the `agents.*` methods on `/rpc`.

use crate::common;

use common::{
    agents::install_ledger, call, connect_agent, connect_rpc, pair, rpc, start, text_of, tool_names,
    wait_until, Ws,
};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use vos_domain::{AppId, Grants};

fn grants_ledger() -> Grants {
    Grants::apps([AppId::parse("ledger").unwrap()])
}

async fn who(r: &common::Running) -> Value {
    let notes = AppId::parse("ledger").unwrap();
    let data = r.host.service.data_of(&notes).unwrap();
    let rows = data
        .query("select who from entries order by id".into(), vos_application::data::Params::None)
        .await
        .unwrap();
    Value::Array(rows.into_iter().map(|row| row["who"].clone()).collect())
}

#[tokio::test]
async fn an_agent_with_its_own_token_calls_a_granted_tool_attributed_and_is_denied_an_ungranted_one() {
    let fake = common::agent_with("fake", "Fake", grants_ledger());
    let other = common::agent_with("other", "Other", Grants::default());
    let r = start(|c| c.agents = vec![("tok-fake".into(), fake), ("tok-other".into(), other)]).await;
    install_ledger(&r).await;
    let (granted, _) = connect_agent(r.addr, "tok-fake").await;
    let (ungranted, _) = connect_agent(r.addr, "tok-other").await;

    // each sees only what it is granted
    assert_eq!(tool_names(&granted).await, ["ledger__add", "ledger__peek", "ledger__sneaky"]);
    assert!(tool_names(&ungranted).await.is_empty(), "deny by default");

    // allowed, and the app sees who called: ctx.actor = {kind = "agent", id, name}
    let out = call(&granted, "ledger__add", json!({})).await;
    assert_ne!(out.is_error, Some(true), "{out:?}");
    assert_eq!(who(&r).await, json!(["agent:fake:Fake"]));

    // denied: an error the model can read, and the VM never ran
    let denied = call(&ungranted, "ledger__add", json!({})).await;
    assert_eq!(denied.is_error, Some(true));
    assert!(text_of(&denied).contains("Other may not call"), "{}", text_of(&denied));
    assert_eq!(who(&r).await, json!(["agent:fake:Fake"]), "nothing was written for the denied agent");

    // a read-only tool reads, and cannot write by any route even though its code tries
    let peek = call(&granted, "ledger__peek", json!({})).await;
    assert!(text_of(&peek).contains("agent:fake:Fake"), "{}", text_of(&peek));
    let sneaky = call(&granted, "ledger__sneaky", json!({})).await;
    assert_ne!(sneaky.is_error, Some(true), "the tool itself ran: {}", text_of(&sneaky));
    let report = text_of(&sneaky);
    for route in ["exec", "returning", "kv"] {
        assert!(report.contains(route), "{route} in {report}");
    }
    assert!(
        report.to_lowercase().contains("readonly") || report.to_lowercase().contains("read-only"),
        "{report}"
    );
    assert_eq!(who(&r).await, json!(["agent:fake:Fake"]), "the read-only tool wrote nothing");
    r.stop().await;
}

async fn paired_rpc(r: &common::Running) -> Ws {
    let cookie = pair(r, "Mac").await;
    let raw = connect_rpc(r.addr, Some(&cookie), None).await.unwrap();
    Ws { raw, backlog: vec![] }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one journey: create, grant, rotate, delete
async fn the_roster_is_managed_over_rpc_and_every_change_reaches_the_agents_mcp_session() {
    let r = start(|_| {}).await;
    install_ledger(&r).await;
    let mut ws = paired_rpc(&r).await;

    // create: deny by default, and the token shown once
    let created =
        rpc(&mut ws, 1, "agents.create", json!({"name": "Code Reviewer", "role": "Reviews diffs"})).await;
    assert!(created.get("error").is_none(), "{created}");
    let agent = &created["result"]["agent"];
    assert_eq!(agent["id"], "code-reviewer");
    assert_eq!(agent["grants"], json!({"all": false, "apps": []}));
    assert!(agent.get("runtime").is_none() && agent.get("approval").is_none(), "{agent}");
    let token = created["result"]["token"].as_str().unwrap().to_owned();
    assert!(token.starts_with("vos_"));
    assert_eq!(
        rpc(&mut ws, 2, "agents.create", json!({"name": "x", "bogus": 1})).await["error"]["code"],
        -32602
    );
    // the launch settings of older versions are no longer parameters: refused like any unknown field
    for old in [json!({"name": "x", "runtime": {"command": "x"}}), json!({"name": "x", "approval": "ask"})] {
        assert_eq!(rpc(&mut ws, 2, "agents.create", old).await["error"]["code"], -32602);
    }
    assert_eq!(
        rpc(&mut ws, 3, "agents.create", json!({"id": "code-reviewer", "name": "Dup"})).await["error"]
            ["code"],
        -32004
    );
    assert_eq!(rpc(&mut ws, 4, "agents.create", json!({"name": ""})).await["error"]["code"], -32001);

    // the token is a working MCP credential with no tools yet
    let (mcp, events) = connect_agent(r.addr, &token).await;
    assert!(tool_names(&mcp).await.is_empty());

    // grants: the agent's session is told, and the list follows
    let set =
        rpc(&mut ws, 5, "agents.grants.set", json!({"id": "code-reviewer", "grants": {"apps": ["ledger"]}}))
            .await;
    assert_eq!(set["result"]["agent"]["grants"], json!({"all": false, "apps": ["ledger"]}), "{set}");
    wait_until("tools/list_changed", || events.0.load(Ordering::SeqCst) >= 1).await;
    assert_eq!(tool_names(&mcp).await, ["ledger__add", "ledger__peek", "ledger__sneaky"]);
    call(&mcp, "ledger__add", json!({})).await;
    assert_eq!(who(&r).await, json!(["agent:code-reviewer:Code Reviewer"]));
    let revoked = rpc(&mut ws, 6, "agents.grants.set", json!({"id": "code-reviewer", "grants": {}})).await;
    assert_eq!(revoked["result"]["agent"]["grants"], json!({"all": false, "apps": []}));
    wait_until("the second tools/list_changed", || events.0.load(Ordering::SeqCst) >= 2).await;
    assert!(tool_names(&mcp).await.is_empty());
    let denied = call(&mcp, "ledger__add", json!({})).await;
    assert_eq!(denied.is_error, Some(true), "a grant revoked mid-session applies to the next call");

    // update changes only what it is given; null clears the avatar
    let updated =
        rpc(&mut ws, 7, "agents.update", json!({"id": "code-reviewer", "avatar": "sparkles"})).await;
    assert_eq!(updated["result"]["agent"]["avatar"], "sparkles");
    assert_eq!(updated["result"]["agent"]["role"], "Reviews diffs");
    let cleared = rpc(&mut ws, 8, "agents.update", json!({"id": "code-reviewer", "avatar": null})).await;
    assert!(cleared["result"]["agent"].get("avatar").is_none());
    assert_eq!(cleared["result"]["agent"]["role"], "Reviews diffs");

    // list
    let listed = rpc(&mut ws, 9, "agents.list", json!({})).await;
    assert_eq!(listed["result"]["agents"][0]["id"], "code-reviewer");
    assert!(listed["result"]["agents"][0].get("token").is_none(), "tokens are never listed");

    // rotate: the old token stops, the new one works
    let rotated = rpc(&mut ws, 10, "agents.token.rotate", json!({"id": "code-reviewer"})).await;
    let new_token = rotated["result"]["token"].as_str().unwrap().to_owned();
    assert_ne!(new_token, token);
    let stale = reqwest::Client::new()
        .post(format!("http://{}/mcp", r.addr))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 401);
    let (fresh, _) = connect_agent(r.addr, &new_token).await;
    assert!(tool_names(&fresh).await.is_empty());

    // delete
    assert!(rpc(&mut ws, 11, "agents.delete", json!({"id": "code-reviewer"})).await.get("error").is_none());
    assert_eq!(
        rpc(&mut ws, 12, "agents.delete", json!({"id": "code-reviewer"})).await["error"]["code"],
        -32002
    );
    assert_eq!(rpc(&mut ws, 13, "agents.list", json!({})).await["result"]["agents"], json!([]));
    let gone = reqwest::Client::new()
        .post(format!("http://{}/mcp", r.addr))
        .header("authorization", format!("Bearer {new_token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 401, "a deleted agent's tokens are dead");
    r.stop().await;
}

#[tokio::test]
async fn agents_and_their_grants_and_tokens_survive_a_restart() {
    let tmp = common::temp("roster-restart");
    vos_host::startup::prepare_data_dir(&tmp.join("data")).unwrap();
    let configure = |c: &mut vos_host::HostConfig| c.data_dir = Some(tmp.join("data"));
    let r = start(configure).await;
    let mut ws = paired_rpc(&r).await;
    let created = rpc(&mut ws, 1, "agents.create", json!({"name": "Claude", "grants": {"all": true}})).await;
    let token = created["result"]["token"].as_str().unwrap().to_owned();
    r.stop().await;

    let r = start(configure).await;
    let mut ws = paired_rpc(&r).await;
    let listed = rpc(&mut ws, 1, "agents.list", json!({})).await;
    assert_eq!(listed["result"]["agents"][0]["grants"], json!({"all": true, "apps": []}));
    install_ledger(&r).await;
    let (agent, _) = connect_agent(r.addr, &token).await;
    assert_eq!(tool_names(&agent).await.len(), 3, "the persistent token still works (it is stored hashed)");
    r.stop().await;
}

/// An app that offers the other apps' tools to an agent of its own.
const BROKER: &str = r#"
local function names()
  local out = {}
  for _, tool in app.tools.all() do
    table.insert(out, tool.name .. (if tool.read_only then " (read)" else ""))
  end
  return out
end
RPC.on_request("_names", function() return names() end)
RPC.on_request("_call", function(args)
  local result, err = app.tools.call(args.tool, { id = "ada", name = "Ada" }, args.input)
  return { result = result, err = err and err.message, kind = err and err.kind }
end)
RPC.on_request("_bad", function(args)
  return { raised = not pcall(app.tools.call, args.tool, args.agent, {}) }
end)
"#;

#[tokio::test]
async fn an_app_lists_and_calls_the_tools_of_the_running_apps_for_an_agent_of_its_own() {
    let r = start(|_| {}).await;
    install_ledger(&r).await;
    let url = common::publish_package(
        &common::temp("broker").join("pkg"),
        "broker",
        "app.luau",
        &[("app.luau", BROKER.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let broker = AppId::parse("broker").unwrap();
    let ask = |method: &'static str, args: Value| {
        let service = r.host.service.clone();
        let broker = broker.clone();
        async move { service.call_handler(&broker, method, args).await.unwrap() }
    };

    // no grant is asked of the app's agent: every tool of every running app, in name order
    assert_eq!(
        ask("_names", json!({})).await,
        json!(["ledger__add", "ledger__peek (read)", "ledger__sneaky (read)"])
    );

    // the tool is told who called: the agent the app named, under the app's own id, so no app can speak as another's
    let added = ask("_call", json!({ "tool": "ledger__add", "input": {} })).await;
    assert_eq!(added, json!({ "result": { "count": 1 } }));
    assert_eq!(who(&r).await, json!(["agent:broker:ada:Ada"]));

    // a read-only tool still cannot write, whoever asks for it
    let sneaky = ask("_call", json!({ "tool": "ledger__sneaky", "input": {} })).await;
    assert_eq!(sneaky["result"]["count"], 1, "{sneaky}");

    // a tool that is not there, and one of an app that is not running, are errors the app can read
    let missing = ask("_call", json!({ "tool": "ledger__nope", "input": {} })).await;
    assert!(missing["err"].as_str().is_some_and(|e| !e.is_empty()), "{missing}");
    let nowhere = ask("_call", json!({ "tool": "ghost__add", "input": {} })).await;
    assert!(nowhere["err"].as_str().unwrap().contains("not installed"), "{nowhere}");

    // a name that is not `<app>__<tool>` and an agent without a name are the app's own mistakes
    assert_eq!(
        ask("_bad", json!({ "tool": "nope", "agent": { "id": "a", "name": "A" } })).await,
        json!({ "raised": true })
    );
    assert_eq!(
        ask("_bad", json!({ "tool": "ledger__add", "agent": { "id": "a" } })).await,
        json!({ "raised": true })
    );
    assert_eq!(
        who(&r).await,
        json!(["agent:broker:ada:Ada"]),
        "nothing was written by a call that was refused"
    );
    r.stop().await;
}

/// An app that leases an MCP token for an agent of its own, and has a tool of its own the agent is not offered.
const LEASER: &str = r#"
RPC.on_request("ping", function() return "pong" end)
RPC.on_request("_lease", function() return app.tools.lease({ id = "ada", name = "Ada" }) end)
RPC.on_request("_release", function(args) app.tools.release(args.token); return true end)
"#;

/// What `/mcp` answers a bare request with `token`: 401 once the token is no longer good.
async fn mcp_status(r: &common::Running, token: &str) -> u16 {
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }
    });
    reqwest::Client::new()
        .post(r.url("/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn a_token_an_app_leases_calls_the_other_apps_tools_over_mcp_as_its_agent_until_released() {
    let r = start(|_| {}).await;
    install_ledger(&r).await;
    let url = common::publish_package(
        &common::temp("leaser").join("pkg"),
        "leaser",
        "app.luau",
        &[("app.luau", LEASER.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let leaser = AppId::parse("leaser").unwrap();
    let ask = |method: &'static str, args: Value| {
        let service = r.host.service.clone();
        let leaser = leaser.clone();
        async move { service.call_handler(&leaser, method, args).await.unwrap() }
    };

    let lease = ask("_lease", json!({})).await;
    let token = lease["token"].as_str().unwrap().to_owned();
    assert_eq!(lease["url"], r.url("/mcp"), "the host's own address: it is bound to loopback");
    assert!(r.host.agents.list().is_empty(), "a lease is not an agent of the roster");

    // every running app's tools but the leasing app's own
    let (agent, changed) = connect_agent(r.addr, &token).await;
    assert_eq!(tool_names(&agent).await, ["ledger__add", "ledger__peek", "ledger__sneaky"]);

    // attributed as `app.tools.call` attributes it: the app's agent, under the app's id
    let added = call(&agent, "ledger__add", json!({})).await;
    assert_ne!(added.is_error, Some(true), "{added:?}");
    assert_eq!(who(&r).await, json!(["agent:leaser:ada:Ada"]));
    let own = call(&agent, "leaser__ping", json!({})).await;
    assert_eq!(own.is_error, Some(true), "not the leasing app's own tools: {own:?}");

    // released: the session is told its tools changed, and the token is refused from then on
    let before = changed.0.load(Ordering::SeqCst);
    assert_eq!(ask("_release", json!({ "token": token })).await, json!(true));
    wait_until("the session is told", || changed.0.load(Ordering::SeqCst) > before).await;
    assert_eq!(mcp_status(&r, &token).await, 401);

    // and every lease of the app goes when its backend stops
    let again = ask("_lease", json!({})).await;
    let token = again["token"].as_str().unwrap().to_owned();
    assert_ne!(mcp_status(&r, &token).await, 401);
    r.host.service.disable(&leaser).await.unwrap();
    assert_eq!(mcp_status(&r, &token).await, 401);
    r.stop().await;
}
