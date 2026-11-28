//! `vnd:agent_client_protocol` against agents in this process, over a real WebSocket on a local port, or over lines
//! a script's transport reads and writes. An agent tells the test what it was sent through a channel, and nothing
//! sleeps to wait for an event.

use std::future::Future;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mlua::Lua;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderMap;

use neoc_vm_core::lifecycle::{Role, async_function};
use neoc_vm_core::make_engine;

/// How an agent's messages travel: a WebSocket, or the lines a script's transport reads and writes.
enum Wire {
    Ws(Box<WebSocketStream<TcpStream>>),
    Lines {
        /// What the script's `write` sent.
        from_client: mpsc::UnboundedReceiver<String>,
        /// What the script's `read` gives; `None` once the agent hung up.
        to_client: Option<mpsc::UnboundedSender<String>>,
    },
}

/// An agent's end of one connection, and the headers of the upgrade that opened it (none over lines).
struct Agent {
    wire: Wire,
    headers: HeaderMap,
}

impl Agent {
    /// The next message the client sent; `None` once it closed the connection.
    async fn recv(&mut self) -> Option<Value> {
        match &mut self.wire {
            Wire::Ws(ws) => {
                while let Some(Ok(message)) = ws.next().await {
                    match message {
                        Message::Text(text) => {
                            return Some(serde_json::from_str(text.as_str()).expect("json"));
                        }
                        Message::Close(_) => return None,
                        _ => {}
                    }
                }
                None
            }
            Wire::Lines { from_client, .. } => {
                from_client.recv().await.map(|line| serde_json::from_str(&line).expect("json"))
            }
        }
    }

    async fn send(&mut self, message: Value) {
        match &mut self.wire {
            Wire::Ws(ws) => {
                let _ = ws.send(Message::text(message.to_string())).await;
            }
            Wire::Lines { to_client, .. } => {
                if let Some(to_client) = to_client {
                    let _ = to_client.send(message.to_string());
                }
            }
        }
    }

    /// Hang up: a close frame, or the end of what the script reads.
    async fn close(&mut self) {
        match &mut self.wire {
            Wire::Ws(ws) => {
                let _ = WebSocketStream::close(ws, None).await;
            }
            Wire::Lines { to_client, .. } => *to_client = None,
        }
    }

    async fn answer(&mut self, request: &Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": result })).await;
    }

    async fn refuse(&mut self, request: &Value, code: i64, message: &str) {
        self.send(
            json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": code, "message": message } }),
        )
        .await;
    }

    async fn update(&mut self, session: &str, update: Value) {
        self.send(json!({
            "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": session, "update": update }
        }))
        .await;
    }

    /// Read `initialize` and answer it as an ACP v1 agent that can load sessions; the request.
    async fn initialize(&mut self) -> Value {
        let request = self.recv().await.expect("initialize");
        assert_eq!(request["method"], "initialize");
        self.answer(&request, json!({ "protocolVersion": 1, "agentCapabilities": { "loadSession": true } }))
            .await;
        request
    }

    /// Read `session/new` and answer it with `session`.
    async fn new_session(&mut self, session: &str) -> Value {
        let request = self.recv().await.expect("session/new");
        assert_eq!(request["method"], "session/new");
        self.answer(&request, json!({ "sessionId": session })).await;
        request
    }
}

fn text(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

/// A port where every WebSocket upgrade is accepted and handed to `serve`.
async fn agents<F, Fut>(serve: F) -> u16
where
    F: Fn(Agent) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let serve = Arc::new(serve);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let serve = serve.clone();
            tokio::spawn(async move {
                let (seen, headers) = std::sync::mpsc::channel();
                // tungstenite's callback type: its error is an HTTP response
                #[allow(clippy::result_large_err)]
                let upgraded = tokio_tungstenite::accept_hdr_async(
                    stream,
                    move |request: &Request, response: Response| {
                        let _ = seen.send(request.headers().clone());
                        Ok(response)
                    },
                )
                .await;
                if let Ok(ws) = upgraded {
                    serve(Agent {
                        wire: Wire::Ws(Box::new(ws)),
                        headers: headers.recv().expect("the headers"),
                    })
                    .await;
                }
            });
        }
    });
    port
}

