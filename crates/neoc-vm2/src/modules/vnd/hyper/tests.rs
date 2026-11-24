//! `vnd:hyper` as a script uses it: a server in the engine, and the client against it. The client's own behaviour
//! (timeouts, redirects, the body cap, TLS) is tested against hand-written servers in `client/tests.rs`.
//!
//! Ported from neoc-vm's `vnd/hyper.rs` test and from `crates/neoc/tests/vnd/hyper.test.lua`.

use mlua::Lua;

use crate::lifecycle::Life;
use crate::{make_engine, make_engine_in};

const PRELUDE: &str = r#"
local net = require("std:net")
local hyper = require("vnd:hyper")
local function check(cond, message) if not cond then error(message or "check failed", 2) end end
-- the call must have failed with this kind: `fails("connect", hyper.get(url))`
local function fails(kind, value, err)
  check(value == nil, "expected a failure, got a value")
  check(type(err) == "table", "expected a typed error, got " .. tostring(err))
  check(err.kind == kind, "expected " .. kind .. ", got " .. tostring(err))
  return err
end
-- the call must have been refused as a wrong argument that says `what`
local function refused(what, f, ...)
  local ok, err = pcall(f, ...)
  check(not ok, "expected the call to be refused: " .. what)
  check(tostring(err):find(what, 1, true), what .. ": " .. tostring(err))
end
local function ok(value, err) check(err == nil, tostring(err)) return value end
-- a server on a port of its own, and its base URL
local function serve(handler)
  local server = ok(hyper.serve(ok(net.TcpListener.bind("127.0.0.1:0")), handler))
  return server, "http://" .. ok(server:addr())
end
"#;

async fn run_in(lua: &Lua, body: &str) {
    if let Err(e) = lua.load(format!("{PRELUDE}\n{body}")).exec_async().await {
        panic!("{e}");
    }
}

