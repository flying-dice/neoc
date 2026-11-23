//! `lib:agui` against agents in this process. Every request goes to a local port; a server tells the test what it
//! saw through a channel, and nothing sleeps to wait for an event.

use std::future::Future;
use std::sync::Arc;

use mlua::Lua;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::lua::sandbox::make_engine;

async fn listen<F, Fut>(serve: F) -> u16
where
    F: Fn(TcpStream) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let serve = Arc::new(serve);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let serve = serve.clone();
            tokio::spawn(async move { serve(stream).await });
        }
    });
    port
}

/// The request on `stream`: its head and its body, as text.
async fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.expect("read");
        assert!(n > 0, "the request ended early");
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|v| v.parse().expect("length"))
                })
                .unwrap_or(0);
            if body.len() >= length {
                return (head.to_owned(), body.to_owned());
            }
        }
    }
}

const STREAM_HEAD: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\nconnection: close\r\n\r\n";

/// An agent that answers every run with `events` (each a JSON object) and closes, reporting each request.
async fn agent(
    events: &'static [&'static str],
) -> (u16, mpsc::UnboundedReceiver<(String, String)>) {
    let (seen, requests) = mpsc::unbounded_channel();
    let port = listen(move |mut stream| {
        let seen = seen.clone();
        async move {
            let _ = seen.send(read_request(&mut stream).await);
            let mut out = STREAM_HEAD.to_owned();
            for event in events {
                out.push_str(&format!("data: {event}\n\n"));
            }
            let _ = stream.write_all(out.as_bytes()).await;
        }
    })
    .await;
    (port, requests)
}

const PRELUDE: &str = r#"
local agui = require("lib:agui")
local function input() return { thread_id = "t", run_id = "r", messages = { { id = "1", role = "user", content = "hi" } } } end
-- the call must have failed: its kind
local function failed(r, e)
  assert(r == nil, "expected an error")
  return agui.error_kind(e) or ("not one of ours: " .. tostring(e))
end
"#;

async fn run(lua: &Lua, body: &str) -> String {
    lua.load(format!("{PRELUDE}\n{body}"))
        .eval_async()
        .await
        .unwrap_or_else(|e| panic!("script failed: {e}"))
}

#[tokio::test]
async fn a_run_posts_the_input_in_camel_case_and_yields_the_events_in_snake_case() {
    let (port, mut requests) = agent(&[
        r#"{"type":"RUN_STARTED","threadId":"t","runId":"r"}"#,
        r#"{"type":"TEXT_MESSAGE_START","messageId":"m"}"#,
        r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"Hel"}"#,
        r#"{"type":"FROM_THE_FUTURE","x":1}"#,
        r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"lo"}"#,
        r#"{"type":"TOOL_CALL_START","toolCallId":"c","toolCallName":"notes__add","parentMessageId":"m"}"#,
        r#"{"type":"STATE_SNAPSHOT","snapshot":{"camelKey":[1,2],"n":null}}"#,
        r#"{"type":"RUN_FINISHED","threadId":"t","runId":"r","usage":[{"inputTokens":3}]}"#,
    ])
    .await;
    let lua = make_engine().expect("engine");
    let got = run(
        &lua,
        &format!(
            r#"
            local i = input()
            i.tools = {{ {{ name = "notes__add", description = "Add a note", parameters = {{ type = "object" }} }} }}
            i.state = {{ count = 1 }}
            i.forwarded_props = {{ a2ui_action = {{ name = "ok" }} }}
            local run = assert(agui.run({{ url = "http://127.0.0.1:{port}/agent", input = i, headers = {{ Authorization = "Bearer s3" }} }}))
            local out, text = {{}}, ""
            while true do
              local event, err = run:next()
              assert(err == nil, err)
              if not event then break end
              table.insert(out, event.type)
              if event.type == "TEXT_MESSAGE_START" then assert(event.message_id == "m" and event.role == "assistant") end
              if event.type == "TEXT_MESSAGE_CONTENT" then text ..= event.delta end
              if event.type == "TOOL_CALL_START" then assert(event.tool_call_name == "notes__add" and event.parent_message_id == "m") end
              if event.type == "STATE_SNAPSHOT" then assert(event.snapshot.camelKey[2] == 2 and event.snapshot.n == nil) end
              if event.type == "RUN_FINISHED" then assert(event.usage[1].input_tokens == 3) end
            end
            local again, err = run:next()
            assert(again == nil and err == nil)
            return table.concat(out, " ") .. " | " .. text
            "#
        ),
    )
    .await;
    assert_eq!(
        got,
        "RUN_STARTED TEXT_MESSAGE_START TEXT_MESSAGE_CONTENT TEXT_MESSAGE_CONTENT TOOL_CALL_START STATE_SNAPSHOT RUN_FINISHED | Hello"
    );

    let (head, body) = requests.recv().await.expect("a request");
    let head = head.to_ascii_lowercase();
    assert!(head.starts_with("post /agent http/1.1"), "{head}");
    assert!(
        head.contains("accept: text/event-stream")
            && head.contains("content-type: application/json"),
        "{head}"
    );
    assert!(head.contains("authorization: bearer s3"), "{head}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        body,
        serde_json::json!({
            "threadId": "t", "runId": "r", "state": { "count": 1 },
            "messages": [{ "id": "1", "role": "user", "content": "hi" }],
            "tools": [{ "name": "notes__add", "description": "Add a note", "parameters": { "type": "object" } }],
            "context": [], "forwardedProps": { "a2ui_action": { "name": "ok" } },
        })
    );
}