/// A port that answers every request with `response`, as it is.
async fn answers(response: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut chunk = [0_u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

const PRELUDE: &str = r#"
local acp = require("vnd:agent_client_protocol")
-- the call must have failed: its kind
local function failed(r, e)
  assert(r == nil, "expected an error")
  return e.kind
end
"#;

async fn run(lua: &Lua, body: &str) -> String {
    lua.load(format!("{PRELUDE}\n{body}")).eval_async().await.unwrap_or_else(|e| panic!("script failed: {e}"))
}

fn engine(url: &str) -> Lua {
    let lua = make_engine().expect("engine");
    lua.globals().set("URL", url).expect("global");
    lua
}

/// An engine with an agent served by `serve` in this process, over lines: the globals `agent_read()` (it waits for
/// the agent's next message, `nil` once the agent hung up) and `agent_write(line)` are what a script's transport for
/// `connect_lines` calls, as it would read and write a container's streams.
fn lines_engine<F, Fut>(serve: F) -> Lua
where
    F: FnOnce(Agent) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let lua = make_engine().expect("engine");
    let (to_agent, from_client) = mpsc::unbounded_channel::<String>();
    let (to_client, from_agent) = mpsc::unbounded_channel::<String>();
    tokio::spawn(serve(Agent {
        wire: Wire::Lines { from_client, to_client: Some(to_client) },
        headers: HeaderMap::new(),
    }));
    let from_agent = Arc::new(tokio::sync::Mutex::new(from_agent));
    let read = async_function(&lua, Role::Wait, move |_, ()| {
        let from_agent = from_agent.clone();
        async move { Ok(from_agent.lock().await.recv().await) }
    })
    .expect("agent_read");
    let write = lua
        .create_function(move |_, line: String| {
            let _ = to_agent.send(line);
            Ok(())
        })
        .expect("agent_write");
    lua.globals().set("agent_read", read).expect("global");
    lua.globals().set("agent_write", write).expect("global");
    lua
}

#[tokio::test]
async fn a_turn_streams_the_agents_updates_asks_for_permission_and_ends_with_the_answer() {
    let (seen, mut said) = mpsc::unbounded_channel::<Value>();
    let port = agents(move |mut agent| {
        let seen = seen.clone();
        async move {
            let headers = &agent.headers;
            let _ = seen.send(json!({
                "authorization": headers.get("authorization").and_then(|v| v.to_str().ok()),
                "origin": headers.get("origin").and_then(|v| v.to_str().ok()),
            }));
            let initialize = agent.recv().await.expect("initialize");
            let _ = seen.send(initialize.clone());
            agent
                .answer(
                    &initialize,
                    json!({
                        "protocolVersion": 1,
                        "agentCapabilities": {
                            "loadSession": true, "promptCapabilities": { "image": true },
                            "mcpCapabilities": { "http": true }, "sessionCapabilities": {}
                        },
                        "agentInfo": { "name": "fake", "title": "Fake", "version": "1.0" },
                        "authMethods": [{ "id": "login", "name": "Log in" }],
                    }),
                )
                .await;
            let new = agent.new_session("s1").await;
            let _ = seen.send(new);
            // sent between turns: the next turn gives it
            agent
                .update(
                    "s1",
                    json!({
                        "sessionUpdate": "available_commands_update",
                        "availableCommands": [{ "name": "review", "description": "Review", "input": { "hint": "what" } }]
                    }),
                )
                .await;
            let prompt = agent.recv().await.expect("session/prompt");
            let _ = seen.send(prompt.clone());

            agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("Hel") })).await;
            // a session this connection does not have
            agent.update("s2", json!({ "sessionUpdate": "agent_message_chunk", "content": text("no") })).await;
            agent.update("s1", json!({ "sessionUpdate": "agent_thought_chunk", "content": text("hmm") })).await;
            // a request the client offers nothing for
            agent
                .send(json!({
                    "jsonrpc": "2.0", "id": 100, "method": "fs/read_text_file",
                    "params": { "sessionId": "s1", "path": "/etc/passwd" }
                }))
                .await;
            agent
                .update(
                    "s1",
                    json!({
                        "sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Edit a", "kind": "edit",
                        "status": "pending", "locations": [{ "path": "/w/a", "line": 3 }],
                        "content": [{ "type": "diff", "path": "/w/a", "oldText": "x", "newText": "y" }],
                        "rawInput": { "filePath": "/w/a", "n": null }
                    }),
                )
                .await;
            agent
                .send(json!({
                    "jsonrpc": "2.0", "id": "p1", "method": "session/request_permission",
                    "params": {
                        "sessionId": "s1", "toolCall": { "toolCallId": "c1", "title": "Edit a" },
                        "options": [
                            { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
                            { "optionId": "no", "name": "Reject", "kind": "reject_once" }
                        ]
                    }
                }))
                .await;
            // the refusal of the fs request, then the permission's answer
            for _ in 0..2 {
                let _ = seen.send(agent.recv().await.expect("an answer"));
            }
            agent
                .update("s1", json!({ "sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed" }))
                .await;
            agent
                .update(
                    "s1",
                    json!({
                        "sessionUpdate": "plan",
                        "entries": [{ "content": "edit a", "priority": "high", "status": "in_progress" }]
                    }),
                )
                .await;
            agent.update("s1", json!({ "sessionUpdate": "from_the_future", "used": 1 })).await;
            // a known kind that is not what ACP says: given as it was sent
            agent.update("s1", json!({ "sessionUpdate": "plan", "entries": "none" })).await;
            agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("lo") })).await;
            agent.answer(&prompt, json!({ "stopReason": "end_turn" })).await;
            let _ = seen.send(json!({ "after": agent.recv().await }));
        }
    })
    .await;

    let lua = engine(&format!("ws://127.0.0.1:{port}/acp?agentId=fake"));
    let got = run(
        &lua,
        r#"
        local conn = assert(acp.connect(URL, { headers = { Authorization = "Bearer t" }, client_info = { name = "vos", version = "2" } }))
        local agent = conn.agent
        assert(agent.protocol_version == 1 and agent.agent_info.name == "fake" and agent.agent_info.title == "Fake")
        local caps = agent.agent_capabilities
        assert(caps.load_session and caps.prompt_capabilities.image and not caps.prompt_capabilities.audio)
        assert(caps.mcp_capabilities.http and not caps.mcp_capabilities.sse)
        assert(#agent.auth_methods == 1 and agent.auth_methods[1].id == "login")
        assert(conn.local_address == "127.0.0.1", conn.local_address)

        local session = assert(conn:new_session({
          cwd = "/w",
          mcp_servers = {
            { type = "http", name = "vos", url = "http://10.0.0.1:1/mcp", headers = { { name = "Authorization", value = "Bearer k" } } },
            { type = "stdio", name = "local", command = "run", args = { "a" }, env = { { name = "K", value = "v" } } },
            -- no arguments, variables or headers: a script's empty table is an empty list
            { type = "stdio", name = "bare", command = "go", args = {}, env = {} },
            { type = "http", name = "open", url = "http://10.0.0.1:2/mcp", headers = {} },
          },
        }))
        assert(session.session_id == "s1" and session.modes == nil)
        -- a session this connection did not start or load
        assert(failed(conn:prompt({ session_id = "s9", prompt = {} })) == "invalid_input")
        local turn = assert(conn:prompt({ session_id = "s1", prompt = { { type = "text", text = "hi" } } }))
        local out, text = {}, ""
        while true do
          local event, err = turn:next()
          assert(err == nil, tostring(err))
          if not event then break end
          if event.type == "update" then
            local u = event.update
            table.insert(out, u.type)
            if u.type == "available_commands_update" then
              assert(u.available_commands[1].name == "review" and u.available_commands[1].input.hint == "what")
            end
            if u.type == "agent_message_chunk" and u.content.type == "text" then text ..= u.content.text end
            if u.type == "agent_thought_chunk" and u.content.type == "text" then assert(u.content.text == "hmm") end
            if u.type == "tool_call" then
              assert(u.tool_call_id == "c1" and u.kind == "edit" and u.status == "pending")
              assert(u.locations[1].line == 3)
              local diff = u.content[1]
              assert(diff.type == "diff" and diff.path == "/w/a" and diff.old_text == "x" and diff.new_text == "y")
              -- the agent's own data keeps its names
              assert(u.raw_input.filePath == "/w/a" and u.raw_input.n == nil)
            end
            if u.type == "tool_call_update" then assert(u.status == "completed" and u.title == nil) end
            if u.type == "plan" then
              local entry = u.entries[1]
              assert(entry.content == "edit a" and entry.priority == "high" and entry.status == "in_progress")
            end
            if u.type == "other" then
              table.insert(out, u.session_update)
              assert(u.update.sessionUpdate == u.session_update)
            end
          elseif event.type == "permission" then
            table.insert(out, "permission " .. event.id)
            assert(event.tool_call.tool_call_id == "c1" and event.tool_call.title == "Edit a")
            assert(#event.options == 2 and event.options[2].kind == "reject_once")
            assert(turn:respond_permission(event.id, { outcome = "selected", option_id = event.options[1].option_id }))
            -- answered once
            assert(failed(turn:respond_permission(event.id, { outcome = "cancelled" })) == "invalid_input")
          else
            table.insert(out, "done " .. event.stop_reason)
          end
        end
        local again, err = turn:next()
        assert(again == nil and err == nil)
        conn:close()
        return table.concat(out, " ") .. " | " .. text
        "#,
    )
    .await;
    assert_eq!(
        got,
        "available_commands_update agent_message_chunk agent_thought_chunk tool_call permission p1 tool_call_update \
         plan other from_the_future other plan agent_message_chunk done end_turn | Hello"
    );

    assert_eq!(said.recv().await.expect("headers"), json!({ "authorization": "Bearer t", "origin": null }));
    let initialize = said.recv().await.expect("initialize");
    let params = &initialize["params"];
    assert_eq!(params["protocolVersion"], 1, "{initialize}");
    assert_eq!(params["clientCapabilities"]["fs"], json!({ "readTextFile": false, "writeTextFile": false }));
    assert_eq!(params["clientCapabilities"]["terminal"], false);
    assert_eq!(params["clientInfo"], json!({ "name": "vos", "version": "2" }));
    let new = said.recv().await.expect("session/new");
    assert_eq!(new["params"]["cwd"], "/w");
    assert_eq!(
        new["params"]["mcpServers"],
        json!([
            { "type": "http", "name": "vos", "url": "http://10.0.0.1:1/mcp",
              "headers": [{ "name": "Authorization", "value": "Bearer k" }] },
            { "name": "local", "command": "run", "args": ["a"], "env": [{ "name": "K", "value": "v" }] },
            { "name": "bare", "command": "go", "args": [], "env": [] },
            { "type": "http", "name": "open", "url": "http://10.0.0.1:2/mcp", "headers": [] },
        ])
    );
    let prompt = said.recv().await.expect("session/prompt");
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(prompt["params"], json!({ "sessionId": "s1", "prompt": [{ "type": "text", "text": "hi" }] }));
    let refusal = said.recv().await.expect("the refusal");
    assert_eq!((&refusal["id"], &refusal["error"]["code"]), (&json!(100), &json!(-32601)), "{refusal}");
    let permission = said.recv().await.expect("the permission's answer");
    assert_eq!(permission["id"], "p1");
    assert_eq!(permission["result"]["outcome"], json!({ "outcome": "selected", "optionId": "yes" }));
    assert_eq!(said.recv().await.expect("the close"), json!({ "after": null }));
}

#[tokio::test]
async fn a_loaded_sessions_history_is_dropped_and_a_cancel_answers_the_waiting_permission_first() {
    let (seen, mut said) = mpsc::unbounded_channel::<Value>();
    let port = agents(move |mut agent| {
        let seen = seen.clone();
        async move {
            agent.initialize().await;
            let load = agent.recv().await.expect("session/load");
            let _ = seen.send(load["params"].clone());
            agent.update("s1", json!({ "sessionUpdate": "user_message_chunk", "content": text("before") })).await;
            agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("before") })).await;
            agent
                .answer(
                    &load,
                    json!({ "modes": { "currentModeId": "ask", "availableModes": [{ "id": "ask", "name": "Ask" }] } }),
                )
                .await;
            let prompt = agent.recv().await.expect("session/prompt");
            agent
                .send(json!({
                    "jsonrpc": "2.0", "id": 7, "method": "session/request_permission",
                    "params": {
                        "sessionId": "s1", "toolCall": { "toolCallId": "c1" },
                        "options": [{ "optionId": "yes", "name": "Allow", "kind": "allow_always" }]
                    }
                }))
                .await;
            // the permission's answer, then the cancel
            for _ in 0..2 {
                let _ = seen.send(agent.recv().await.expect("a message"));
            }
            agent.answer(&prompt, json!({ "stopReason": "cancelled" })).await;
            let _ = agent.recv().await;
        }
    })
    .await;

    let lua = engine(&format!("ws://127.0.0.1:{port}/acp"));
    let got = run(
        &lua,
        r#"
        local conn = assert(acp.connect(URL))
        local loaded = assert(conn:load_session({ session_id = "s1", cwd = "/w" }))
        assert(loaded.modes.current_mode_id == "ask" and loaded.modes.available_modes[1].name == "Ask")
        local turn = assert(conn:prompt({ session_id = "s1", prompt = { { type = "text", text = "go" } } }))
        -- one turn at a time
        assert(failed(conn:prompt({ session_id = "s1", prompt = {} })) == "invalid_input")
        local first = assert(turn:next())
        assert(first.type == "permission" and first.id == "7", first.type)
        assert(conn:cancel("s1"))
        local last = assert(turn:next())
        assert(last.type == "done")
        assert(failed(turn:respond_permission("7", { outcome = "cancelled" })) == "invalid_input")
        return last.stop_reason
        "#,
    )
    .await;
    assert_eq!(got, "cancelled");
    let load = said.recv().await.expect("load");
    assert_eq!(
        (&load["sessionId"], &load["cwd"], &load["mcpServers"]),
        (&json!("s1"), &json!("/w"), &json!([]))
    );
    let permission = said.recv().await.expect("the permission's answer");
    assert_eq!(
        (&permission["id"], &permission["result"]["outcome"]),
        (&json!(7), &json!({ "outcome": "cancelled" }))
    );
    let cancel = said.recv().await.expect("the cancel");
    assert_eq!((&cancel["method"], &cancel["params"]["sessionId"]), (&json!("session/cancel"), &json!("s1")));
}

