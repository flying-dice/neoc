#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Per-agent identity and permissions across the real wiring: the roster, `/mcp` attribution and grants, the
//! `readOnly` rule, and the `agents.*` methods on `/rpc`.

mod common;

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

    // a readOnly tool reads, and cannot write by any route even though its code tries
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
    assert_eq!(who(&r).await, json!(["agent:fake:Fake"]), "the readOnly tool wrote nothing");
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
