#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! What one app's handlers can do to the host, from the host's own side: a client that ignores every courtesy (the shell's per-app
//! queue, its fairness) and sends calls straight at `/rpc`. The host holds the line itself: an app runs at most
//! `MAX_CALLS_PER_APP` handler calls at once, the ninth is answered busy at once, `notifications/cancelled` ends a running call
//! and frees its place, a connection closing frees everything, and the shell's own calls and every other app go on throughout.

use crate::common;

use common::{browser, publish_package_with_client, rpc, start, temp, Running, Ws};
use futures_util::SinkExt;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use vos_application::MAX_CALLS_PER_APP;
use vos_host::startup::prepare_data_dir;

const BACKEND: &str = r#"
local task = require("std:task")
local generation = 0
app.tool {
  name = "release_all",
  input = {},
  run = function() generation += 1 return { released = true } end,
}
return {
  hang = function()
    local mine = generation
    while generation == mine do task.wait(0.05) end
    return "released"
  end,
  ping = function() return "pong" end,
}
"#;

async fn host_with_apps(tmp: &Path, ids: &[&str]) -> Running {
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| c.data_dir = Some(tmp.join("data"))).await;
    for id in ids {
        let files = [("app.luau", BACKEND.as_bytes().to_vec()), ("static/index.html", b"<p>x</p>".to_vec())];
        let url =
            publish_package_with_client(&tmp.join(format!("pkg-{id}")), id, "app.luau", "static", &files);
        r.host.service.install_from_url(&url).await.unwrap();
    }
    r
}

async fn send(ws: &mut Ws, id: i64, method: &str, params: Value) {
    ws.raw
        .send(Message::text(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()))
        .await
        .unwrap();
}

async fn cancel(ws: &mut Ws, id: i64) {
    ws.raw
        .send(Message::text(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"test"}})
                .to_string(),
        ))
        .await
        .unwrap();
}

/// The next frame with an id, within `within`; `None` when none came.
async fn answer_within(ws: &mut Ws, within: Duration) -> Option<Value> {
    if let Some(i) = ws.backlog.iter().position(|m| m.get("id").is_some()) {
        return Some(ws.backlog.remove(i));
    }
    loop {
        let frame = tokio::time::timeout(within, common::next_json(&mut ws.raw)).await.ok()?;
        if frame.get("id").is_some() {
            return Some(frame);
        }
        ws.backlog.push(frame);
    }
}

fn is_busy(answer: &Value) -> bool {
    answer["error"]["code"] == -32000
}

/// Fill an app's places: nine calls of `hang` for eight places, which is deterministic whatever order they are taken in (the one that
/// loses is refused at once); returns once that refusal has come, so eight are running.
async fn fill(ws: &mut Ws, app: &str) {
    for id in 1..=(i64::try_from(MAX_CALLS_PER_APP).unwrap() + 1) {
        send(ws, id, "app.call", json!({"app": app, "method": "hang"})).await;
    }
    let refused = answer_within(ws, Duration::from_secs(5)).await.expect("the ninth call is refused at once");
    assert!(is_busy(&refused), "{refused}");
}