#[tokio::test]
async fn what_a_turn_the_script_let_go_of_leaves_is_not_the_next_turns() {
    let port = agents(|mut agent| async move {
        agent.initialize().await;
        agent.new_session("s1").await;
        let first = agent.recv().await.expect("the first prompt");
        let second = agent.recv().await.expect("the second prompt");
        agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("first") })).await;
        agent.answer(&first, json!({ "stopReason": "end_turn" })).await;
        agent
            .update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("second") }))
            .await;
        agent.answer(&second, json!({ "stopReason": "max_tokens" })).await;
        let _ = agent.recv().await;
    })
    .await;
    let lua = engine(&format!("ws://127.0.0.1:{port}/acp"));
    run(
        &lua,
        r#"
        conn = assert(acp.connect(URL))
        assert(conn:new_session({ cwd = "/w" }))
        do
          local _ = assert(conn:prompt({ session_id = "s1", prompt = { { type = "text", text = "1" } } }))
        end
        return ""
        "#,
    )
    .await;
    // the first turn is collected unread
    lua.gc_collect().expect("collect");
    lua.gc_collect().expect("collect");
    let got = run(
        &lua,
        r#"
        local turn = assert(conn:prompt({ session_id = "s1", prompt = { { type = "text", text = "2" } } }))
        local out = {}
        while true do
          local event = assert(turn:next())
          if event.type == "done" then table.insert(out, event.stop_reason) break end
          if event.type == "update" and event.update.type == "agent_message_chunk" and event.update.content.type == "text" then
            table.insert(out, event.update.content.text)
          end
        end
        return table.concat(out, " ")
        "#,
    )
    .await;
    assert_eq!(got, "second max_tokens");
}