async fn run(body: &str) {
    run_in(&make_engine().expect("engine"), body).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_handler_that_reaches_a_panicking_host_function_answers_500_and_the_server_serves_on() {
    let lua = make_engine().expect("engine");
    let explode =
        lua.create_function(|_, ()| -> mlua::Result<()> { panic!("the module broke") }).expect("function");
    lua.globals().set("explode", explode).expect("global");
    let (status, body, next): (u16, String, String) = lua
        .load(
            r#"
            local net = require("std:net")
            local hyper = require("vnd:hyper")
            local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
            local server = assert(hyper.serve(listener, function(req)
                if req.path == "/explode" then explode() end
                return "fine"
            end))
            local base = "http://" .. assert(server:addr())
            -- before the fix the panic ended the connection's task, so this request got no response at all
            local failed = assert(hyper.get(base .. "/explode"))
            local served = assert(hyper.get(base .. "/next"))
            server:close()
            return failed.status, failed.body, served.body
        "#,
        )
        .eval_async()
        .await
        .expect("script");
    assert_eq!((status, body.as_str(), next.as_str()), (500, "handler error", "fine"));
}

/// The service of `hyper.test.lua`: one handler that dispatches on the method and the path.
const SERVICE: &str = r#"
local handler_calls = 0
local base
local server
server, base = serve(function(req)
    if req.method == "GET" and req.path == "/hello" then
        return { status = 200, headers = { ["content-type"] = "text/plain" }, body = "hello" }
    end
    if req.method == "GET" and req.path == "/headers" then
        return { status = 200, body = req:header("x-echo") or "" }
    end
    if req.path == "/length" then
        return { status = 200, body = req:header("content-length") or "none" }
    end
    if req.path == "/host" then
        return { status = 200, body = req:header("host") or "none" }
    end
    if req.path == "/method" then
        return { status = 200, body = req.method .. ":" .. (req:header("content-length") or "none") }
    end
    if req.path == "/moved" then
        return { status = 302, headers = { location = "/hello" }, body = "moved" }
    end
    if req.path == "/loop" then
        return { status = 307, headers = { location = "/loop" }, body = "again" }
    end
    if req.path == "/to-get" then
        return { status = 303, headers = { location = "/method" }, body = "see other" }
    end
    if req.path == "/big" then
        return { status = 200, body = string.rep("x", 2000) }
    end
    if req.path == "/slow" then
        require("std:task").wait(0.6)
        return { status = 200, body = "late" }
    end
    if req.method == "POST" and req.path == "/echo" then
        return { status = 200, body = req.body }
    end
    if req.method == "PUT" and req.path == "/items/1" then
        return { status = 204 }
    end
    if req.method == "GET" and req.path == "/handler/greet" then
        handler_calls += 1
        return { status = 200, headers = { ["x-handled-by"] = "lua" }, body = "hi from lua handler, " .. req.method }
    end
    if req.method == "POST" and req.path == "/handler/echo" then
        handler_calls += 1
        return { status = 200, body = req.body }
    end
    if req.method == "GET" and req.path == "/handler/proxy" then
        handler_calls += 1
        -- the handler re-enters the server through the async client: async calls work inside a handler, and the
        -- handler captures `base` from the enclosing scope
        local inner, err = hyper.get(base .. "/hello")
        if inner == nil then
            return { status = 502, body = "proxy err: " .. tostring(err) }
        end
        return { status = inner.status, body = "proxied:" .. inner.body }
    end
    return { status = 404, body = "not found" }
end)
"#;

async fn with_service(body: &str) {
    run(&format!("{SERVICE}\n{body}\nserver:close()")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_returns_the_status_the_headers_the_body_and_the_url() {
    with_service(
        r#"
        check(ok(server:addr()):find("^127%.0%.0%.1:%d+$"))
        local resp = ok(hyper.get(base .. "/hello"))
        check(resp.status == 200 and resp.body == "hello" and resp.url == base .. "/hello")
        check(resp.headers["content-type"] == "text/plain")
        -- a response is a plain table: these four fields and nothing else
        local keys = {}
        for key in resp do table.insert(keys, key) end
        table.sort(keys)
        check(table.concat(keys, ",") == "body,headers,status,url")

        check(ok(hyper.get(base .. "/headers", { headers = { ["x-echo"] = "from get" } })).body == "from get")
        -- a GET sends no content-length
        check(ok(hyper.get(base .. "/length")).body == "none")
        check(ok(hyper.get(base .. "/anything-unhandled")).status == 404)

        local err = fails("invalid_url", hyper.get("not a url"))
        check(err.op == "vnd:hyper.get")
        err = fails("invalid_url", hyper.get("ftp://example.com/"))
        check(err.message:find("only http:// and https://", 1, true))
        check(tostring(err) == "vnd:hyper.get: invalid_url: " .. err.message)
        -- https itself, the certificate checks and ca_pem are tested against a local TLS server
        err = fails("invalid_input", hyper.get("https://localhost:1/", { ca_pem = "nonsense" }))
        check(err.reason:find("ca_pem", 1, true))
        fails("connect", hyper.get("http://127.0.0.1:1/"))

        refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base .. "/headers", { headers = "x-echo: ping" })
        -- a misspelt option is refused by name, never dropped
        refused("unknown field `header`", hyper.get, base .. "/headers", { header = { ["x-echo"] = "typo" } })
        "#,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_sends_its_body_and_says_its_length() {
    with_service(
        r#"
        local resp = ok(hyper.post(base .. "/echo", "ping pong"))
        check(resp.status == 200 and resp.body == "ping pong")
        check(ok(hyper.post(base .. "/echo", "")).body == "")
        -- any bytes, there and back
        local bytes = "\x41\x01\xFF\x00"
        check(ok(hyper.post(base .. "/echo", bytes)).body == bytes)
        check(ok(hyper.post(base .. "/echo", buffer.tostring(buffer.fromstring(bytes)))).body == bytes)

        -- no body or an empty one is sent with content-length 0
        check(ok(hyper.post(base .. "/length", "")).body == "0")
        check(ok(hyper.post(base .. "/length")).body == "0")
        check(ok(hyper.post(base .. "/length", nil, { timeout = 30 })).body == "0")
        check(ok(hyper.post(base .. "/length", "abcd")).body == "4")

        -- the URL and the body are arguments: a table in their place, a buffer or a number is refused
        refused("vnd:hyper.post: argument #1 'url'", hyper.post, {})
        refused("vnd:hyper.post: argument #2 'body'", hyper.post, base .. "/echo", { "ping" })
        refused("vnd:hyper.post: argument #2 'body'", hyper.post, base .. "/echo", true)
        refused("vnd:hyper.post: argument #2 'body'", hyper.post, base .. "/echo", 42)
        refused("vnd:hyper.post: argument #2 'body'", hyper.post, base .. "/echo", buffer.create(4))
        refused("unknown field `timeuot`", hyper.post, base .. "/echo", "x", { timeuot = 5 })
        "#,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_sends_the_method_asked_for() {
    with_service(
        r#"
        local resp = ok(hyper.request("GET", base .. "/headers", nil, { headers = { ["x-echo"] = "ping" } }))
        check(resp.status == 200 and resp.body == "ping")
        check(ok(hyper.request("PUT", base .. "/items/1")).status == 204)
        -- a lower-case standard method is the standard method
        check(ok(hyper.request("put", base .. "/method")).body == "PUT:0")
        -- a PUT with no body says content-length 0, a DELETE with none sends no length
        check(ok(hyper.request("PUT", base .. "/length")).body == "0")
        check(ok(hyper.request("DELETE", base .. "/length")).body == "none")
        check(ok(hyper.request("POST", base .. "/echo", "put")).body == "put")

        local err = fails("invalid_url", hyper.request("GET", "ftp://example.com/"))
        check(err.op == "vnd:hyper.request" and err.message:find("only http:// and https://", 1, true))
        fails("invalid_input", hyper.request("GE T", base .. "/hello"))

        refused("vnd:hyper.request: argument #1 'method'", hyper.request)
        refused("vnd:hyper.request: argument #2 'url'", hyper.request, "GET")
        refused("vnd:hyper.request: argument #1 'method'", hyper.request, { method = "GET", url = base })
        refused("vnd:hyper.request: argument #4 'opts'", hyper.request, "GET", base, nil, { headers = "x-echo: ping" })
        refused("unknown field `hedaers`", hyper.request, "GET", base .. "/headers", nil, { hedaers = {} })
        refused("vnd:hyper.request: argument #4 'opts'", hyper.request, "GET", "https://localhost:1/", nil, { ca_pem = {} })
        "#,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redirects_are_followed_to_the_limit() {
    with_service(
        r#"
        local resp = ok(hyper.get(base .. "/moved"))
        check(resp.status == 200 and resp.body == "hello" and resp.url == base .. "/hello")

        resp = ok(hyper.get(base .. "/moved", { follow_redirects = false }))
        check(resp.status == 302 and resp.headers.location == "/hello" and resp.url == base .. "/moved")

        local err = fails("too_many_redirects", hyper.get(base .. "/loop", { max_redirects = 3 }))
        check(err.max_redirects == 3 and err.message == "more than 3 redirects")

        -- a 303 becomes a GET without a body
        check(ok(hyper.request("POST", base .. "/to-get", "payload")).body == "GET:none")

        fails("invalid_input", hyper.get(base .. "/moved", { max_redirects = 0 }))
        for _, bad in { -1, 1.5, "yes" } do
            refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base .. "/moved", { max_redirects = bad })
        end
        for _, bad in { 0, 3, "yes" } do
            refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base .. "/moved", { follow_redirects = bad })
        end
        "#,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_limits_are_kept_to() {
    with_service(
        r#"
        local err = fails("too_large", hyper.get(base .. "/big", { max_body = 1000 }))
        check(err.max_body == 1000)
        check(#ok(hyper.get(base .. "/big", { max_body = 2000 })).body == 2000)

        -- a server that stays silent ends at idle_timeout
        err = fails("timed_out", hyper.get(base .. "/slow", { idle_timeout = 0.15 }))
        check(err.limit == "idle_timeout")
        -- timeout bounds the whole request
        err = fails("timed_out", hyper.get(base .. "/slow", { timeout = 0.15 }))
        check(err.limit == "timeout" and err.message:find("the request took longer than", 1, true))

        -- a limit that is not positive and finite is an error; one that is not a number is not an option's value
        for _, name in { "timeout", "idle_timeout", "connect_timeout" } do
            for _, bad in { 0, -1, math.huge } do
                err = fails("invalid_input", hyper.get(base .. "/hello", { [name] = bad }))
                check(err.reason:find(name .. " must be", 1, true), err.reason)
            end
            refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base .. "/hello", { [name] = "5" })
        end
        err = fails("invalid_input", hyper.get(base .. "/hello", { max_body = 0 }))
        check(err.reason:find("max_body must be", 1, true))
        for _, bad in { -1, "5", math.huge } do
            refused("vnd:hyper.get: argument #2 'opts'", hyper.get, base .. "/hello", { max_body = bad })
        end
        "#,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_sent_is_the_request_asked_for() {
    with_service(
        r#"
        local err = fails("invalid_input", hyper.post(base .. "/echo", "abc", { headers = { ["Content-Length"] = "1" } }))
        check(err.reason:find("Content-Length", 1, true))
        -- a caller-set Host replaces the built-in one
        check(ok(hyper.get(base .. "/host", { headers = { Host = "virtual.example" } })).body == "virtual.example")
        "#,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handler_answers_and_may_call_the_client_itself() {
    with_service(
        r#"
        local resp = ok(hyper.get(base .. "/handler/greet"))
        check(resp.status == 200 and resp.body == "hi from lua handler, GET")
        check(resp.headers["x-handled-by"] == "lua")
        check(ok(hyper.post(base .. "/handler/echo", "from-test")).body == "from-test")

        -- the outer request lands on /handler/proxy; the inner /hello is dispatched by the same handler
        local before = handler_calls
        resp = ok(hyper.get(base .. "/handler/proxy"))
        check(resp.status == 200 and resp.body == "proxied:hello")
        check(handler_calls - before == 1)
        "#,
    )
    .await;
}

/// What a handler may return: a string, a `Reply`, or nothing. Anything else is a 500 and the server serves on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handler_returns_a_body_a_reply_or_nothing() {
    run(r#"
        local server, base = serve(function(req)
            local path = req.path
            if path == "/text" then return "plain" end
            if path == "/bytes" then return "\xff\x00\xfe" end
            if path == "/nothing" then return end
            if path == "/empty" then return {} end
            if path == "/reply-bytes" then return { status = 201, body = "\xff\x00\xfe" } end
            if path == "/number" then return 42 :: any end
            if path == "/boolean" then return true :: any end
            if path == "/typo" then return { status = 200, bodyy = "x" } :: any end
            if path == "/bad-status" then return { status = "200" } :: any end
            if path == "/bad-header" then return { headers = { ["x-n"] = 5 } } :: any end
            if path == "/raise" then error("no") end
            return { status = 404 }
        end)
        local function got(path)
            local resp = ok(hyper.get(base .. path))
            return resp.status .. ":" .. resp.body
        end
        check(got("/text") == "200:plain")
        check(got("/bytes") == "200:\xff\x00\xfe")
        check(got("/nothing") == "200:")
        check(got("/empty") == "200:")
        check(got("/reply-bytes") == "201:\xff\x00\xfe")
        for _, path in { "/number", "/boolean", "/typo", "/bad-status", "/bad-header", "/raise" } do
            check(got(path) == "500:handler error", path .. " " .. got(path))
        end
        check(got("/after") == "404:")
        server:close()
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_says_its_method_path_headers_and_body() {
    run(r#"
        local seen
        local server, base = serve(function(req)
            local json = req.json
            seen = {
                method = req.method,
                path = req.path,
                body = req.body,
                is_json = req.is_json,
                headers = req.headers,
                header = req:header("X-Echo"),
                missing = req:header("x-missing"),
                json_type = type(json),
                name = if type(json) == "userdata" then (json :: any):get("name") else json,
            }
            return nil
        end)
        ok(hyper.post(base .. "/items/1?verbose=1", '{"name":"ada"}', {
            headers = { ["Content-Type"] = "application/json; charset=utf-8", ["x-echo"] = "ping" },
        }))
        check(seen.method == "POST" and seen.path == "/items/1" and seen.body == '{"name":"ada"}', "the request line")
        check(seen.is_json == true and seen.header == "ping" and seen.missing == nil, "is_json and header")
        -- names in lower case, one value each
        check(seen.headers["content-type"] == "application/json; charset=utf-8", "content-type")
        check(seen.headers["x-echo"] == "ping" and seen.headers["content-length"] == "14", "x-echo and length")
        check(seen.headers["Content-Type"] == nil and seen.headers.host ~= nil, "lower case names")
        check(seen.json_type == "userdata" and seen.name == "ada", "json: " .. seen.json_type)

        -- a scalar document is a Lua value; an empty body, text that is not JSON and `null` are nil
        ok(hyper.post(base .. "/", "42"))
        check(seen.is_json == false and seen.json_type == "number" and seen.name == 42, "a scalar: " .. seen.json_type)
        for _, body in { "", "{not json", "null" } do
            ok(hyper.post(base .. "/", body))
            check(seen.json_type == "nil", body)
        end
        -- a body that is not UTF-8 reaches the handler as its bytes
        ok(hyper.post(base .. "/", "\xff\x00"))
        check(seen.body == "\xff\x00" and seen.json_type == "nil", "bytes")
        server:close()
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_ends_the_server_and_wait_returns_once_it_has() {
    run(r#"
        local server, base = serve(function() return "x" end)
        check(ok(hyper.get(base .. "/x")).status == 200)
        server:close()
        server:wait()
        -- a second wait after the server is done returns too, and closing twice is nothing
        server:wait()
        server:close()
        local addr, err = server:addr()
        check(addr == nil and err.kind == "closed" and err.op == "vnd:hyper.Server:addr")
        check(tostring(err) == "vnd:hyper.Server:addr: closed: the server is closed")
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_takes_a_listener_and_a_function() {
    run(r#"
        local listener = ok(net.TcpListener.bind("127.0.0.1:0"))
        refused("vnd:hyper.serve: argument #1 'listener'", hyper.serve, "127.0.0.1:0", function() return "x" end)
        refused("vnd:hyper.serve: argument #1 'listener'", hyper.serve)
        refused("vnd:hyper.serve: argument #2 'handler'", hyper.serve, listener)
        refused("vnd:hyper.serve: argument #2 'handler'", hyper.serve, listener, "handler")
        -- two servers on one listener race on accept: every request is answered by one of them
        local a = ok(hyper.serve(listener, function() return "a" end))
        local b = ok(hyper.serve(listener, function() return "b" end))
        check(ok(a:addr()) == ok(b:addr()))
        for _ = 1, 4 do
            local body = ok(hyper.get("http://" .. ok(a:addr()) .. "/")).body
            check(body == "a" or body == "b")
        end
        a:close()
        b:close()
    "#)
    .await;
}

/// The engine's shutdown closes the server at once: it says it is closed, a script waiting on it goes on, and a
/// listener the engine has closed serves nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutting_the_engine_down_closes_its_servers() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let addr: String = lua
        .load(format!(
            "{PRELUDE}\nlistener = ok(net.TcpListener.bind(\"127.0.0.1:0\"))\n\
             server = ok(hyper.serve(listener, function() return \"x\" end))\nreturn ok(server:addr())"
        ))
        .eval_async()
        .await
        .expect("script");
    let waiting = tokio::spawn(lua.load("server:wait() return true").eval_async::<bool>());
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!waiting.is_finished(), "nothing closed the server yet");

    life.shutdown();
    let woken =
        tokio::time::timeout(std::time::Duration::from_secs(10), waiting).await.expect("the wait ended");
    // the wait ends: with `true`, or with the error of a call the shutdown cut short
    let _ = woken.expect("task");
    assert!(crate::modules::std::net::port_is_free(&addr), "the port is free");
    let (closed, listener_closed): (String, String) = lua
        .load(
            r#"
            local hyper = require("vnd:hyper")
            local _, err = server:addr()
            local _, serve_err = hyper.serve(listener, function() return "x" end)
            return err.kind, tostring(serve_err)
            "#,
        )
        .eval()
        .expect("script");
    assert_eq!(closed, "closed");
    assert_eq!(listener_closed, "vnd:hyper.serve: listener_closed: the listener is closed");
}