/// A `ping` of `app` until it is not refused as busy (a freed place comes back a moment after the request that held it ended).
async fn ping_until_taken(ws: &mut Ws, app: &str, first_id: i64) -> Value {
    for n in 0..200 {
        let a = rpc(ws, first_id + n, "app.call", json!({"app": app, "method": "ping"})).await;
        if !is_busy(&a) {
            return a;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the app never took a call again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hung_app_holds_eight_places_and_nobody_else_feels_it() {
    let tmp = temp("call-limits");
    let r = host_with_apps(&tmp, &["hung", "other"]).await;
    let mut ws = browser(&r).await;

    // twelve calls of a handler that never returns, straight at the host: the first eight run, the rest are refused at once
    for id in 1..=12 {
        send(&mut ws, id, "app.call", json!({"app": "hung", "method": "hang"})).await;
    }
    let mut refused = vec![];
    for _ in 0..(12 - MAX_CALLS_PER_APP) {
        let a = answer_within(&mut ws, Duration::from_secs(3))
            .await
            .expect("the refused ones are answered at once");
        assert!(is_busy(&a), "{a}");
        assert!(a["error"]["message"].as_str().unwrap().contains("already has 8 calls running"), "{a}");
        refused.push(a["id"].as_i64().unwrap());
    }
    refused.sort_unstable();
    assert_eq!(refused.len(), 4, "exactly the four over the limit were refused: {refused:?}");
    // the calls are taken concurrently, so which four lose is not fixed: any of the others is a running call
    let running: Vec<i64> = (1..=12).filter(|id| !refused.contains(id)).collect();

    // the shell's own calls and another app are untouched, and the hung app's other calls are refused with a reason
    let list = rpc(&mut ws, 20, "app.list", json!({})).await;
    assert!(list["result"]["apps"].is_array());
    let other = rpc(&mut ws, 21, "app.call", json!({"app": "other", "method": "ping"})).await;
    assert_eq!(other["result"], "pong");
    let hung = rpc(&mut ws, 22, "app.call", json!({"app": "hung", "method": "ping"})).await;
    assert!(is_busy(&hung), "{hung}");
    assert!(
        answer_within(&mut ws, Duration::from_millis(300)).await.is_none(),
        "the eight are still running"
    );

    // cancelling one running call ends it, frees its place, and sends it no reply
    let victim = running[2];
    cancel(&mut ws, victim).await;
    let pong = ping_until_taken(&mut ws, "hung", 100).await;
    assert_eq!(pong["result"], "pong");
    assert!(
        ws.backlog.iter().all(|m| m["id"] != victim),
        "a cancelled request gets no reply: {:?}",
        ws.backlog
    );

    // an unknown or finished id, and a malformed cancellation, are ignored
    cancel(&mut ws, 9_999).await;
    ws.raw
        .send(Message::text(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        rpc(&mut ws, 300, "app.call", json!({"app": "other", "method": "ping"})).await["result"],
        "pong"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_closing_frees_every_place_its_calls_held() {
    let tmp = temp("call-limits-close");
    let r = host_with_apps(&tmp, &["hung"]).await;
    let mut first = browser(&r).await;
    fill(&mut first, "hung").await;
    let mut second = browser(&r).await;
    // while the first connection holds them all, a second connection is refused too: the limit is the app's, not the connection's
    let refused = rpc(&mut second, 1, "app.call", json!({"app": "hung", "method": "ping"})).await;
    assert!(is_busy(&refused), "{refused}");
    drop(first); // the socket goes
    let pong = ping_until_taken(&mut second, "hung", 10).await;
    assert_eq!(pong["result"], "pong");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uninstalling_a_hung_app_answers_its_calls_and_a_reinstall_starts_clean() {
    let tmp = temp("call-limits-uninstall");
    let r = host_with_apps(&tmp, &["hung"]).await;
    let mut ws = browser(&r).await;
    fill(&mut ws, "hung").await;
    let gone = rpc(&mut ws, 400, "app.uninstall", json!({"app": "hung", "purge": true})).await;
    assert!(gone.get("error").is_none(), "{gone}");
    // every held call is answered (an error saying the app was unloaded), none is left running on the host
    let mut answered = 0;
    while answered < MAX_CALLS_PER_APP {
        let a = answer_within(&mut ws, Duration::from_secs(5))
            .await
            .expect("a held call is answered when its app goes");
        assert!(a["error"].is_object(), "{a}");
        answered += 1;
    }
    let files = [("app.luau", BACKEND.as_bytes().to_vec()), ("static/index.html", b"<p>x</p>".to_vec())];
    let url = publish_package_with_client(&tmp.join("pkg-again"), "hung", "app.luau", "static", &files);
    r.host.service.install_from_url(&url).await.unwrap();
    assert_eq!(
        rpc(&mut ws, 60, "app.call", json!({"app": "hung", "method": "ping"})).await["result"],
        "pong"
    );
}