#[tokio::test]
async fn what_is_not_an_acp_v1_agent_is_an_error_with_a_kind() {
    let forbidden = answers(
        "HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\ncontent-length: 21\r\nconnection: close\r\n\r\n{\"error\":\"forbidden\"}",
    )
    .await;
    let garbage = answers("SSH-2.0-OpenSSH_9.6\r\n\r\n").await;
    let version_two = agents(|mut agent| async move {
        let request = agent.recv().await.expect("initialize");
        agent.answer(&request, json!({ "protocolVersion": 2 })).await;
        let _ = agent.recv().await;
    })
    .await;
    let refuses = agents(|mut agent| async move {
        let request = agent.recv().await.expect("initialize");
        agent.refuse(&request, -32000, "sign in first").await;
        let _ = agent.recv().await;
    })
    .await;
    let silent = agents(|mut agent| async move {
        let _ = agent.recv().await;
        let _ = agent.recv().await;
    })
    .await;
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let lua = engine("");
    let got = run(
        &lua,
        &format!(
            r#"
            local out = {{}}
            local function try(...) table.insert(out, failed(acp.connect(...))) end
            local none, e = acp.connect("ws://127.0.0.1:{forbidden}/acp")
            assert(none == nil and e.kind == "status" and e.code == 403 and e.body == '{{"error":"forbidden"}}')
            assert(tostring(e) == 'vnd:agent_client_protocol.connect: status: the endpoint answered 403: {{"error":"forbidden"}}', tostring(e))
            local none, e = acp.connect("ws://127.0.0.1:{refuses}/acp")
            assert(none == nil and e.kind == "rpc" and e.code == -32000 and e.reason == "sign in first", tostring(e))
            try("ws://127.0.0.1:{garbage}/acp")
            -- the SDK's protocol guard refuses another version
            local _, v2 = acp.connect("ws://127.0.0.1:{version_two}/acp")
            assert(v2 ~= nil and v2.kind == "rpc" and v2.code == -32600 and v2.reason:find("protocol version 1"), tostring(v2))
            try("ws://127.0.0.1:{silent}/acp", {{ timeout = 0.2 }})
            try("ws://127.0.0.1:{closed}/acp")
            -- TLS to an endpoint that does not speak it
            try("wss://127.0.0.1:{silent}/acp")
            try("http://127.0.0.1:{silent}/acp")
            try("not a url")
            try("ws://127.0.0.1:{silent}/acp", {{ timeout = 0 }})
            try("ws://127.0.0.1:{silent}/acp", {{ headers = {{ ["bad name"] = "x" }} }})
            -- what the types say is checked before the call runs: a wrong argument is raised
            local ok = pcall(acp.connect :: any, "ws://127.0.0.1:{silent}/acp", {{ origin = "x" }})
            table.insert(out, if ok then "accepted" else "raised")
            return table.concat(out, " ")
            "#
        ),
    )
    .await;
    assert_eq!(
        got,
        "protocol timed_out connect connect invalid_input invalid_input invalid_input invalid_input raised"
    );
}

