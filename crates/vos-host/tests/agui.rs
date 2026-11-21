#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! `/agui` over real HTTP + SSE against the real wiring, with the scripted ACP agent (no model).

mod common;

use vos_domain::{AppId, Grants};
use common::{
    agents::{event_types, launchable, resume_body, run_body, sse_events},
    pair, publish_package, start, temp, Running,
};
use serde_json::{json, Value};
use std::time::Duration;

struct Chat {
    r: Running,
    cookie: String,
    http: reqwest::Client,
}

impl Chat {
    async fn start() -> Self {
        Self::start_granted(Grants::default()).await
    }

    async fn start_granted(grants: Grants) -> Self {
        let r = start(|c| c.agents = vec![("tok-fake".into(), launchable("fake", "Fake", grants))]).await;
        let cookie = pair(&r, "Safari on iPad").await;
        Self { r, cookie, http: reqwest::Client::new() }
    }

    fn post(&self, path: &str, body: &Value) -> reqwest::RequestBuilder {
        self.http.post(self.r.url(path)).header("cookie", &self.cookie).json(body)
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(self.r.url(path)).header("cookie", &self.cookie)
    }

    /// A run, read to the end of its stream.
    async fn run(&self, body: Value) -> Vec<Value> {
        let res = self.post("/agui", &body).send().await.unwrap();
        assert_eq!(res.status(), 200);
        assert!(res.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
        sse_events(&res.text().await.unwrap())
    }

    async fn thread(&self, id: &str) -> Value {
        let res = self.get(&format!("/agui/threads/{id}")).send().await.unwrap();
        assert_eq!(res.status(), 200);
        res.json().await.unwrap()
    }

    async fn ensure(&self, thread: &str) -> Value {
        let res =
            self.post("/agui/threads", &json!({"threadId": thread, "agentId": "fake"})).send().await.unwrap();
        assert_eq!(res.status(), 200, "{:?}", res.text().await);
        res.json().await.unwrap()
    }
}

fn text_of(events: &[Value]) -> String {
    events
        .iter()
        .filter(|e| e["type"] == "TEXT_MESSAGE_CONTENT")
        .map(|e| e["delta"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn a_thread_starts_the_agent_and_a_text_reply_streams_as_ag_ui_events() {
    let c = Chat::start().await;
    let snap = c.ensure("t1").await;
    assert_eq!(snap["agentId"], "fake");
    assert_eq!(snap["running"], false);
    assert_eq!(snap["state"]["session"]["agent"]["name"], "fake-agent");
    assert_eq!(snap["state"]["session"]["modes"]["current"], "default");
    assert_eq!(snap["state"]["session"]["config"][0]["id"], "model");
    assert_eq!(snap["state"]["session"]["toolsConnected"], true, "the host handed the agent /mcp");
    assert_eq!(c.ensure("t1").await["agentId"], "fake", "ensuring again is idempotent");

    let events = c.run(run_body("t1", "r1", "hello", "fake")).await;
    let types = event_types(&events);
    assert_eq!(types.first().map(String::as_str), Some("RUN_STARTED"));
    assert_eq!(types[1], "STATE_SNAPSHOT");
    assert!(
        types.contains(&"TEXT_MESSAGE_START".to_owned()) && types.contains(&"TEXT_MESSAGE_END".to_owned())
    );
    assert_eq!(types.last().map(String::as_str), Some("RUN_FINISHED"));
    assert_eq!(events[0]["threadId"], "t1");
    assert_eq!(events[0]["runId"], "r1");
    assert_eq!(text_of(&events), "Hi there");
    assert_eq!(events.last().unwrap()["outcome"], json!({"type": "success"}));
    assert_eq!(events.last().unwrap()["result"]["stopReason"], "end_turn");

    let snap = c.thread("t1").await;
    assert_eq!(snap["running"], false);
    let roles: Vec<&str> =
        snap["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant"]);
    assert_eq!(snap["messages"][1]["content"], "Hi there");
    c.r.stop().await;
}

#[tokio::test]
async fn reasoning_a_tool_call_and_state_changes_map_to_their_events() {
    let c = Chat::start().await;
    c.ensure("t1").await;
    let think = c.run(run_body("t1", "r1", "think", "fake")).await;
    let types = event_types(&think);
    for expected in [
        "REASONING_START",
        "REASONING_MESSAGE_START",
        "REASONING_MESSAGE_CONTENT",
        "REASONING_MESSAGE_END",
        "REASONING_END",
    ] {
        assert!(types.contains(&expected.to_owned()), "{expected} in {types:?}");
    }

    let tool = c.run(run_body("t1", "r2", "tool", "fake")).await;
    let types = event_types(&tool);
    assert!(
        types.windows(3).any(|w| w == ["TOOL_CALL_START", "TOOL_CALL_ARGS", "TOOL_CALL_END"]),
        "{types:?}"
    );
    let start = tool.iter().find(|e| e["type"] == "TOOL_CALL_START").unwrap();
    assert_eq!(start["toolCallName"], "mcp__agentos__notes__search");
    let statuses: Vec<&str> = tool
        .iter()
        .filter(|e| e["type"] == "CUSTOM" && e["name"] == "agentos.tool")
        .map(|e| e["value"]["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["in_progress", "completed"]);
    let result = tool.iter().find(|e| e["type"] == "TOOL_CALL_RESULT").unwrap();
    assert_eq!(result["content"], "2 notes");

    let state = c.run(run_body("t1", "r3", "state", "fake")).await;
    let paths: Vec<String> = state
        .iter()
        .filter(|e| e["type"] == "STATE_DELTA")
        .map(|e| e["delta"][0]["path"].as_str().unwrap().to_owned())
        .collect();
    for p in ["/session/commands", "/session/usage", "/plan", "/session/title", "/session/modes/current"] {
        assert!(paths.contains(&p.to_owned()), "{p} in {paths:?}");
    }
    let snap = c.thread("t1").await;
    assert_eq!(snap["state"]["session"]["usage"], json!({"used": 1200, "size": 200_000}));
    assert_eq!(snap["state"]["session"]["commands"][0]["name"], "compact");
    assert_eq!(snap["state"]["session"]["title"], "A titled chat");
    assert_eq!(snap["state"]["plan"][0]["content"], "write the test");
    // the tool call is in the history with its final status, and its output follows as a tool message
    let messages = snap["messages"].as_array().unwrap();
    let call = messages.iter().find(|m| m["toolCalls"].is_array()).unwrap();
    assert_eq!(call["agentos"]["status"], "completed");
    assert!(messages.iter().any(|m| m["role"] == "tool" && m["content"] == "2 notes"));
    c.r.stop().await;
}

#[tokio::test]
async fn a_permission_request_interrupts_the_run_and_the_resume_finishes_the_turn() {
    let c = Chat::start().await;
    c.ensure("t1").await;
    let first = c.run(run_body("t1", "r1", "ask-write", "fake")).await;
    let last = first.last().unwrap();
    assert_eq!(last["type"], "RUN_FINISHED");
    assert_eq!(last["outcome"]["type"], "interrupt");
    let interrupt = &last["outcome"]["interrupts"][0];
    assert_eq!(interrupt["reason"], "tool_call");
    assert_eq!(interrupt["message"], "mcp__agentos__notes__append");
    let options: Vec<&str> = interrupt["metadata"]["agentos:options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["optionId"].as_str().unwrap())
        .collect();
    assert_eq!(options, ["allow", "always", "deny"]);
    assert_eq!(
        interrupt["responseSchema"]["properties"]["optionId"]["enum"],
        json!(["allow", "always", "deny"])
    );
    let id = interrupt["id"].as_str().unwrap().to_owned();

    // "reload": the thread still offers the pending request, and is still running
    let snap = c.thread("t1").await;
    assert_eq!(snap["running"], true);
    assert_eq!(snap["interrupts"][0]["id"], id.as_str());

    // a new message while waiting, and an answer for a request that is not there, are RUN_ERRORs
    let blocked = c.run(run_body("t1", "r2", "hello", "fake")).await;
    assert_eq!(blocked.last().unwrap()["type"], "RUN_ERROR");
    assert_eq!(blocked.last().unwrap()["code"], "interrupts_pending");
    let unknown = c.run(resume_body("t1", "r2", "nope", Some("allow"))).await;
    assert_eq!(unknown.last().unwrap()["code"], "unknown_interrupt");

    let rest = c.run(resume_body("t1", "r3", &id, Some("allow"))).await;
    assert_eq!(rest[0]["type"], "RUN_STARTED");
    assert_eq!(rest[0]["runId"], "r3");
    assert!(text_of(&rest).ends_with("Answer: selected allow"), "{}", text_of(&rest));
    assert_eq!(rest.last().unwrap()["outcome"]["type"], "success");
    let snap = c.thread("t1").await;
    assert_eq!(snap["running"], false);
    assert!(snap["interrupts"].as_array().unwrap().is_empty());

    // denying: the agent is told the request was cancelled
    let first = c.run(run_body("t1", "r4", "ask-write", "fake")).await;
    let id = first.last().unwrap()["outcome"]["interrupts"][0]["id"].as_str().unwrap().to_owned();
    let rest = c.run(resume_body("t1", "r5", &id, None)).await;
    assert!(text_of(&rest).ends_with("Answer: cancelled"), "{}", text_of(&rest));
    c.r.stop().await;
}

#[tokio::test]
async fn a_read_only_tool_needs_no_prompt_under_the_default_policy_but_a_writing_one_does() {
    // the agent is granted the notes app: the prompt is only skipped for a tool it may call
    let c = Chat::start_granted(Grants::apps([AppId::parse("notes").unwrap()])).await;
    // the scripted agent asks about notes__search (readOnly) and notes__append: install an app that has both
    let url = publish_package(
        &temp("notes-policy").join("pkg"),
        "notes",
        "app.lua",
        &[(
            "app.lua",
            br#"app.manifest { id = "notes", name = "Notes", icon = "gear" }
app.tool { name = "search", readOnly = true, run = function() return {} end }
app.tool { name = "append", run = function() return {} end }"#
                .to_vec(),
        )],
    );
    c.r.host.service.install_from_url(&url).await.unwrap();
    c.ensure("t1").await;
    let read = c.run(run_body("t1", "r1", "ask", "fake")).await;
    assert_eq!(read.last().unwrap()["outcome"]["type"], "success", "{read:?}");
    assert!(text_of(&read).ends_with("Answer: selected allow"), "answered for the agent: {}", text_of(&read));
    let write = c.run(run_body("t1", "r2", "ask-write", "fake")).await;
    assert_eq!(write.last().unwrap()["outcome"]["type"], "interrupt", "the writing tool asks the person");
    c.r.stop().await;
}

#[tokio::test]
async fn a_read_only_tool_of_an_app_the_agent_is_not_granted_still_asks() {
    let c = Chat::start().await; // no grants
    let url = publish_package(
        &temp("notes-ungranted").join("pkg"),
        "notes",
        "app.lua",
        &[(
            "app.lua",
            br#"app.manifest { id = "notes", name = "Notes", icon = "gear" }
app.tool { name = "search", readOnly = true, run = function() return {} end }"#
                .to_vec(),
        )],
    );
    c.r.host.service.install_from_url(&url).await.unwrap();
    c.ensure("t1").await;
    let read = c.run(run_body("t1", "r1", "ask", "fake")).await;
    assert_eq!(read.last().unwrap()["outcome"]["type"], "interrupt", "{read:?}");
    c.r.stop().await;
}

#[tokio::test]
async fn revoking_a_device_ends_its_open_event_streams() {
    let c = Chat::start().await;
    c.ensure("t1").await;
    let device = {
        let res = c.http.get(c.r.url("/session")).header("cookie", &c.cookie).send().await.unwrap();
        res.json::<Value>().await.unwrap()["device"]["id"].as_str().unwrap().to_owned()
    };
    let (http, url, cookie) = (c.http.clone(), c.r.url("/agui"), c.cookie.clone());
    let body = run_body("t1", "r1", "slow", "fake");
    let streaming = tokio::spawn(async move {
        let res = http.post(url).header("cookie", cookie).json(&body).send().await.unwrap();
        sse_events(&res.text().await.unwrap())
    });
    for _ in 0..200 {
        if c.thread("t1").await["running"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    c.r.devices().revoke(&vos_domain::DeviceId(device)).await.unwrap();
    // the "slow" turn would stream until cancelled: the stream ends because the device is gone
    let events = tokio::time::timeout(Duration::from_secs(10), streaming)
        .await
        .expect("the stream of a revoked device ends")
        .unwrap();
    assert!(events.iter().all(|e| e["type"] != "RUN_FINISHED"), "cut off, not finished: {events:?}");
    c.r.stop().await;
}

#[tokio::test]
async fn cancel_ends_the_turn_and_so_does_a_client_that_goes_away() {
    let c = Chat::start().await;
    c.ensure("t1").await;
    // explicit cancel
    let (http, url, cookie) = (c.http.clone(), c.r.url("/agui"), c.cookie.clone());
    let body = run_body("t1", "r1", "slow", "fake");
    let streaming = tokio::spawn(async move {
        let res = http.post(url).header("cookie", cookie).json(&body).send().await.unwrap();
        sse_events(&res.text().await.unwrap())
    });
    let mut running = false;
    for _ in 0..200 {
        if c.thread("t1").await["running"] == true {
            running = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(running, "the run is going");
    let res = c.post("/agui/threads/t1/cancel", &json!({})).send().await.unwrap();
    assert_eq!(res.status(), 204);
    let events = tokio::time::timeout(Duration::from_secs(10), streaming).await.unwrap().unwrap();
    assert_eq!(events.last().unwrap()["result"]["stopReason"], "cancelled", "{events:?}");
    assert_eq!(c.thread("t1").await["running"], false);

    // abort: the client drops the connection mid-run, which the host turns into a cancel
    let res = c.post("/agui", &run_body("t1", "r2", "slow", "fake")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    drop(res);
    let mut gone = false;
    for _ in 0..200 {
        if c.thread("t1").await["running"] == false {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(gone, "the abandoned run was cancelled");
    // and the thread is usable again
    assert_eq!(text_of(&c.run(run_body("t1", "r3", "hello", "fake")).await), "Hi there");

    assert_eq!(c.post("/agui/threads/ghost/cancel", &json!({})).send().await.unwrap().status(), 404);
    c.r.stop().await;
}

#[tokio::test]
async fn session_settings_change_through_the_settings_endpoint() {
    let c = Chat::start().await;
    c.ensure("t1").await;
    let state: Value = c
        .post("/agui/threads/t1/settings", &json!({"mode": "plan"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["session"]["modes"]["current"], "plan");
    let state: Value = c
        .post("/agui/threads/t1/settings", &json!({"config": {"id": "model", "value": "sonnet"}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["session"]["config"][0]["current"], "sonnet");
    assert_eq!(
        c.thread("t1").await["state"]["session"]["config"][0]["current"],
        "sonnet",
        "kept in the thread"
    );

    for (body, status) in [
        (json!({"mode": "yolo"}), 422),
        (json!({"config": {"id": "nope", "value": "x"}}), 422),
        (json!({"bogus": 1}), 422),
    ] {
        assert_eq!(
            c.post("/agui/threads/t1/settings", &body).send().await.unwrap().status(),
            status,
            "{body}"
        );
    }
    assert_eq!(
        c.post("/agui/threads/ghost/settings", &json!({"mode": "plan"})).send().await.unwrap().status(),
        404
    );
    c.r.stop().await;
}

#[tokio::test]
async fn bad_requests_are_refused_before_a_stream_starts() {
    let c = Chat::start().await;
    for (body, status) in [
        (json!({"runId": "r"}), 422),                  // no threadId
        (json!({"threadId": "t", "runId": "r"}), 404), // unknown thread, no agent
        (run_body("t", "r", "hi", "ghost"), 404),      // unknown agent
        (json!({"threadId": "t", "runId": "r", "forwardedProps": {"agentId": "fake"}}), 422), // no user message
    ] {
        let res = c.post("/agui", &body).send().await.unwrap();
        assert_eq!(res.status(), status, "{body}");
        assert!(res.json::<Value>().await.unwrap()["error"].is_string(), "{body}");
    }
    let not_json =
        c.http.post(c.r.url("/agui")).header("cookie", &c.cookie).body("{ nope").send().await.unwrap();
    assert_eq!(not_json.status(), 422);
    assert!(not_json.json::<Value>().await.unwrap()["error"].as_str().unwrap().contains("RunAgentInput"));
    assert_eq!(c.post("/agui/threads", &json!({"threadId": "t"})).send().await.unwrap().status(), 422);
    assert_eq!(
        c.post("/agui/threads", &json!({"threadId": "t", "agentId": "ghost"})).send().await.unwrap().status(),
        404
    );
    assert_eq!(c.get("/agui/threads/ghost").send().await.unwrap().status(), 404);
    c.r.stop().await;
}

#[tokio::test]
async fn an_unpaired_request_is_refused_on_every_agui_route() {
    let c = Chat::start().await;
    let anonymous = reqwest::Client::new();
    for (method, path) in [
        ("POST", "/agui"),
        ("POST", "/agui/threads"),
        ("GET", "/agui/threads/t1"),
        ("POST", "/agui/threads/t1/cancel"),
        ("POST", "/agui/threads/t1/settings"),
    ] {
        let req = if method == "GET" {
            anonymous.get(c.r.url(path))
        } else {
            anonymous.post(c.r.url(path)).json(&json!({}))
        };
        assert_eq!(req.send().await.unwrap().status(), 401, "{method} {path}");
    }
    let forged = anonymous
        .post(c.r.url("/agui"))
        .header("cookie", "agentos_session=made-up")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(forged.status(), 401);
    // the agent's MCP token is not a session either
    let token = anonymous
        .post(c.r.url("/agui"))
        .header("authorization", "Bearer tok-fake")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(token.status(), 401);
    c.r.stop().await;
}

#[tokio::test]
async fn the_transcript_survives_a_host_restart_and_the_next_prompt_opens_a_new_session() {
    let tmp = temp("agui-restart");
    vos_host::startup::prepare_data_dir(&tmp.join("data")).unwrap();
    let configure = |c: &mut vos_host::HostConfig| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![("tok-fake".into(), launchable("fake", "Fake", Grants::default()))];
    };
    let http = reqwest::Client::new();
    let run = |r: &Running, cookie: &str, body: Value| {
        let (http, url, cookie) = (http.clone(), r.url("/agui"), cookie.to_owned());
        async move {
            sse_events(
                &http
                    .post(url)
                    .header("cookie", cookie)
                    .json(&body)
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
            )
        }
    };
    let r = start(configure).await;
    let cookie = pair(&r, "Mac").await;
    assert_eq!(text_of(&run(&r, &cookie, run_body("t1", "r1", "hello", "fake")).await), "Hi there");
    r.stop().await;

    let r = start(configure).await;
    let cookie = pair(&r, "Mac").await;
    let snapshot = |r: &Running, cookie: &str| {
        let (http, url, cookie) = (http.clone(), r.url("/agui/threads/t1"), cookie.to_owned());
        async move { http.get(url).header("cookie", cookie).send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let snap = snapshot(&r, &cookie).await;
    assert_eq!(snap["messages"].as_array().unwrap().len(), 2, "history is in system.db");
    assert_eq!(snap["running"], false);
    assert_eq!(text_of(&run(&r, &cookie, run_body("t1", "r2", "hello", "fake")).await), "Hi there");
    assert_eq!(snapshot(&r, &cookie).await["messages"].as_array().unwrap().len(), 4);
    r.stop().await;
}
