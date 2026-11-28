//! `lib:http`'s client against hyper servers in this process: a body streamed both ways, the whole-body reads and
//! their cap, and server-sent events that reconnect with `Last-Event-ID`. Every request goes to a local port.

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use mlua::Lua;
use parking_lot::Mutex;
use tokio::net::TcpListener;

type Reply = Response<http_body_util::combinators::BoxBody<Bytes, Infallible>>;

/// Serve every connection on a local port with `handle`; the port.
async fn serve<F, Fut>(handle: F) -> u16
where
    F: Fn(Request<Incoming>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Reply> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let handle = handle.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let handle = handle.clone();
                    async move { Ok::<_, Infallible>(handle(req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    port
}

fn full(status: u16, content_type: &str, body: &'static str) -> Reply {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Full::new(Bytes::from_static(body.as_bytes())).boxed())
        .unwrap()
}

/// A body sent in the pieces given, each flushed on its own (chunked).
fn chunked(pieces: Vec<&'static str>) -> Reply {
    let frames = futures_util::stream::iter(
        pieces
            .into_iter()
            .map(|piece| Ok::<_, Infallible>(Frame::data(Bytes::from_static(piece.as_bytes())))),
    );
    Response::builder().header("content-type", "text/plain").body(StreamBody::new(frames).boxed()).unwrap()
}

async fn run(lua: &Lua, port: u16, script: &str) -> String {
    lua.globals().set("base", format!("http://127.0.0.1:{port}")).unwrap();
    lua.load(format!("local http = require('lib:http')\n{script}"))
        .eval_async()
        .await
        .unwrap_or_else(|e| panic!("script failed: {e}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_is_read_as_it_arrives_by_chunks_or_by_lines() {
    let port = serve(|req: Request<Incoming>| async move {
        match req.uri().path() {
            "/lines" => chunked(vec!["one\r\ntw", "o\nthr", "ee"]),
            _ => chunked(vec!["a", "b", "c"]),
        }
    })
    .await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local res = assert(http.fetch(base .. "/chunks"))
        assert(res.status == 200 and res.ok and res.redirected == false)
        assert(res.headers["content-type"] == "text/plain" and res:header("Content-Type") == "text/plain")
        local parts = {}
        while true do
            local chunk, err = res:read()
            assert(err == nil, tostring(err))
            if chunk == nil then break end
            table.insert(parts, chunk)
        end
        assert(res:read() == nil, "the end stays the end")
        local lines = {}
        for line in assert(assert(http.fetch(base .. "/lines")):lines()):items() do
            table.insert(lines, line)
        end
        return table.concat(parts) .. "|" .. table.concat(lines, ",")
        "#,
    )
    .await;
    assert_eq!(got, "abc|one,two,three");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_whole_body_is_text_or_json_and_keeps_to_max_body() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let port = serve(move |req: Request<Incoming>| {
        let log = Arc::clone(&log);
        async move {
            let method = req.method().to_string();
            let path = req.uri().path().to_owned();
            let body = req.into_body().collect().await.unwrap().to_bytes();
            log.lock().push(format!("{method} {path} {}", String::from_utf8_lossy(&body)));
            match path.as_str() {
                "/json" => full(200, "application/json", r#"{"items":[1,2,3]}"#),
                "/missing" => full(404, "text/plain", "not here"),
                _ => full(200, "text/plain", "0123456789"),
            }
        }
    })
    .await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local res = assert(http.fetch(base .. "/json", { method = "post", body = "hello" }))
        local doc = assert(res:json())
        local missing = assert(http.fetch(base .. "/missing"))
        assert(missing.status == 404 and not missing.ok and missing:text() == "not here")
        local _, big = assert(http.fetch(base .. "/text", { max_body = 4 })):text()
        assert(big ~= nil and big.kind == "too_large" and big.max_body == 4, tostring(big))
        local _, notjson = assert(http.fetch(base .. "/text")):json()
        assert(notjson ~= nil and notjson.kind == "bad_response")
        return tostring(assert(doc:get("items")):len())
        "#,
    )
    .await;
    assert_eq!(got, "3");
    assert_eq!(seen.lock()[0], "POST /json hello");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_written_as_it_goes_is_sent_chunked() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let port = serve(move |req: Request<Incoming>| {
        let log = Arc::clone(&log);
        async move {
            let chunked = req.headers().get("transfer-encoding").map(|v| v.to_str().unwrap().to_owned());
            let method = req.method().to_string();
            let body = req.into_body().collect().await.unwrap().to_bytes();
            log.lock().push(format!("{method} {chunked:?} {}", String::from_utf8_lossy(&body)));
            full(201, "text/plain", "stored")
        }
    })
    .await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local upload = assert(http.request(base .. "/upload", { method = "PUT" }))
        assert(upload:write("part one, "))
        assert(upload:write(""))
        assert(upload:write("part two"))
        local res = assert(upload:finish())
        local _, again = upload:write("late")
        assert(again ~= nil and again.kind == "invalid_input")
        local _, twice = upload:finish()
        assert(twice ~= nil and twice.kind == "invalid_input")
        local _, length = http.request(base .. "/upload", { headers = { ["content-length"] = "3" } })
        assert(length ~= nil and length.kind == "invalid_input")
        return res.status .. " " .. res:text()
        "#,
    )
    .await;
    assert_eq!(got, "201 stored");
    assert_eq!(seen.lock().as_slice(), [r#"PUT Some("chunked") part one, part two"#]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_reconnect_with_the_last_event_id_and_a_refusal_is_an_error() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let port = serve(move |req: Request<Incoming>| {
        let log = Arc::clone(&log);
        async move {
            let last = req.headers().get("last-event-id").map(|v| v.to_str().unwrap().to_owned());
            let accept = req.headers().get("accept").map(|v| v.to_str().unwrap().to_owned());
            log.lock().push(format!("{} {last:?} {accept:?}", req.uri().path()));
            match (req.uri().path(), last.as_deref()) {
                ("/refused", _) => full(500, "text/plain", "no"),
                ("/plain", _) => full(200, "text/plain", "data: x\n\n"),
                (_, None) => {
                    full(200, "text/event-stream", "retry: 50\nid: 1\nevent: greet\ndata: a\ndata: b\n\n")
                }
                (_, Some(_)) => full(200, "text/event-stream", ": a comment\ndata: c\n\n"),
            }
        }
    })
    .await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local events = assert(http.events(base .. "/stream", { retry = 0.05 }))
        local first = assert(events:next())
        local second = assert(events:next())
        events:close()
        assert(events:next() == nil, "a closed stream ends")
        local _, refused = http.events(base .. "/refused")
        assert(refused ~= nil and refused.kind == "status" and refused.code == 500, tostring(refused))
        local _, plain = http.events(base .. "/plain")
        assert(plain ~= nil and plain.kind == "bad_response", tostring(plain))
        local once = assert(http.events(base .. "/stream", { reconnect = false }))
        local only = assert(once:next())
        assert(once:next() == nil, "without reconnect the stream ends with its connection")
        return table.concat({
            first.event, first.data, first.id, second.event, second.data, second.id, only.data,
        }, "|")
        "#,
    )
    .await;
    assert_eq!(got, "greet|a\nb|1|message|c|1|a\nb");
    let seen = seen.lock();
    assert_eq!(seen[0], r#"/stream None Some("text/event-stream")"#);
    assert_eq!(seen[1], r#"/stream Some("1") Some("text/event-stream")"#);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failure_is_a_typed_error() {
    let lua = neoc_vm_core::make_engine().unwrap();
    // a port nothing listens on; the default connect_timeout (10 s), since Windows retries a refused connect for
    // about 2 s before it gives up
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let got = run(
        &lua,
        closed,
        r#"
        local _, refused = http.fetch(base .. "/")
        assert(refused ~= nil and refused.kind == "connect" and refused.op == "lib:http.fetch", tostring(refused))
        local _, url = http.fetch("ftp://example.com/")
        assert(url ~= nil and url.kind == "invalid_url")
        local _, method = http.fetch(base .. "/", { method = "GE T" })
        assert(method ~= nil and method.kind == "invalid_input")
        local upload = assert(http.request(base .. "/"))
        local wrote, lost = upload:write("x")
        if not wrote then assert(lost ~= nil and lost.kind == "connection_lost") end
        local _, why = upload:finish()
        assert(why ~= nil and why.kind == "connect", tostring(why))
        local ok = pcall(http.fetch :: any, base, { methd = "GET" })
        assert(not ok, "a misspelt option is refused")
        return "ok"
        "#,
    )
    .await;
    assert_eq!(got, "ok");
}

/// Says so when it is dropped: a body that is dropped once its connection closes.
struct Dropped(tokio::sync::mpsc::UnboundedSender<&'static str>);

impl Drop for Dropped {
    fn drop(&mut self) {
        let _ = self.0.send("closed");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_ends_a_read_text_or_lines_that_waits_and_closes_the_connection() {
    let (closed_tx, mut closed) = tokio::sync::mpsc::unbounded_channel();
    // headers, then a body that never comes; the stream (and the guard in it) goes when the connection does
    let port = serve(move |_req: Request<Incoming>| {
        let guard = Dropped(closed_tx.clone());
        async move {
            let frames = futures_util::stream::pending::<Result<Frame<Bytes>, Infallible>>();
            let frames = futures_util::StreamExt::map(frames, move |frame| {
                let _ = &guard;
                frame
            });
            Response::builder()
                .header("content-type", "text/plain")
                .body(StreamBody::new(frames).boxed())
                .unwrap()
        }
    })
    .await;
    let lua = neoc_vm_core::make_engine().unwrap();
    lua.globals().set("base", format!("http://127.0.0.1:{port}")).unwrap();
    let mut got = Vec::new();
    for read in ["RES:read()", "RES:text()", "RES:json()", "LINES:next()"] {
        let setup = if read.starts_with("LINES") {
            "local http = require('lib:http') RES = assert(http.fetch(base .. '/')) LINES = assert(RES:lines())"
        } else {
            "local http = require('lib:http') RES = assert(http.fetch(base .. '/'))"
        };
        lua.load(setup).exec_async().await.unwrap();
        let canceller = lua.clone();
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            canceller.load("RES:cancel()").exec_async().await.unwrap();
        });
        let ended: String = lua
            .load(format!("local _, err = {read} return if err then err.kind else 'no error'"))
            .eval_async()
            .await
            .unwrap();
        cancel.await.unwrap();
        // a read after it gives the end, and does not wait
        let after: String = lua
            .load("local chunk, err = RES:read() return tostring(chunk) .. ' ' .. tostring(err)")
            .eval_async()
            .await
            .unwrap();
        got.push(format!("{read} {ended} / {after}"));
        lua.load("RES = nil LINES = nil").exec().unwrap();
    }
    assert_eq!(
        got,
        [
            "RES:read() cancelled / nil nil",
            "RES:text() cancelled / nil nil",
            "RES:json() cancelled / nil nil",
            "LINES:next() cancelled / nil nil",
        ]
    );
    // every connection the server saw was closed by the cancel
    for _ in 0..4 {
        let seen = tokio::time::timeout(std::time::Duration::from_secs(5), closed.recv())
            .await
            .expect("a connection closed");
        assert_eq!(seen, Some("closed"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_id_resets_the_last_event_id_and_a_retry_alone_takes_effect() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let port = serve(move |req: Request<Incoming>| {
        let log = Arc::clone(&log);
        async move {
            let last = req.headers().get("last-event-id").map(|v| v.to_str().unwrap().to_owned());
            let n = {
                let mut log = log.lock();
                log.push(format!("{last:?}"));
                log.len()
            };
            match n {
                // an id, and an event that carries it
                1 => full(200, "text/event-stream", "id: 1\ndata: a\n\n"),
                // a retry with no event, then an empty id that resets the last one, then an event
                2 => full(200, "text/event-stream", "retry: 20\n\nid\ndata: b\n\n"),
                // an id with no event after it still counts for the next reconnection
                3 => full(200, "text/event-stream", "data: c\n\nid: 9\n\n"),
                _ => full(200, "text/event-stream", "data: d\n\n"),
            }
        }
    })
    .await;
    let lua = neoc_vm_core::make_engine().unwrap();
    let got = run(
        &lua,
        port,
        r#"
        local events = assert(http.events(base .. "/stream", { retry = 0.05 }))
        local out = {}
        for _ = 1, 4 do
          local event = assert(events:next())
          table.insert(out, event.data .. "=" .. event.id)
        end
        events:close()
        return table.concat(out, " ")
        "#,
    )
    .await;
    assert_eq!(got, "a=1 b= c= d=9");
    assert_eq!(seen.lock()[..4], [r#"None"#, r#"Some("1")"#, "None", r#"Some("9")"#]);
}