#[tokio::test]
async fn what_is_not_a_run_is_an_error_with_a_kind() {
    let refuses = listen(|mut stream| async move {
        read_request(&mut stream).await;
        let _ = stream
            .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 9\r\nconnection: close\r\n\r\nno token.")
            .await;
    })
    .await;
    let moved = listen(|mut stream| async move {
        read_request(&mut stream).await;
        let _ = stream
            .write_all(b"HTTP/1.1 307 Temporary Redirect\r\nlocation: http://127.0.0.1:1/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
            .await;
    })
    .await;
    let json = listen(|mut stream| async move {
        read_request(&mut stream).await;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
            .await;
    })
    .await;
    let (bad_event, _requests) = agent(&[
        r#"{"type":"STEP_STARTED","stepName":"s"}"#,
        r#"{"type":"STEP_STARTED"}"#,
    ])
    .await;
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let lua = make_engine().expect("engine");
    let got = run(
        &lua,
        &format!(
            r#"
            local out = {{}}
            local function try(opts) table.insert(out, failed(agui.run(opts))) end
            local r, e = agui.run({{ url = "http://127.0.0.1:{refuses}/", input = input() }})
            assert(r == nil and string.find(e, "agui.run: status: the endpoint answered 401: no token.", 1, true), e)
            try({{ url = "http://127.0.0.1:{moved}/", input = input() }})
            try({{ url = "http://127.0.0.1:{json}/", input = input() }})
            try({{ url = "http://127.0.0.1:{closed}/", input = input() }})
            try({{ url = "ftp://example.com/", input = input() }})
            try({{ url = "http://127.0.0.1:{json}/" }})
            try({{ url = "http://127.0.0.1:{json}/", input = {{ thread_id = "t" }} }})
            try({{ url = "http://127.0.0.1:{json}/", input = {{ thread_id = "t", run_id = "r", messages = {{ {{ role = "robot" }} }} }} }})
            try({{ url = "http://127.0.0.1:{json}/", input = input(), follow_redirects = true }})
            try({{ url = "http://127.0.0.1:{json}/", input = input(), max_event = 0 }})
            try({{ input = input() }})

            local run = assert(agui.run({{ url = "http://127.0.0.1:{bad_event}/", input = input() }}))
            assert(assert(run:next()).step_name == "s")
            table.insert(out, failed(run:next()))
            local r, e = run:next()
            assert(r == nil and e == nil)
            assert(agui.error_kind("hyper.get: connect: x") == nil and agui.error_kind(nil) == nil)
            assert(agui.error_message("agui.run: status: the endpoint answered 404") == "the endpoint answered 404")
            assert(agui.error_message("hyper.get: connect: x") == nil and agui.error_message(nil) == nil)
            return table.concat(out, " ")
            "#
        ),
    )
    .await;
    assert_eq!(
        got,
        "status protocol connect invalid_url invalid_input invalid_input invalid_input invalid_input invalid_input invalid_input protocol"
    );
}

#[tokio::test]
async fn an_event_over_the_limit_a_cut_stream_and_a_silent_agent_end_the_run() {
    let endless = listen(|mut stream| async move {
        read_request(&mut stream).await;
        let _ = stream.write_all(STREAM_HEAD.as_bytes()).await;
        while stream
            .write_all(b"data: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n")
            .await
            .is_ok()
        {}
    })
    .await;
    let silent = listen(|mut stream| async move {
        read_request(&mut stream).await;
        let _ = stream.write_all(STREAM_HEAD.as_bytes()).await;
        let _ = stream
            .write_all(b"data: {\"type\":\"STEP_STARTED\",\"stepName\":\"s\"}\n\n")
            .await;
        std::future::pending::<()>().await;
    })
    .await;
    let lua = make_engine().expect("engine");
    let got = run(
        &lua,
        &format!(
            r#"
            local run = assert(agui.run({{ url = "http://127.0.0.1:{endless}/", input = input(), max_event = 4096 }}))
            local a = failed(run:next())
            run = assert(agui.run({{ url = "http://127.0.0.1:{silent}/", input = input(), idle_timeout = 0.2 }}))
            assert(run:next())
            local b = failed(run:next())
            run = assert(agui.run({{ url = "http://127.0.0.1:{silent}/", input = input(), timeout = 0.2 }}))
            assert(run:next())
            local c = failed(run:next())
            return a .. " " .. b .. " " .. c
            "#
        ),
    )
    .await;
    assert_eq!(got, "too_large timed_out timed_out");
}

#[tokio::test]
async fn a_closed_run_gives_out_nothing_more_even_what_was_already_read() {
    let (port, _requests) = agent(&[
        r#"{"type":"STEP_STARTED","stepName":"a"}"#,
        r#"{"type":"STEP_STARTED","stepName":"b"}"#,
        r#"{"type":"STEP_STARTED","stepName":"c"}"#,
    ])
    .await;
    let lua = make_engine().expect("engine");
    let got = run(
        &lua,
        &format!(
            r#"
            local run = assert(agui.run({{ url = "http://127.0.0.1:{port}/", input = input() }}))
            local first = assert(run:next())
            run:close()
            local event, err = run:next()
            return first.step_name .. " " .. tostring(event) .. " " .. tostring(err)
            "#
        ),
    )
    .await;
    assert_eq!(got, "a nil nil");
}

#[tokio::test]
async fn closing_a_run_closes_its_connection_and_so_does_shutting_the_engine_down() {
    let (gone, mut closed) = mpsc::unbounded_channel();
    let port = listen(move |mut stream| {
        let gone = gone.clone();
        async move {
            read_request(&mut stream).await;
            let _ = stream.write_all(STREAM_HEAD.as_bytes()).await;
            // the read ends when the client closes
            let mut rest = [0_u8; 16];
            let _ = stream.read(&mut rest).await;
            let _ = gone.send(());
        }
    })
    .await;
    let lua = make_engine().expect("engine");
    let got = run(
        &lua,
        &format!(
            r#"
            local run = assert(agui.run({{ url = "http://127.0.0.1:{port}/", input = input() }}))
            run:close()
            run:close()
            local event, err = run:next()
            held = assert(agui.run({{ url = "http://127.0.0.1:{port}/", input = input() }}))
            return tostring(event) .. " " .. tostring(err)
            "#
        ),
    )
    .await;
    assert_eq!(got, "nil nil");
    closed.recv().await.expect("the closed run's connection");
    crate::lua::lifecycle::Life::of(&lua).shutdown();
    closed.recv().await.expect("the held run's connection");
}

#[tokio::test]
async fn a_patch_gives_a_new_document_and_leaves_the_old_one() {
    let lua = make_engine().expect("engine");
    let got = run(
        &lua,
        r#"
        local state = { steps = { "a" }, n = 1 }
        local next = assert(agui.apply_patch(state, {
          { op = "add", path = "/steps/-", value = "b" },
          { op = "replace", path = "/n", value = 2 },
          { op = "add", path = "/made", value = { deep = true } },
        }))
        assert(#state.steps == 1 and state.n == 1)
        local same = assert(agui.apply_patch(next, {}))
        local a = failed(agui.apply_patch(next, { { op = "remove", path = "/missing" } }))
        local b = failed(agui.apply_patch(next, { { op = "explode" } }))
        return table.concat(same.steps, ",") .. " " .. same.n .. " " .. tostring(same.made.deep) .. " " .. a .. " " .. b
            .. " " .. #agui.EVENT_TYPES .. " " .. agui.EVENT_TYPES[1]
        "#,
    )
    .await;
    assert_eq!(
        got,
        "a,b 2 true invalid_input invalid_input 36 TEXT_MESSAGE_START"
    );
}
