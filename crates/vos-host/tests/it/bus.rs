//! The one RPC bus, end to end over `/rpc`: a backend's request reaches the handlers the open pages registered, the
//! first page to answer wins, the others are told it is settled and a late answer is refused as expired; with no
//! handler anywhere it is method not found. A page's notification reaches the backend's listeners and the pages.

use crate::common::{browser, notification_where, publish_package, rpc, start, temp};
use serde_json::json;

/// A backend that asks its own pages to confirm, and hears what its page says it is typing.
const ASKER: &str = r#"
local typed = ""
RPC.on_notification("asker", "_typing", function(p) typed = p.text end)
return {
  clear = function()
    local ok, err = RPC.send_request("asker", "_client_confirm", { question = "Clear everything?" })
    if err then return { err = err.kind } end
    return { ok = ok }
  end,
  typed = function() return typed end,
}
"#;

#[tokio::test]
async fn a_request_reaches_every_page_that_serves_it_and_the_first_answer_wins() {
    let r = start(|_| {}).await;
    let url = publish_package(
        &temp("bus").join("asker"),
        "asker",
        "app.luau",
        &[("app.luau", ASKER.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let serve = json!({ "app": "asker", "method": "_client_confirm" });

    // two windows serve the confirmation; a third connection triggers it
    let (mut a, mut b, mut c) = (browser(&r).await, browser(&r).await, browser(&r).await);
    assert_eq!(rpc(&mut a, 1, "app.handle", serve.clone()).await["result"], json!(null));
    assert_eq!(rpc(&mut b, 1, "app.handle", serve.clone()).await["result"], json!(null));
    let asking = tokio::spawn(async move {
        let out = rpc(&mut c, 1, "app.call", json!({ "app": "asker", "method": "clear", "args": {} })).await;
        (c, out)
    });

    // both are asked the same request
    let to_a = notification_where(&mut a, "app.request", |p| p["method"] == "_client_confirm").await;
    let to_b = notification_where(&mut b, "app.request", |p| p["method"] == "_client_confirm").await;
    assert_eq!(to_a["call"], to_b["call"]);
    assert_eq!(to_a["params"], json!({ "question": "Clear everything?" }));
    let call = to_a["call"].clone();

    // b answers first and wins; a's answer comes too late, and a is told the request is settled
    assert_eq!(
        rpc(&mut b, 2, "app.answer", json!({ "call": call, "result": true })).await["result"],
        json!(null)
    );
    let (mut c, out) = asking.await.unwrap();
    assert_eq!(out["result"], json!({ "ok": true }), "{out}");
    let late = rpc(&mut a, 2, "app.answer", json!({ "call": call, "result": false })).await;
    assert_eq!(late["error"]["code"], -32013, "{late}");
    assert_eq!(notification_where(&mut a, "app.requestSettled", |p| p["call"] == call).await["call"], call);

    // with no page serving it any more, the request finds no handler
    rpc(&mut a, 3, "app.unhandle", serve.clone()).await;
    rpc(&mut b, 3, "app.unhandle", serve).await;
    let none = rpc(&mut c, 2, "app.call", json!({ "app": "asker", "method": "clear", "args": {} })).await;
    assert_eq!(none["result"], json!({ "err": "not_found" }), "{none}");
}

#[tokio::test]
async fn a_pages_notification_reaches_the_backend_and_the_pages() {
    let r = start(|_| {}).await;
    let url = publish_package(
        &temp("bus-notify").join("asker"),
        "asker",
        "app.luau",
        &[("app.luau", ASKER.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let mut page = browser(&r).await;
    let sent = rpc(
        &mut page,
        1,
        "app.notify",
        json!({ "app": "asker", "name": "_typing", "params": { "text": "hel" } }),
    )
    .await;
    assert_eq!(sent["result"], json!(null), "{sent}");
    let heard =
        notification_where(&mut page, "app.event", |p| p["app"] == "asker" && p["name"] == "_typing").await;
    assert_eq!(heard["data"], json!({ "text": "hel" }));
    let typed = rpc(&mut page, 2, "app.call", json!({ "app": "asker", "method": "typed", "args": {} })).await;
    assert_eq!(typed["result"], json!("hel"));
}

/// A backend whose public handlers describe themselves: they are agent tools, answered on the bus.
const TOOLS: &str = r#"
RPC.on_request("lookup", function(p, ctx)
  local wrote = pcall(function() app.kv:set("seen", p.q) end)
  return { q = p.q, who = ctx.actor.kind .. ":" .. ctx.actor.id, wrote = wrote }
end, {
  title = "Look up",
  description = "Look a thing up.",
  input_schema = { type = "object", properties = { q = { type = "string" } }, required = { "q" } },
  read_only = true,
})
RPC.on_request("_secret", function() return "no" end, { description = "Private: never a tool." })
"#;

#[tokio::test]
async fn agent_tools_are_the_public_handlers_on_the_bus_and_a_call_is_a_request_on_it() {
    let r = start(|c| c.agents = vec![crate::common::claude()]).await;
    let url = publish_package(
        &temp("bus-tools").join("tools"),
        "tools",
        "app.luau",
        &[("app.luau", TOOLS.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let (agent, _) = crate::common::connect_agent(r.addr, "tok-claude").await;
    let names =
        |names: Vec<String>| names.into_iter().filter(|n| n.starts_with("tools__")).collect::<Vec<_>>();

    // the backend's public handler is a tool, its private one is not; the call runs read-only, as the agent
    assert_eq!(names(crate::common::tool_names(&agent).await), ["tools__lookup"]);
    let out = crate::common::call(&agent, "tools__lookup", json!({ "q": "tea" })).await;
    let text = crate::common::text_of(&out);
    assert!(text.contains(r#"\"who\":\"agent:claude\""#) && text.contains(r#"\"wrote\":false"#), "{text}");

    // a page's public handler is a tool too, while the page serves it, and the agent's call is answered by the page
    let mut page = browser(&r).await;
    let about = json!({ "description": "Pick one.", "input_schema": { "type": "object" } });
    rpc(&mut page, 1, "app.handle", json!({ "app": "tools", "method": "pickOne", "about": about })).await;
    assert_eq!(names(crate::common::tool_names(&agent).await), ["tools__lookup", "tools__pick_one"]);
    let calling =
        tokio::spawn(async move { crate::common::call(&agent, "tools__pick_one", json!({})).await });
    let asked = notification_where(&mut page, "app.request", |p| p["method"] == "pickOne").await;
    rpc(&mut page, 2, "app.answer", json!({ "call": asked["call"], "result": "the red one" })).await;
    let picked = crate::common::text_of(&calling.await.unwrap());
    assert!(picked.contains("the red one"), "{picked}");

    // the page stops serving it: the tool goes
    rpc(&mut page, 3, "app.unhandle", json!({ "app": "tools", "method": "pickOne" })).await;
    let (agent, _) = crate::common::connect_agent(r.addr, "tok-claude").await;
    assert_eq!(names(crate::common::tool_names(&agent).await), ["tools__lookup"]);
}

/// An app with a contract and handlers that say nothing about themselves: the bus holds it to the contract.
const HELD: &str = r#"
local ran = 0
RPC.on_request("add", function(p) ran += 1 return { id = #p.text } end)
RPC.on_request("_ran", function() return { id = ran } end)
RPC.on_request("wrong", function() return { id = "not a number" } end)
RPC.on_request("shout", function()
  local ok = pcall(function() RPC.send_notification("undeclared", {}) end)
  RPC.send_notification("added", { id = 1 })
  return { id = if ok then 1 else 0 }
end)
"#;

const CONTRACT: &str = r##"{
  "openrpc": "1.4.0", "info": { "title": "Held", "version": "1.0.0" },
  "methods": [
    { "name": "add", "summary": "Add", "description": "Add a thing.", "x-readOnly": true,
      "params": [{ "name": "text", "required": true, "schema": { "type": "string", "minLength": 1 } }],
      "result": { "name": "item", "schema": { "$ref": "#/components/schemas/Item" } } },
    { "name": "_ran", "params": [], "result": { "name": "item", "schema": { "$ref": "#/components/schemas/Item" } } },
    { "name": "wrong", "params": [], "result": { "name": "item", "schema": { "$ref": "#/components/schemas/Item" } } },
    { "name": "shout", "params": [], "result": { "name": "item", "schema": { "$ref": "#/components/schemas/Item" } } },
    { "name": "added", "params": [{ "name": "id", "required": true, "schema": { "type": "integer" } }] }
  ],
  "components": { "schemas": { "Item": { "type": "object", "properties": { "id": { "type": "integer" } }, "required": ["id"] } } }
}"##;

#[tokio::test]
async fn the_bus_holds_every_request_answer_and_notification_to_the_installed_contract() {
    let r = start(|c| c.agents = vec![crate::common::claude()]).await;
    let manifest = json!({ "id": "held", "name": "held", "main": "app.luau", "ui": { "root": "static" }, "contract": "openrpc.json" });
    let url = crate::common::publish_app_ref(
        &temp("bus-held").join("held"),
        manifest,
        &[("app.luau", HELD.as_bytes().to_vec()), ("openrpc.json", CONTRACT.as_bytes().to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    let mut page = browser(&r).await;
    let call =
        |method: &str, args: serde_json::Value| json!({ "app": "held", "method": method, "args": args });

    // params the contract refuses never reach the handler; a method it does not declare reaches none
    let ok = rpc(&mut page, 1, "app.call", call("add", json!({ "text": "milk" }))).await;
    assert_eq!(ok["result"], json!({ "id": 4 }), "{ok}");
    let bad = rpc(&mut page, 2, "app.call", call("add", json!({ "text": "" }))).await;
    assert_eq!(bad["error"]["code"], -32602, "{bad}");
    let extra = rpc(&mut page, 3, "app.call", call("add", json!({ "text": "x", "more": 1 }))).await;
    assert_eq!(extra["error"]["code"], -32602, "{extra}");
    let unknown = rpc(&mut page, 4, "app.call", call("nope", json!({}))).await;
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
    let ran = rpc(&mut page, 8, "app.call", call("_ran", json!({}))).await;
    assert_eq!(
        ran["result"],
        json!({ "id": 1 }),
        "only the call the contract allowed ran the handler: {ran}"
    );

    // an answer the contract refuses is the handler's failure
    let wrong = rpc(&mut page, 5, "app.call", call("wrong", json!({}))).await;
    assert_eq!(wrong["error"]["code"], -32603, "{wrong}");

    // a notification the contract does not declare is not sent; one it declares is
    let shouted = rpc(&mut page, 6, "app.call", call("shout", json!({}))).await;
    assert_eq!(shouted["result"], json!({ "id": 1 }), "{shouted}");
    let heard = notification_where(&mut page, "app.event", |p| p["app"] == "held").await;
    assert_eq!(heard["name"], "added");
    let notified =
        rpc(&mut page, 7, "app.notify", json!({ "app": "held", "name": "undeclared", "params": {} })).await;
    assert!(notified["error"].is_object(), "{notified}");

    // the tool is the contract's: described from it, read-only as it says, though its handler said nothing
    let (agent, _) = crate::common::connect_agent(r.addr, "tok-claude").await;
    let tools = agent.peer().list_tools(None).await.unwrap().tools;
    let add = tools.iter().find(|t| t.name == "held__add").expect("held__add");
    assert_eq!(add.description.as_deref(), Some("Add a thing."));
    assert_eq!(add.input_schema.get("required"), Some(&json!(["text"])));
    assert_eq!(add.annotations.as_ref().and_then(|a| a.read_only_hint), Some(true));
}
