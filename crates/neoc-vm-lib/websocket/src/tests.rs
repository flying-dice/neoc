//! `lib:websocket` against a tungstenite server in this process: text and binary both ways, the close handshake
//! with its code and reason, a subprotocol, a refused upgrade, and a socket taken over as lines.

use futures_util::{SinkExt as _, StreamExt as _};
use mlua::Lua;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// An echo server: every text or binary message is sent back, `"bye"` is answered with a close (4000, "later"),
/// and the subprotocol `chat` is chosen when offered. A request to `/refused` is answered 403.
async fn echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let callback = |req: &Request, mut resp: Response| {
                    if req.uri().path() == "/refused" {
                        let mut refused = tokio_tungstenite::tungstenite::http::Response::new(Some("go away".to_owned()));
                        *refused.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
                        return Err(refused);
                    }
                    let offered = req.headers().get("sec-websocket-protocol").and_then(|v| v.to_str().ok());
                    if offered.is_some_and(|offered| offered.split(',').any(|p| p.trim() == "chat")) {
                        resp.headers_mut().insert("sec-websocket-protocol", "chat".parse().unwrap());
                    }
                    Ok(resp)
                };
                let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await else { return };
                while let Some(Ok(message)) = ws.next().await {
                    match message {
                        Message::Text(text) if text.as_str() == "bye" => {
                            let frame = CloseFrame { code: CloseCode::from(4000), reason: "later".into() };
                            let _ = ws.send(Message::Close(Some(frame))).await;
                        }
                        Message::Text(_) | Message::Binary(_) => {
                            if ws.send(message).await.is_err() {
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    });
    port
}

async fn run(lua: &Lua, port: u16, script: &str) -> String {
    lua.globals().set("base", format!("ws://127.0.0.1:{port}")).unwrap();
    lua.load(format!("local websocket = require('lib:websocket')\n{script}"))
        .eval_async()
        .await
        .unwrap_or_else(|e| panic!("script failed: {e}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn messages_go_both_ways_and_the_close_says_why() {
    let port = echo().await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local ws = assert(websocket.connect(base .. "/echo", { protocols = { "other", "chat" } }))
        assert(ws.protocol == "chat" and ws.url == base .. "/echo" and ws.local_address == "127.0.0.1")
        assert(ws.close_code == nil)
        assert(ws:send("hello"))
        assert(ws:send_binary("\0\1\2"))
        assert(ws:ping("are you there"))
        local text = assert(ws:receive())
        local binary = assert(ws:receive())
        assert(ws:send("again"))
        local streamed = {}
        local messages = ws:messages()
        table.insert(streamed, assert(messages:next()).data)
        assert(ws:send("bye"))
        local after, err = ws:receive()
        assert(after == nil and err == nil, "the peer's close ends receive")
        assert(ws.close_code == 4000 and ws.close_reason == "later")
        local _, closed = ws:send("too late")
        assert(closed ~= nil and closed.kind == "closed")
        return table.concat({ text.kind, text.data, binary.kind, #binary.data, streamed[1] }, "|")
        "#,
    )
    .await;
    assert_eq!(got, "text|hello|binary|3|again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_is_checked_and_a_refused_upgrade_is_a_status() {
    let port = echo().await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local ws = assert(websocket.connect(base .. "/echo"))
        local _, code = ws:close(1001)
        assert(code ~= nil and code.kind == "invalid_input")
        local _, reason = ws:close(1000, string.rep("x", 124))
        assert(reason ~= nil and reason.kind == "invalid_input")
        assert(ws:close(3001, "done"))
        assert(ws:close(), "closing again does nothing")
        assert(ws:receive() == nil)
        local _, refused = websocket.connect(base .. "/refused")
        assert(refused ~= nil and refused.kind == "status" and refused.code == 403, tostring(refused))
        local _, url = websocket.connect("http://127.0.0.1/")
        assert(url ~= nil and url.kind == "invalid_url")
        local _, header = websocket.connect(base, { headers = { ["Sec-WebSocket-Key"] = "x" } })
        assert(header ~= nil and header.kind == "invalid_input")
        return tostring(ws.close_code)
        "#,
    )
    .await;
    // the echo server answers a close with the same code
    assert_eq!(got, "3001");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_port_is_a_connect_error() {
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local _, err = websocket.connect(base, { connect_timeout = 2 })
        assert(err ~= nil and err.kind == "connect" and err.op == "lib:websocket.connect", tostring(err))
        return "ok"
        "#,
    )
    .await;
    assert_eq!(got, "ok");
}