#[tokio::test]
async fn an_agent_error_and_a_lost_connection_end_what_waits_on_them() {
    let port = agents(|mut agent| async move {
        agent.initialize().await;
        let new = agent.recv().await.expect("session/new");
        agent.refuse(&new, -32602, "cwd must be absolute").await;
        agent.new_session("s1").await;
        let prompt = agent.recv().await.expect("session/prompt");
        agent.refuse(&prompt, -32603, "model down").await;
        let _ = agent.recv().await.expect("session/prompt");
        agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("a") })).await;
        // the agent goes away mid-turn
        agent.close().await;
    })
    .await;
    let lua = engine(&format!("ws://127.0.0.1:{port}/acp"));
    let got = run(
        &lua,
        r#"
        local conn = assert(acp.connect(URL))
        local none, e = conn:new_session({ cwd = "w" })
        assert(none == nil and e.kind == "rpc" and e.code == -32602 and e.reason == "cwd must be absolute", tostring(e))
        local session = assert(conn:new_session({ cwd = "/w" }))
        local turn = assert(conn:prompt({ session_id = session.session_id, prompt = { { type = "text", text = "a" } } }))
        local none, e = turn:next()
        assert(none == nil and e.kind == "rpc" and e.code == -32603, tostring(e))
        local after, err = turn:next()
        assert(after == nil and err == nil)

        turn = assert(conn:prompt({ session_id = session.session_id, prompt = { { type = "text", text = "b" } } }))
        assert(assert(turn:next()).type == "update")
        local lost = failed(turn:next())
        local after, err = turn:next()
        assert(after == nil and err == nil)
        return lost .. " " .. failed(conn:new_session({ cwd = "/w" }))
        "#,
    )
    .await;
    assert_eq!(got, "closed closed");
}

