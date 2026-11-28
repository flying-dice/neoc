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
                // the refusal's type is tungstenite's, whatever its size
                #[allow(clippy::result_large_err)]
                let callback = |req: &Request, mut resp: Response| {
                    if req.uri().path() == "/refused" {
                        let mut refused =
                            tokio_tungstenite::tungstenite::http::Response::new(Some("go away".to_owned()));
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
                    if matches!(&message, Message::Text(text) if text.as_str() == "bye") {
                        let frame = CloseFrame { code: CloseCode::from(4000), reason: "later".into() };
                        let _ = ws.send(Message::Close(Some(frame))).await;
                    } else if matches!(message, Message::Text(_) | Message::Binary(_))
                        && ws.send(message).await.is_err()
                    {
                        return;
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
        -- the default connect_timeout (10 s): Windows retries a refused connect for about 2 s before it gives up
        local _, err = websocket.connect(base)
        assert(err ~= nil and err.kind == "connect" and err.op == "lib:websocket.connect", tostring(err))
        return "ok"
        "#,
    )
    .await;
    assert_eq!(got, "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peers_close_is_answered_and_the_connection_released_while_the_script_holds_the_socket() {
    use tokio::io::AsyncReadExt as _;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let frame = CloseFrame { code: CloseCode::from(4001), reason: "going".into() };
        ws.send(Message::Close(Some(frame))).await.unwrap();
        // the client's answer to the close, then the end of its TCP connection: the server never closes its own
        let reply = match tokio::time::timeout(std::time::Duration::from_secs(10), ws.next()).await {
            Ok(Some(Ok(Message::Close(Some(frame))))) => format!("reply {}", u16::from(frame.code)),
            other => format!("no reply: {other:?}"),
        };
        let _ = seen_tx.send(reply);
        let mut rest = [0_u8; 16];
        let ended =
            tokio::time::timeout(std::time::Duration::from_secs(10), ws.get_mut().read(&mut rest)).await;
        let _ = seen_tx.send(match ended {
            Ok(Ok(0)) => "eof".to_owned(),
            other => format!("not ended: {other:?}"),
        });
    });
    let lua = neoc_vm_core::make_engine().unwrap();
    let seen_rx = std::sync::Arc::new(tokio::sync::Mutex::new(seen_rx));
    let server_saw =
        neoc_vm_core::lifecycle::async_function(&lua, neoc_vm_core::lifecycle::Role::Wait, move |_, ()| {
            let seen_rx = seen_rx.clone();
            async move { Ok(seen_rx.lock().await.recv().await) }
        })
        .unwrap();
    lua.globals().set("server_saw", server_saw).unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local ws = assert(websocket.connect(base .. "/"))
        assert(ws:receive() == nil, "the peer's close ends receive")
        -- `ws` is still held here, and nothing more is read or written on it
        local reply, ended = server_saw(), server_saw()
        assert(ws.close_code == 4001 and ws.close_reason == "going")
        return reply .. " " .. ended
        "#,
    )
    .await;
    // the reply echoes the peer's code, as tungstenite answers a close
    assert_eq!(got, "reply 4001 eof");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_that_ignores_this_ends_close_is_cut_off_and_receive_ends() {
    use tokio::io::AsyncReadExt as _;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (ended_tx, ended_rx) = tokio::sync::oneshot::channel::<String>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        // the raw connection from here on: the client's close frame is read and never answered, until its end
        let tcp = ws.get_mut();
        let mut buf = [0_u8; 256];
        let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                match tcp.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
            }
        })
        .await;
        let _ = ended_tx.send(if ended.is_ok() { "released".to_owned() } else { "still open".to_owned() });
    });
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local ws = assert(websocket.connect(base .. "/"))
        assert(ws:close(1000, "bye"))
        -- the peer never answers: receive ends once the close wait is over, with `ws` still held
        local message, err = ws:receive()
        assert(message == nil and err == nil, tostring(err))
        local _, late = ws:send("x")
        return tostring(ws.close_code) .. " " .. late.kind
        "#,
    )
    .await;
    assert_eq!(got, "1006 closed");
    let ended =
        tokio::time::timeout(std::time::Duration::from_secs(5), ended_rx).await.expect("the server ended");
    assert_eq!(ended.unwrap(), "released");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_behind_a_send_to_a_peer_that_stopped_reading_is_cut_off_in_time() {
    use std::time::Duration;
    use tokio::io::AsyncReadExt as _;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let (ended_tx, ended_rx) = tokio::sync::oneshot::channel::<String>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        // the peer stops reading: the client's send fills the TCP buffers and stalls, and its close cannot go
        let _ = done_rx.await;
        let tcp = ws.get_mut();
        let mut buf = vec![0_u8; 1 << 16];
        let ended = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match tcp.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
            }
        })
        .await;
        let _ = ended_tx.send(if ended.is_ok() { "released".to_owned() } else { "still open".to_owned() });
    });
    let lua = neoc_vm_core::make_engine().unwrap();
    run(&lua, port, "WS = assert(websocket.connect(base .. '/')) return ''").await;
    let sender = lua.clone();
    let sending = tokio::spawn(async move {
        sender
            // sends until one stalls on the full buffers (however large the platform's are), and then fails
            .load(
                "local chunk = string.rep('x', 1024 * 1024)
                while true do
                    local ok, err = WS:send(chunk)
                    if not ok then return tostring(ok) .. ' ' .. err.kind end
                end",
            )
            .eval_async::<String>()
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!sending.is_finished(), "the send stalls on a peer that stopped reading");
    // the caller stops waiting for its close (stuck behind the send): the deadline runs all the same
    let first = lua.load("WS:close(1000, 'bye')").exec_async();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), first).await.is_err(),
        "the close waits behind the send"
    );
    let got = tokio::time::timeout(
        Duration::from_secs(5),
        lua.load(
            r#"
            local message, err = WS:receive()
            assert(message == nil and err == nil, tostring(err))
            return tostring(WS.close_code) .. " " .. tostring(WS:close(1000, "again"))
            "#,
        )
        .eval_async::<String>(),
    )
    .await
    .expect("receive ends once the close wait is over")
    .unwrap();
    assert_eq!(got, "1006 true");
    let sent = tokio::time::timeout(Duration::from_secs(5), sending)
        .await
        .expect("the stalled send gives up")
        .unwrap()
        .unwrap();
    assert_eq!(sent, "nil closed");
    let _ = done_tx.send(());
    let ended = tokio::time::timeout(Duration::from_secs(15), ended_rx).await.expect("the server ended");
    assert_eq!(ended.unwrap(), "released");
}