#[tokio::test]
async fn closing_the_connection_closes_the_socket_and_so_does_shutting_the_engine_down() {
    let (gone, mut closed) = mpsc::unbounded_channel();
    let port = agents(move |mut agent| {
        let gone = gone.clone();
        async move {
            agent.initialize().await;
            // the client closes: there is nothing more to read
            while agent.recv().await.is_some() {}
            let _ = gone.send(());
        }
    })
    .await;
    let lua = engine(&format!("ws://127.0.0.1:{port}/acp"));
    let got = run(
        &lua,
        r#"
        local conn = assert(acp.connect(URL))
        conn:close()
        conn:close()
        local _, e = conn:new_session({ cwd = "/w" })
        local _, c = conn:cancel("s1")
        held = assert(acp.connect(URL))
        return e.kind .. " " .. c.kind
        "#,
    )
    .await;
    assert_eq!(got, "closed closed");
    closed.recv().await.expect("the closed connection's socket");
    neoc_vm_core::lifecycle::Life::of(&lua).shutdown();
    closed.recv().await.expect("the held connection's socket");
}

#[tokio::test]
async fn an_agent_that_drops_the_socket_mid_turn_ends_the_turn() {
    let port = agents(|mut agent| async move {
        agent.initialize().await;
        agent.new_session("s1").await;
        let _ = agent.recv().await.expect("session/prompt");
        // no close frame: the socket is just gone
        drop(agent);
    })
    .await;
    let lua = engine(&format!("ws://127.0.0.1:{port}/acp"));
    let got = run(
        &lua,
        r#"
        local conn = assert(acp.connect(URL))
        assert(conn:new_session({ cwd = "/w" }))
        local turn = assert(conn:prompt({ session_id = "s1", prompt = { { type = "text", text = "a" } } }))
        return failed(turn:next()) .. " " .. failed(conn:new_session({ cwd = "/w" }))
        "#,
    )
    .await;
    assert_eq!(got, "closed closed");
}

// ---------------------------------------------------------------------------------------------------------------
// over the script's lines
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn over_the_scripts_lines_a_turn_streams_asks_for_permission_and_ends_with_the_answer() {
    let (seen, mut said) = mpsc::unbounded_channel::<Value>();
    let lua = lines_engine(move |mut agent| async move {
        let initialize = agent.initialize().await;
        let _ = seen.send(initialize);
        agent.new_session("s1").await;
        let prompt = agent.recv().await.expect("session/prompt");
        let _ = seen.send(prompt.clone());
        agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("Hel") })).await;
        agent
            .update(
                "s1",
                json!({ "sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Edit a", "kind": "edit", "status": "pending" }),
            )
            .await;
        agent
            .send(json!({
                "jsonrpc": "2.0", "id": "p1", "method": "session/request_permission",
                "params": {
                    "sessionId": "s1", "toolCall": { "toolCallId": "c1", "title": "Edit a" },
                    "options": [
                        { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
                        { "optionId": "no", "name": "Reject", "kind": "reject_once" }
                    ]
                }
            }))
            .await;
        let _ = seen.send(agent.recv().await.expect("the permission's answer"));
        agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("lo") })).await;
        agent.answer(&prompt, json!({ "stopReason": "end_turn" })).await;
        // nothing more: the script's transport is still open, as a container's streams would be
        let _ = agent.recv().await;
    });
    let got = run(
        &lua,
        r#"
        local reads, writes = 0, 0
        local conn = assert(acp.connect_lines({
          -- both wait, as reading and writing a container's streams does
          read = function(): string?
            reads += 1
            return agent_read()
          end,
          write = function(line: string)
            writes += 1
            agent_write(line)
          end,
        }, { client_info = { name = "agents", version = "1" }, timeout = 5 }))
        assert(conn.local_address == "", conn.local_address)
        assert(conn.agent.protocol_version == 1 and conn.agent.agent_capabilities.load_session)
        local session = assert(conn:new_session({ cwd = "/w" }))
        local turn = assert(conn:prompt({ session_id = session.session_id, prompt = { { type = "text", text = "hi" } } }))
        local out, said = {}, ""
        while true do
          local event = assert(turn:next())
          if event.type == "done" then
            table.insert(out, "done " .. event.stop_reason)
            break
          elseif event.type == "permission" then
            table.insert(out, "permission " .. event.id)
            assert(turn:respond_permission(event.id, { outcome = "selected", option_id = event.options[1].option_id }))
          else
            table.insert(out, event.update.type)
            if event.update.type == "agent_message_chunk" then said ..= event.update.content.text end
          end
        end
        assert(reads > 0 and writes >= 4, `{reads} reads, {writes} writes`)
        conn:close()
        return table.concat(out, " ") .. " | " .. said
        "#,
    )
    .await;
    assert_eq!(got, "agent_message_chunk tool_call permission p1 agent_message_chunk done end_turn | Hello");
    let initialize = said.recv().await.expect("initialize");
    assert_eq!(initialize["params"]["protocolVersion"], 1);
    assert_eq!(initialize["params"]["clientInfo"], json!({ "name": "agents", "version": "1" }));
    let prompt = said.recv().await.expect("session/prompt");
    assert_eq!(prompt["params"], json!({ "sessionId": "s1", "prompt": [{ "type": "text", "text": "hi" }] }));
    let permission = said.recv().await.expect("the permission's answer");
    assert_eq!(permission["id"], "p1");
    assert_eq!(permission["result"]["outcome"], json!({ "outcome": "selected", "optionId": "yes" }));
}

#[tokio::test]
async fn over_the_scripts_lines_an_agent_that_hangs_up_ends_what_waits_on_it() {
    let lua = lines_engine(|mut agent| async move {
        agent.initialize().await;
        agent.new_session("s1").await;
        let _ = agent.recv().await.expect("session/prompt");
        agent.update("s1", json!({ "sessionUpdate": "agent_message_chunk", "content": text("a") })).await;
        // what the script reads ends
        agent.close().await;
        let _ = agent.recv().await;
    });
    let got = run(
        &lua,
        r#"
        local conn = assert(acp.connect_lines({ read = agent_read, write = agent_write }))
        assert(conn:new_session({ cwd = "/w" }))
        local turn = assert(conn:prompt({ session_id = "s1", prompt = { { type = "text", text = "a" } } }))
        assert(assert(turn:next()).type == "update")
        return failed(turn:next()) .. " " .. failed(conn:new_session({ cwd = "/w" }))
        "#,
    )
    .await;
    assert_eq!(got, "closed closed");
}

#[tokio::test]
async fn over_the_scripts_lines_what_fails_before_initialize_is_an_error_with_a_kind() {
    // the agent answers nothing, and never hangs up
    let lua = lines_engine(|mut agent| async move { while agent.recv().await.is_some() {} });
    let got = run(
        &lua,
        r#"
        local out = {}
        local function try(...) table.insert(out, failed(acp.connect_lines(...))) end
        -- the agent's side is closed from the start
        try({ read = function(): string? return nil end, write = function(_: string) end })
        -- reading fails
        try({ read = function(): string? error("the container is gone") end, write = function(_: string) end })
        -- writing fails
        try({ read = agent_read, write = function(_: string) error("the input is closed") end })
        -- the agent does not answer in time
        try({ read = agent_read, write = agent_write }, { timeout = 0.2 })
        try({ read = agent_read, write = agent_write }, { timeout = 0 })
        -- what the types say is checked before the call runs: a wrong argument is raised
        local function raised(...)
          local ok = pcall(acp.connect_lines :: any, ...)
          table.insert(out, if ok then "accepted" else "raised")
        end
        raised({ read = agent_read })
        raised({ read = agent_read, write = "agent_write" })
        raised({ read = agent_read, write = agent_write, close = agent_write })
        raised("ws://127.0.0.1:1/acp")
        raised({ read = agent_read, write = agent_write }, { headers = {} })
        return table.concat(out, " ")
        "#,
    )
    .await;
    assert_eq!(got, "closed closed closed timed_out invalid_input raised raised raised raised raised");
}
