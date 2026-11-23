//! The `vnd:hyper` client against servers in this process: a server that answers, one that never does, one that
//! trickles for ever, a redirect loop, one that drops the connection mid-body. Every request goes to a local port;
//! nothing here reaches the network, and the limits are set to a few hundred milliseconds so nothing is slow. No test
//! sleeps to wait for an event: a server tells the test what it saw through a channel or a shared log.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use mlua::Lua;
use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::{self, ServerConfig};

use crate::lifecycle::Life;
use crate::make_engine;

// ---------------------------------------------------------------------------
// servers
// ---------------------------------------------------------------------------

/// A request a test server received.
#[derive(Clone, Debug)]
struct Seen {
    /// The request line and the headers, as sent.
    head: String,
    body: Vec<u8>,
}

impl Seen {
    fn line(&self) -> &str {
        self.head.lines().next().unwrap_or_default()
    }

    /// The values of the header `name` (any case), one per line sent.
    fn header(&self, name: &str) -> Vec<String> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.head
            .lines()
            .skip(1)
            .filter(|l| l.to_ascii_lowercase().starts_with(&prefix))
            .map(|l| l[prefix.len()..].trim().to_owned())
            .collect()
    }
}

/// Read one request (head and a `content-length` body) from `stream`; `None` when the peer closed first.
async fn read_request<S: AsyncRead + Unpin>(stream: &mut S) -> Option<Seen> {
    let mut data = Vec::new();
    let mut chunk = [0_u8; 1024];
    let end = loop {
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let length = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = data[end..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some(Seen { head, body })
}

/// A complete response with `content-length` and `connection: close`.
fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n").into_bytes();
    for (k, v) in headers {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("content-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out
}

fn redirect(status: u16, location: &str) -> Vec<u8> {
    response(&format!("{status} Moved"), &[("location", location)], b"moved")
}

/// A listener on a free local port, and the task serving it: `serve` gets each connection.
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

/// An HTTP server that answers each request with `reply(request)` and closes, logging what it saw.
async fn http_server(
    reply: impl Fn(&Seen) -> Vec<u8> + Send + Sync + 'static,
) -> (u16, Arc<Mutex<Vec<Seen>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let reply = Arc::new(reply);
    let port = {
        let log = log.clone();
        listen(move |mut stream| {
            let (log, reply) = (log.clone(), reply.clone());
            async move {
                if let Some(request) = read_request(&mut stream).await {
                    let out = reply(&request);
                    log.lock().push(request);
                    let _ = stream.write_all(&out).await;
                    let _ = stream.shutdown().await;
                }
            }
        })
        .await
    };
    (port, log)
}

/// A TLS server, whose self-signed certificate names `names`, answering each request with `reply(request)`.
/// Returns the port and the certificate (PEM).
async fn tls_server_with(
    names: &[&str],
    reply: impl Fn(&Seen) -> Vec<u8> + Send + Sync + 'static,
) -> (u16, String) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>())
            .expect("certificate");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der())),
        )
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let reply = Arc::new(reply);
    let port = listen(move |stream| {
        let (acceptor, reply) = (acceptor.clone(), reply.clone());
        async move {
            // a client that refuses the certificate fails the handshake: nothing to serve then
            let Ok(mut tls) = acceptor.accept(stream).await else {
                return;
            };
            if let Some(request) = read_request(&mut tls).await {
                let _ = tls.write_all(&reply(&request)).await;
                let _ = tls.shutdown().await;
            }
        }
    })
    .await;
    (port, cert.pem())
}

/// A TLS server for `localhost` that answers every request with a body naming its request line.
async fn tls_server() -> (u16, String) {
    tls_server_with(&["localhost"], |req| response("200 OK", &[], format!("hello {}", req.line()).as_bytes()))
        .await
}

/// A port nothing listens on.
async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    listener.local_addr().expect("addr").port()
}

// ---------------------------------------------------------------------------
// scripts
// ---------------------------------------------------------------------------

const PRELUDE: &str = r#"
local hyper = require("vnd:hyper")
-- the call must have failed: its kind, the whole error as text, and the error itself
local function failed(r, e)
  assert(r == nil, "expected an error, got a response with status " .. tostring(r and r.status))
  assert(type(e) == "table" and type(e.kind) == "string", "not a typed error: " .. tostring(e))
  return e.kind, tostring(e), e
end
-- the call must have been refused as a wrong argument that says `what`
local function refused(what, f, ...)
  local ok, e = pcall(f, ...)
  assert(not ok, "expected the call to be refused: " .. what)
  assert(tostring(e):find(what, 1, true), what .. ": " .. tostring(e))
end
"#;

async fn run(lua: &Lua, body: &str) -> String {
    lua.load(format!("{PRELUDE}\n{body}")).eval_async().await.unwrap_or_else(|e| panic!("script failed: {e}"))
}

async fn engine() -> Lua {
    make_engine().expect("engine")
}

// ---------------------------------------------------------------------------
// TLS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn https_works_against_a_server_whose_certificate_is_given_as_ca_pem() {
    let (port, pem) = tls_server().await;
    let lua = engine().await;
    lua.globals().set("PEM", pem).expect("pem");
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("https://localhost:{port}/some/path", {{ ca_pem = PEM }})
               assert(r, e)
               -- post and request carry the option too
               local p, pe = hyper.post("https://localhost:{port}/p", "x", {{ ca_pem = PEM }})
               assert(p, pe)
               local q, qe = hyper.request("PUT", "https://localhost:{port}/q", nil, {{ ca_pem = PEM }})
               assert(q, qe)
               return r.status .. " " .. r.body .. " | " .. p.body .. " | " .. q.body .. " | " .. r.url"#
        ),
    )
    .await;
    assert_eq!(
        out,
        format!(
            "200 hello GET /some/path HTTP/1.1 | hello POST /p HTTP/1.1 | hello PUT /q HTTP/1.1 | https://localhost:{port}/some/path"
        )
    );
}

/// An IPv6 literal URL: the URL's host keeps its brackets (`[::1]`), the certificate names the bare address.
#[tokio::test]
async fn https_to_an_ipv6_literal_checks_the_ip_san() {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["::1".to_owned()]).expect("certificate");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der())),
        )
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("[::1]:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                if let Some(req) = read_request(&mut tls).await {
                    let _ = tls
                        .write_all(&response("200 OK", &[], format!("hello {}", req.line()).as_bytes()))
                        .await;
                    let _ = tls.shutdown().await;
                }
            });
        }
    });
    let lua = engine().await;
    lua.globals().set("PEM", cert.pem()).expect("pem");
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("https://[::1]:{port}/v6", {{ ca_pem = PEM }})
               assert(r, e)
               return r.body"#
        ),
    )
    .await;
    assert_eq!(out, "hello GET /v6 HTTP/1.1");
}

/// What the platform verifier is checked for here is the refusal: a self-signed certificate is not in the system
/// roots. That it accepts a certificate a public CA signed needs a public server and is not tested.
#[tokio::test]
async fn a_certificate_the_platform_does_not_trust_is_refused_without_ca_pem() {
    let (port, _pem) = tls_server().await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("https://localhost:{port}/"))
               assert(e:find("^vnd:hyper%.get: tls: "), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "tls");
}

#[tokio::test]
async fn a_ca_pem_that_is_not_the_servers_certificate_is_refused() {
    let (port, _pem) = tls_server().await;
    let (_, other) = tls_server().await;
    let lua = engine().await;
    lua.globals().set("PEM", other).expect("pem");
    let out =
        run(&lua, &format!(r#"return (failed(hyper.get("https://localhost:{port}/", {{ ca_pem = PEM }})))"#))
            .await;
    assert_eq!(out, "tls");
}

#[tokio::test]
async fn a_bad_ca_pem_is_an_invalid_input_error_not_a_panic() {
    let lua = engine().await;
    for (ca_pem, needle) in [
        (r#""nonsense""#, "ca_pem holds no certificate"),
        (
            r#""-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n""#,
            "ca_pem holds an unusable certificate",
        ),
    ] {
        // refused before anything is sent, for an http:// URL too, where it would never be used
        for url in ["https://localhost:1/", "http://localhost:1/"] {
            let out = run(
                &lua,
                &format!(
                    r#"local kind, e = failed(hyper.get("{url}", {{ ca_pem = {ca_pem} }}))
                       return kind .. "|" .. e"#
                ),
            )
            .await;
            assert!(out.starts_with("invalid_input|") && out.contains(needle), "{url} {ca_pem}: {out}");
        }
    }
    // what is not a string is not an option's value at all: the call is refused
    let out = run(
        &lua,
        r#"refused("vnd:hyper.get: argument #2 'opts'", hyper.get, "https://localhost:1/", { ca_pem = {} })
           refused("vnd:hyper.get: argument #2 'opts'", hyper.get, "http://localhost:1/", { ca_pem = 5 })
           return "ok""#,
    )
    .await;
    assert_eq!(out, "ok");
}

/// `ca_pem` applies to every hop, and a hop to another host name is verified against it too.
#[tokio::test]
async fn ca_pem_applies_to_every_hop() {
    let (port, pem) = tls_server_with(&["localhost", "127.0.0.1"], |req| {
        if req.line().starts_with("GET /start") {
            return redirect(
                302,
                &format!("https://127.0.0.1:{}/end", req.header("host")[0].rsplit(':').next().unwrap_or("")),
            );
        }
        response("200 OK", &[], format!("end {}", req.header("host")[0]).as_bytes())
    })
    .await;
    let lua = engine().await;
    lua.globals().set("PEM", pem).expect("pem");
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("https://localhost:{port}/start", {{ ca_pem = PEM }})
               assert(r, e)
               return r.status .. " " .. r.body .. " " .. r.url"#
        ),
    )
    .await;
    assert_eq!(out, format!("200 end 127.0.0.1:{port} https://127.0.0.1:{port}/end"));
    // and without it the first hop is refused
    let out = run(&lua, &format!(r#"return (failed(hyper.get("https://localhost:{port}/start")))"#)).await;
    assert_eq!(out, "tls");
}

/// A server that accepts the TCP connection and never speaks TLS: the handshake is under the connect timeout.
#[tokio::test]
async fn a_tls_handshake_that_never_completes_is_a_connect_timeout() {
    let port = listen(|mut stream| async move {
        let mut sink = [0_u8; 256];
        while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {}
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("https://localhost:{port}/", {{ connect_timeout = 0.2 }}))
               assert(e:find("connecting took longer than 0.2s", 1, true), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
}

// ---------------------------------------------------------------------------
// error kinds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_failure_has_a_kind_and_the_function_in_its_prefix() {
    let refused = closed_port().await;
    let garbage = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        let _ = stream.write_all(b"this is not http\r\n\r\n").await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local cases = {{
              {{ "invalid_url", hyper.get("not a url") }},
              {{ "invalid_url", hyper.get("ftp://localhost/") }},
              {{ "invalid_url", hyper.get("http://") }},
              {{ "invalid_url", hyper.get("/relative") }},
              {{ "connect", hyper.get("http://127.0.0.1:{refused}/") }},
              {{ "bad_response", hyper.get("http://127.0.0.1:{garbage}/") }},
              {{ "invalid_input", hyper.get("http://127.0.0.1:{refused}/", {{ max_body = 0 }}) }},
            }}
            for i, case in ipairs(cases) do
              local kind, e = failed(case[2], case[3])
              assert(kind == case[1], i .. ": expected " .. case[1] .. ", got " .. kind .. " (" .. e .. ")")
              assert(e:find("^vnd:hyper%.get: " .. kind .. ": "), e)
            end
            local _, post = failed(hyper.post("nope"))
            assert(post:find("^vnd:hyper%.post: invalid_url: "), post)
            local _, req = failed(hyper.request("GET", "nope"))
            assert(req:find("^vnd:hyper%.request: invalid_url: "), req)
            return "ok""#
        ),
    )
    .await;
    assert_eq!(out, "ok");
}

/// What `error_kind` and `error_message` read out of a string is the error's own fields now.
#[tokio::test]
async fn an_error_is_a_table_with_its_kind_its_message_and_what_the_kind_carries() {
    let (port, _) = http_server(|req| match req.line() {
        "GET /loop HTTP/1.1" => redirect(307, "/loop"),
        _ => response("200 OK", &[], b"0123456789"),
    })
    .await;
    let silent = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        let mut sink = [0_u8; 16];
        let _ = stream.read(&mut sink).await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"assert(hyper.error_kind == nil and hyper.error_message == nil)
               local base = "http://127.0.0.1:{port}"
               local _, text, err = failed(hyper.get("ftp://example.com/"))
               assert(err.kind == "invalid_url" and err.op == "vnd:hyper.get")
               assert(err.message == err.reason and err.message:find("only http:// and https://", 1, true))
               assert(text == "vnd:hyper.get: invalid_url: " .. err.message, text)

               local _, _, hops = failed(hyper.get(base .. "/loop", {{ max_redirects = 2 }}))
               assert(hops.kind == "too_many_redirects" and hops.max_redirects == 2)
               assert(hops.message == "more than 2 redirects" and hops.reason == nil)

               local _, _, large = failed(hyper.post(base .. "/", "x", {{ max_body = 9 }}))
               assert(large.kind == "too_large" and large.max_body == 9 and large.op == "vnd:hyper.post")
               assert(large.message == "the response body is longer than max_body (9 bytes)")

               local _, _, idle = failed(hyper.request("GET", "http://127.0.0.1:{silent}/", nil, {{ idle_timeout = 0.2 }}))
               assert(idle.kind == "timed_out" and idle.limit == "idle_timeout" and idle.op == "vnd:hyper.request")
               assert(idle.message == "no data for 0.2s" and idle.reason == idle.message)

               local _, _, total = failed(hyper.get("http://127.0.0.1:{silent}/", {{ timeout = 0.2 }}))
               assert(total.kind == "timed_out" and total.limit == "timeout", total.limit)
               return "ok""#
        ),
    )
    .await;
    assert_eq!(out, "ok");
}

// ---------------------------------------------------------------------------
// timeouts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_server_that_never_answers_ends_at_the_idle_timeout() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        // never answers; holds the connection until the client goes
        let mut sink = [0_u8; 16];
        let _ = stream.read(&mut sink).await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/", {{ idle_timeout = 0.2 }}))
               assert(e:find("^vnd:hyper%.get: timed_out: "), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
}

/// The headers and part of the body arrive, then nothing: the idle timeout ends it mid-body.
#[tokio::test]
async fn a_body_that_stalls_half_way_ends_at_the_idle_timeout() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\nonly ten b").await;
        // holds the connection open and says nothing more
        std::future::pending::<()>().await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/", {{ idle_timeout = 0.3 }}))
               assert(e:find("no data for 0.3s", 1, true), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
}

/// A server that accepts the connection and never reads: the upload fills the socket's buffers, the writes stop
/// moving, and the idle timeout ends the request (it counts writes as well as reads).
#[tokio::test]
async fn an_upload_the_server_stops_reading_ends_at_the_idle_timeout() {
    let port = listen(|stream| async move {
        let _held = stream;
        std::future::pending::<()>().await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local body = string.rep("x", 32 * 1024 * 1024)
               local kind, e = failed(hyper.post("http://127.0.0.1:{port}/", body, {{ idle_timeout = 0.3 }}))
               assert(e:find("no data for 0.3s", 1, true), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
}

/// Data keeps arriving, each piece sooner than the idle timeout: the whole takes longer than it and succeeds.
#[tokio::test]
async fn the_idle_timeout_is_reset_whenever_data_arrives() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 8\r\n\r\n").await;
        for piece in ["ab", "cd", "ef", "gh"] {
            tokio::time::sleep(Duration::from_millis(120)).await;
            let _ = stream.write_all(piece.as_bytes()).await;
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/", {{ idle_timeout = 0.3 }})
               assert(r, e)
               return r.body"#
        ),
    )
    .await;
    assert_eq!(out, "abcdefgh");
}

/// A body that never ends and never stops: the idle timeout never fires, the whole-request `timeout` does.
#[tokio::test]
async fn a_trickle_is_ended_by_the_whole_request_timeout() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        if stream.write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n").await.is_err() {
            return;
        }
        while stream.write_all(b"1\r\nx\r\n").await.is_ok() {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/", {{ timeout = 0.4 }}))
               assert(e:find("the request took longer than 0.4s", 1, true), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
}

/// A server that sends its headers a byte at a time, never finishing them, keeps resetting the idle timeout for
/// ever; `timeout` is the bound on it.
#[tokio::test]
async fn headers_that_trickle_in_never_trip_the_idle_timeout_but_trip_timeout() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        for byte in b"HTTP/1.1 200 OK\r\nx-slow: ".iter().chain(std::iter::repeat(&b'a')) {
            if stream.write_all(&[*byte]).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/", {{ idle_timeout = 0.25, timeout = 0.6 }}))
               assert(e:find("the request took longer than 0.6s", 1, true), "the idle timeout fired: " .. e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
}

/// `timeout` holds across a redirect chain: each hop is quick, the chain is not. (The server answers on a runtime
/// thread of its own, since its replies block.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_whole_request_timeout_holds_across_redirects() {
    let (port, log) = http_server(|req| {
        std::thread::sleep(Duration::from_millis(100));
        redirect(302, &format!("/{}", req.line().len()))
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/a", {{ timeout = 0.35 }}))
               assert(e:find("the request took longer than 0.35s", 1, true), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out");
    assert!(log.lock().len() < 16, "the chain was cut by the timeout, not the hop limit");
}

// ---------------------------------------------------------------------------
// the body cap
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_declared_length_over_the_cap_is_refused_before_the_body_is_read() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let port = listen(move |mut stream| {
        let tx = tx.clone();
        async move {
            let _ = read_request(&mut stream).await;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 1000000\r\n\r\n").await;
            // the client must go before a byte of body is asked for: wait for it to close
            let mut sink = [0_u8; 16];
            let n = stream.read(&mut sink).await.unwrap_or(0);
            let _ = tx.send(n);
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(r#"return (failed(hyper.get("http://127.0.0.1:{port}/", {{ max_body = 1000 }})))"#),
    )
    .await;
    assert_eq!(out, "too_large");
    assert_eq!(rx.recv().await, Some(0), "the server saw the connection close");
}

/// No length: a chunked body that never ends. The client stops reading at the cap and drops the connection.
#[tokio::test]
async fn an_endless_chunked_body_is_cut_at_the_cap_and_the_connection_dropped() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let port = listen(move |mut stream| {
        let tx = tx.clone();
        async move {
            let _ = read_request(&mut stream).await;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n").await;
            let chunk = [b"400\r\n".as_slice(), &[b'x'; 1024], b"\r\n"].concat();
            let mut sent = 0_usize;
            // runs until the client goes, which a write error says
            while stream.write_all(&chunk).await.is_ok() {
                sent += 1024;
            }
            let _ = tx.send(sent);
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/", {{ max_body = 10000 }}))
               assert(e:find("max_body (10000 bytes)", 1, true), e)
               return kind"#
        ),
    )
    .await;
    assert_eq!(out, "too_large");
    let sent = rx.recv().await.expect("the server's connection ended");
    assert!(sent >= 10_000, "the server sent {sent}");
}

#[tokio::test]
async fn a_body_at_the_cap_is_returned_and_one_byte_over_is_not() {
    let (port, _) = http_server(|_| response("200 OK", &[], &[b'z'; 1000])).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/", {{ max_body = 1000 }})
               assert(r, e)
               local kind = failed(hyper.get("http://127.0.0.1:{port}/", {{ max_body = 999 }}))
               return #r.body .. " " .. kind"#
        ),
    )
    .await;
    assert_eq!(out, "1000 too_large");
}

/// The module sends no `Accept-Encoding` and does not decode: a body labelled gzip comes back as the bytes sent,
/// and the cap counts those bytes.
#[tokio::test]
async fn a_compressed_response_is_returned_as_sent_and_capped_on_the_wire() {
    let wire: Vec<u8> = (0..2000_u32).map(|i| (i % 251) as u8).collect();
    let (port, log) = http_server(move |_| response("200 OK", &[("content-encoding", "gzip")], &wire)).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/")
               assert(r, e)
               local kind = failed(hyper.get("http://127.0.0.1:{port}/", {{ max_body = 1999 }}))
               return r.headers["content-encoding"] .. " " .. #r.body .. " " .. kind"#
        ),
    )
    .await;
    assert_eq!(out, "gzip 2000 too_large");
    assert!(log.lock()[0].header("accept-encoding").is_empty(), "no Accept-Encoding is sent");
}

// ---------------------------------------------------------------------------
// redirects
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_moved_url_is_followed_and_the_response_says_where_it_ended() {
    let (port, log) = http_server(|req| match req.line() {
        "GET /a HTTP/1.1" => redirect(301, "/dir/b"),
        "GET /dir/b HTTP/1.1" => redirect(302, "c?x=1#frag"),
        "GET /dir/c?x=1 HTTP/1.1" => redirect(308, "../d"),
        "GET /d HTTP/1.1" => response("200 OK", &[], b"arrived"),
        other => response("404 Not Found", &[], other.as_bytes()),
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/a")
               assert(r, e)
               return r.status .. " " .. r.body .. " " .. r.url"#
        ),
    )
    .await;
    assert_eq!(out, format!("200 arrived http://127.0.0.1:{port}/d"));
    assert_eq!(log.lock().len(), 4);
}

/// Every form a `Location` takes is resolved against the current URL: absolute, protocol-relative, root-relative,
/// relative, `..`, query only, with a fragment, with spaces and with the port written out.
#[tokio::test]
async fn every_form_of_location_is_resolved_against_the_current_url() {
    let (port, log) = http_server(move |req| {
        let line = req.line().to_owned();
        let port = req
            .header("host")
            .first()
            .and_then(|h| h.rsplit(':').next().map(str::to_owned))
            .unwrap_or_default();
        match line.as_str() {
            "GET /1 HTTP/1.1" => redirect(302, &format!("http://127.0.0.1:{port}/a/2")),
            "GET /a/2 HTTP/1.1" => redirect(302, &format!("//127.0.0.1:{port}/a/b/3")),
            "GET /a/b/3 HTTP/1.1" => redirect(302, "/a/b/4"),
            "GET /a/b/4 HTTP/1.1" => redirect(302, "5"),
            "GET /a/b/5 HTTP/1.1" => redirect(302, "../6"),
            "GET /a/6 HTTP/1.1" => redirect(302, "?q=7"),
            "GET /a/6?q=7 HTTP/1.1" => redirect(302, "/8#ignored"),
            "GET /8 HTTP/1.1" => redirect(302, "/with space/9"),
            "GET /with%20space/9 HTTP/1.1" => redirect(302, &format!("http://127.0.0.1:{port}/10")),
            "GET /10 HTTP/1.1" => response("200 OK", &[], b"arrived"),
            other => response("404 Not Found", &[], other.as_bytes()),
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/1")
               assert(r, e)
               return r.body .. " " .. r.url"#
        ),
    )
    .await;
    assert_eq!(out, format!("arrived http://127.0.0.1:{port}/10"));
    assert_eq!(log.lock().len(), 10);
}

/// A redirect's body is never read, so a hop that declares (or sends) more than `max_body` does not count against
/// it: the cap is on the response returned.
#[tokio::test]
async fn a_redirect_with_a_body_over_the_cap_is_followed_without_reading_it() {
    let port = listen(|mut stream| async move {
        let Some(req) = read_request(&mut stream).await else {
            return;
        };
        if req.line() == "GET /start HTTP/1.1" {
            let _ = stream
                .write_all(b"HTTP/1.1 302 Found\r\nlocation: /end\r\ncontent-length: 1000000000\r\n\r\n")
                .await;
            let _ = stream.write_all(&[b'x'; 4096]).await;
        } else {
            let _ = stream.write_all(&response("200 OK", &[], b"small")).await;
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/start", {{ max_body = 100 }})
               assert(r, e)
               return r.body"#
        ),
    )
    .await;
    assert_eq!(out, "small");
}

/// A length that lies: shorter than what is sent, the body is the declared length (the rest is never read, and the
/// connection is not reused); longer, the connection ends early and the call is `connection_lost` (tested above).
#[tokio::test]
async fn a_declared_length_shorter_than_the_body_is_the_body() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\n0123456789").await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/", {{ max_body = 5 }})
               assert(r, e)
               return r.body"#
        ),
    )
    .await;
    assert_eq!(out, "01234");
}

/// A `Location` with non-ASCII text (UTF-8 on the wire, as servers send it) is followed, percent-encoded.
#[tokio::test]
async fn a_location_with_non_ascii_text_is_followed() {
    let (port, log) = http_server(|req| match req.line() {
        "GET /start HTTP/1.1" => redirect(302, "/café"),
        _ => response("200 OK", &[], b"arrived"),
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/start")
               assert(r, e)
               return r.body .. " " .. r.url"#
        ),
    )
    .await;
    assert_eq!(out, format!("arrived http://127.0.0.1:{port}/caf%C3%A9"));
    assert_eq!(log.lock()[1].line(), "GET /caf%C3%A9 HTTP/1.1");
}

#[tokio::test]
async fn follow_redirects_false_returns_the_3xx_and_max_redirects_sets_the_limit() {
    let (port, log) = http_server(|req| {
        let n: usize = req
            .line()
            .split('/')
            .nth(1)
            .and_then(|s| s.split(' ').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        redirect(302, &format!("/{}", n + 1))
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local base = "http://127.0.0.1:{port}"
               local r, e = hyper.get(base .. "/0", {{ follow_redirects = false }})
               assert(r, e)
               local line = r.status .. " " .. r.headers.location .. " " .. r.url
               -- max_redirects: that many hops are followed, the next redirect is an error
               local kind, err = failed(hyper.get(base .. "/0", {{ max_redirects = 3 }}))
               assert(err:find("more than 3 redirects", 1, true), err)
               -- the default limit is 16, and a loop is cut by it
               local kind16, err16 = failed(hyper.get(base .. "/0"))
               assert(err16:find("more than 16 redirects", 1, true), err16)
               -- true is the default
               local kind_true = failed(hyper.get(base .. "/0", {{ follow_redirects = true }}))
               -- false follows none, whatever the limit says
               local none = assert(hyper.get(base .. "/0", {{ follow_redirects = false, max_redirects = 3 }}))
               assert(none.status == 302)
               return line .. " | " .. kind .. " " .. kind16 .. " " .. kind_true"#
        ),
    )
    .await;
    assert_eq!(
        out,
        format!(
            "302 /1 http://127.0.0.1:{port}/0 | too_many_redirects too_many_redirects too_many_redirects"
        )
    );
    // 1 + 4 + 17 + 17 + 1 requests: the limit counts redirects followed
    assert_eq!(log.lock().len(), 1 + 4 + 17 + 17 + 1);
}

#[tokio::test]
async fn a_redirect_loop_ends_at_the_limit() {
    let (port, log) = http_server(|req| {
        let to = if req.line().starts_with("GET /ping") { "/pong" } else { "/ping" };
        redirect(307, to)
    })
    .await;
    let lua = engine().await;
    let out = run(&lua, &format!(r#"return (failed(hyper.get("http://127.0.0.1:{port}/ping")))"#)).await;
    assert_eq!(out, "too_many_redirects");
    assert_eq!(log.lock().len(), 17);
}

#[tokio::test]
async fn redirects_rewrite_the_method_and_the_body_as_async_http_client_does() {
    let (port, log) = http_server(|req| {
        let path = req.line().split(' ').nth(1).unwrap_or("/").to_owned();
        match path.as_str() {
            "/c303" => redirect(303, "/end"),
            "/c301" => redirect(301, "/end"),
            "/c302" => redirect(302, "/end"),
            "/c307" => redirect(307, "/end"),
            "/c308" => redirect(308, "/end"),
            _ => response("200 OK", &[], b"end"),
        }
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local base = "http://127.0.0.1:{port}"
               local function go(method, path)
                 local r, e = hyper.request(method, base .. path, "payload",
                   {{ headers = {{ ["content-type"] = "text/plain", ["x-keep"] = "1" }} }})
                 assert(r, e)
                 return r.url
               end
               go("POST", "/c303")
               go("PUT", "/c303")
               go("POST", "/c301")
               go("POST", "/c302")
               go("PUT", "/c301")
               go("POST", "/c307")
               go("PUT", "/c308")
               local h = hyper.request("HEAD", base .. "/c303")
               assert(h, "head")
               return "ok""#
        ),
    )
    .await;
    assert_eq!(out, "ok");
    let log = log.lock().clone();
    let finals: Vec<(String, usize, Vec<String>, Vec<String>)> = log
        .iter()
        .filter(|s| s.line().contains(" /end "))
        .map(|s| {
            (
                s.line().split(' ').next().unwrap().to_owned(),
                s.body.len(),
                s.header("content-type"),
                s.header("x-keep"),
            )
        })
        .collect();
    let want = [
        ("GET", 0, 0, true),   // POST, 303
        ("GET", 0, 0, true),   // PUT, 303
        ("GET", 0, 0, true),   // POST, 301
        ("GET", 0, 0, true),   // POST, 302
        ("PUT", 7, 1, true),   // PUT, 301 keeps method and body
        ("POST", 7, 1, true),  // POST, 307
        ("PUT", 7, 1, true),   // PUT, 308
        ("HEAD", 0, 0, false), // HEAD, 303 (sent no headers of its own)
    ];
    assert_eq!(finals.len(), want.len(), "{finals:?}");
    for ((method, body, types, keep), (w_method, w_body, w_types, w_keep)) in finals.iter().zip(want) {
        assert_eq!(
            (method.as_str(), *body, types.len(), !keep.is_empty()),
            (w_method, w_body, w_types, w_keep),
            "{finals:?}"
        );
    }
    // a dropped body takes `Content-Length` with it: the GETs sent none
    for s in log.iter().filter(|s| s.line().starts_with("GET /end")) {
        assert!(s.header("content-length").is_empty(), "{}", s.head);
    }
}

/// The credentials stay with the origin they were written for.
#[tokio::test]
async fn a_cross_origin_hop_drops_the_credentials_and_a_same_origin_hop_keeps_them() {
    let (target, target_log) = http_server(|_| response("200 OK", &[], b"target")).await;
    let (origin, origin_log) = http_server(move |req| match req.line() {
        "GET /same HTTP/1.1" => redirect(302, "/inner"),
        "GET /other-port HTTP/1.1" => redirect(302, &format!("http://127.0.0.1:{target}/landed")),
        "GET /other-host HTTP/1.1" => redirect(302, &format!("http://localhost:{target}/landed")),
        _ => response("200 OK", &[], b"inner"),
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local headers = {{ Authorization = "Bearer secret", Cookie = "s=1", ["Proxy-Authorization"] = "Basic x",
                                 Origin = "http://a.example", Host = "vhost.example", ["X-Keep"] = "yes" }}
               for _, path in ipairs({{ "/same", "/other-port", "/other-host" }}) do
                 local r, e = hyper.get("http://127.0.0.1:{origin}" .. path, {{ headers = headers }})
                 assert(r, e)
               end
               return "ok""#
        ),
    )
    .await;
    assert_eq!(out, "ok");
    // same origin: the second request of /same kept everything, including the caller's Host
    let inner = origin_log.lock().iter().find(|s| s.line() == "GET /inner HTTP/1.1").cloned().expect("inner");
    for name in ["authorization", "cookie", "proxy-authorization", "origin", "x-keep"] {
        assert_eq!(inner.header(name).len(), 1, "{name} was dropped on a same-origin hop: {}", inner.head);
    }
    assert_eq!(inner.header("host"), ["vhost.example"]);
    // another port and another host name: the credentials and the Host are gone, the rest is kept
    let landed = target_log.lock().clone();
    assert_eq!(landed.len(), 2);
    for s in &landed {
        for name in ["authorization", "cookie", "proxy-authorization", "origin"] {
            assert!(s.header(name).is_empty(), "{name} leaked across origins: {}", s.head);
        }
        assert_eq!(s.header("x-keep"), ["yes"]);
        assert_eq!(s.header("host").len(), 1);
        assert_ne!(s.header("host")[0], "vhost.example");
    }
}

/// A redirect from `https` to `http` is another origin: it is followed (the reference does), without credentials.
#[tokio::test]
async fn a_redirect_from_https_to_http_is_followed_without_the_credentials() {
    let (plain, plain_log) = http_server(|_| response("200 OK", &[], b"plain")).await;
    let (port, pem) =
        tls_server_with(&["localhost"], move |_| redirect(302, &format!("http://127.0.0.1:{plain}/down")))
            .await;
    let lua = engine().await;
    lua.globals().set("PEM", pem).expect("pem");
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("https://localhost:{port}/", {{ ca_pem = PEM, headers = {{ Authorization = "Bearer secret" }} }})
               assert(r, e)
               return r.body .. " " .. r.url"#
        ),
    )
    .await;
    assert_eq!(out, format!("plain http://127.0.0.1:{plain}/down"));
    assert!(plain_log.lock()[0].header("authorization").is_empty());
}

#[tokio::test]
async fn a_redirect_to_another_scheme_a_bad_location_or_no_location_is_handled() {
    let (port, _) = http_server(|req| match req.line() {
        "GET /ftp HTTP/1.1" => redirect(302, "ftp://example.com/file"),
        "GET /file HTTP/1.1" => redirect(302, "file:///etc/passwd"),
        "GET /js HTTP/1.1" => redirect(302, "javascript:alert(1)"),
        "GET /bad HTTP/1.1" => redirect(302, "http://[bad"),
        "GET /blob HTTP/1.1" => redirect(302, &format!("{}https://example.com/", "blob:".repeat(20_000))),
        "GET /data HTTP/1.1" => redirect(302, "data:text/plain,hi"),
        "GET /ws HTTP/1.1" => redirect(302, "ws://example.com/socket"),
        "GET /none HTTP/1.1" => response("302 Found", &[], b"no location"),
        "GET /multiple HTTP/1.1" => response("300 Multiple Choices", &[("location", "/x")], b"choose"),
        "GET /notmodified HTTP/1.1" => response("304 Not Modified", &[("location", "/x")], b""),
        _ => response("404 Not Found", &[], b""),
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local base = "http://127.0.0.1:{port}"
               local seen = {{}}
               for _, path in ipairs({{ "/ftp", "/file", "/js", "/bad", "/blob", "/data", "/ws" }}) do
                 local kind, e = failed(hyper.get(base .. path))
                 assert(e:find("Location", 1, true), e)
                 seen[#seen + 1] = kind
               end
               for _, path in ipairs({{ "/none", "/multiple", "/notmodified" }}) do
                 local r, e = hyper.get(base .. path)
                 assert(r, e)
                 seen[#seen + 1] = r.status
               end
               return table.concat(seen, " ")"#
        ),
    )
    .await;
    assert_eq!(
        out,
        "invalid_url invalid_url invalid_url invalid_url invalid_url invalid_url invalid_url 302 300 304"
    );
}

// ---------------------------------------------------------------------------
// the request sent is the request asked for
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_content_length_that_disagrees_with_the_body_is_refused_before_anything_is_sent() {
    let (port, log) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local url = "http://127.0.0.1:{port}/"
               local kinds = {{}}
               for _, case in ipairs({{
                 {{ body = "abc", length = "1" }},
                 {{ body = "abc", length = "4" }},
                 {{ body = "", length = "1" }},
                 {{ body = "abc", length = "three" }},
               }}) do
                 local kind, e = failed(hyper.post(url, case.body, {{ headers = {{ ["Content-Length"] = case.length }} }}))
                 assert(e:find("Content-Length", 1, true), e)
                 kinds[#kinds + 1] = kind
               end
               -- a length that is right is sent once, whatever its case
               local r, e = hyper.post(url, "abc", {{ headers = {{ ["content-LENGTH"] = "3" }} }})
               assert(r, e)
               -- a GET with no body may say 0
               local g, ge = hyper.get(url, {{ headers = {{ ["Content-Length"] = "0" }} }})
               assert(g, ge)
               return table.concat(kinds, " ")"#
        ),
    )
    .await;
    assert_eq!(out, "invalid_input invalid_input invalid_input invalid_input");
    let log = log.lock().clone();
    assert_eq!(log.len(), 2, "only the two valid requests were sent");
    assert_eq!(log[0].header("content-length"), ["3"]);
    assert_eq!(log[0].body, b"abc");
}

#[tokio::test]
async fn a_caller_set_host_replaces_the_built_in_one_and_the_default_port_is_left_out() {
    let (port, log) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    run(
        &lua,
        &format!(
            r#"local url = "http://127.0.0.1:{port}/"
               assert(hyper.get(url, {{ headers = {{ host = "virtual.example" }} }}))
               assert(hyper.get(url))
               return "ok""#
        ),
    )
    .await;
    let log = log.lock().clone();
    assert_eq!(log[0].header("host"), ["virtual.example"], "{}", log[0].head);
    assert_eq!(log[1].header("host"), [format!("127.0.0.1:{port}")]);
}

/// A header the server sends twice is one value joined with ", " (as the Fetch standard's `Headers.get`), `set-cookie`
/// with "\n" so cookies holding a comma split back apart, and a value that is not UTF-8 is decoded with replacement
/// characters: nothing is dropped.
#[tokio::test]
async fn response_headers_sent_twice_or_not_utf8_are_kept() {
    let port = listen(|mut stream| async move {
        let _ = read_request(&mut stream).await;
        let out = b"HTTP/1.1 200 OK\r\nset-cookie: a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT\r\nSet-Cookie: b=2\r\nx-list: one\r\nx-list: two\r\nx-name: caf\xe9\r\nx-utf8: caf\xc3\xa9\r\ncontent-length: 2\r\n\r\nok".to_vec();
        let _ = stream.write_all(&out).await;
    })
    .await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/")
               assert(r, e)
               local cookies = {{}}
               for cookie in r.headers["set-cookie"]:gmatch("[^\n]+") do cookies[#cookies + 1] = cookie end
               return #cookies .. "|" .. cookies[1] .. "|" .. r.headers["x-list"] .. "|" .. r.headers["x-name"] .. "|" .. r.headers["x-utf8"]"#
        ),
    )
    .await;
    assert_eq!(out, "2|a=1; Expires=Wed, 21 Oct 2026 07:28:00 GMT|one, two|caf\u{fffd}|café");
}

#[tokio::test]
async fn a_header_named_twice_or_a_transfer_encoding_is_refused_before_anything_is_sent() {
    let (port, log) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local url = "http://127.0.0.1:{port}/"
               local seen = {{}}
               for _, headers in ipairs({{
                 {{ Host = "a.example", host = "b.example" }},
                 {{ ["Content-Length"] = "3", ["content-length"] = "3" }},
                 {{ ["Transfer-Encoding"] = "chunked" }},
               }}) do
                 local kind, e = failed(hyper.post(url, "abc", {{ headers = headers }}))
                 assert(e:find("twice", 1, true) or e:find("Transfer-Encoding", 1, true), e)
                 seen[#seen + 1] = kind
               end
               return table.concat(seen, " ")"#
        ),
    )
    .await;
    assert_eq!(out, "invalid_input invalid_input invalid_input");
    assert!(log.lock().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn a_standard_method_in_any_case_is_the_standard_method() {
    let (port, log) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local url = "http://127.0.0.1:{port}/"
               for _, m in ipairs({{ "put", "Patch", "delete", "get", "PuRgE", "MKCOL" }}) do
                 assert(hyper.request(m, url))
               end
               local kind = failed(hyper.request("bad method", url))
               local kind2 = failed(hyper.request("", url))
               return kind .. " " .. kind2"#
        ),
    )
    .await;
    assert_eq!(out, "invalid_input invalid_input");
    let seen = log.lock().clone();
    let methods: Vec<&str> = seen.iter().map(|s| s.line().split(' ').next().unwrap()).collect();
    // `PuRgE` is no standard method: it goes as written
    assert_eq!(methods, ["PUT", "PATCH", "DELETE", "GET", "PuRgE", "MKCOL"]);
    // a PUT and a PATCH with no body carry content, so they say so
    assert_eq!(seen[0].header("content-length"), ["0"]);
    assert_eq!(seen[1].header("content-length"), ["0"]);
    assert!(seen[2].header("content-length").is_empty());
}

/// What an option's type cannot say is an `invalid_input` error that names the option, returned before anything
/// is sent.
#[tokio::test]
async fn a_bad_option_value_is_an_invalid_input_error_that_names_it() {
    let (port, log) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local url = "http://127.0.0.1:{port}/"
               local function bad(needle, opts, how)
                 local kind, e, err = failed((how or hyper.get)(url, opts))
                 assert(kind == "invalid_input" and e:find(needle, 1, true), needle .. ": " .. e)
                 assert(err.reason == err.message)
               end
               for _, name in ipairs({{ "timeout", "idle_timeout", "connect_timeout" }}) do
                 bad(name .. " must be", {{ [name] = 0 }})
                 bad(name .. " must be", {{ [name] = -1 }})
                 bad(name .. " must be", {{ [name] = 0 / 0 }})
                 bad(name .. " must be", {{ [name] = math.huge }})
                 bad(name .. " must be", {{ [name] = 1e300 }})
               end
               bad("max_body must be", {{ max_body = 0 }})
               bad("max_redirects must be", {{ max_redirects = 0 }})
               bad("is not a valid header name", {{ headers = {{ ["bad name"] = "x" }} }})
               bad("is not valid", {{ headers = {{ ["x-a"] = "line\r\nx-b: injected" }} }})
               bad("ca_pem holds no certificate", {{ ca_pem = "nonsense" }})
               local function post(_, opts) return hyper.post(url, "x", opts) end
               local function request(_, opts) return hyper.request("GE T", url, nil, opts) end
               bad("timeout must be", {{ timeout = 0 }}, post)
               bad("method is not a valid HTTP method name", nil, request)
               bad("method is not a valid HTTP method name", nil, function() return hyper.request("", url) end)
               return "ok""#
        ),
    )
    .await;
    assert_eq!(out, "ok");
    assert!(log.lock().is_empty(), "nothing was sent");
}

/// What is not an option, or not of an option's type, is not a request at all: the call is refused as a wrong
/// argument that names it, and a misspelt option is never dropped. (neoc-vm returned these as `invalid_input`.)
#[tokio::test]
async fn an_unknown_or_wrongly_typed_option_is_refused_as_an_argument() {
    let (port, log) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local url = "http://127.0.0.1:{port}/"
               local function bad(needle, opts)
                 refused(needle, hyper.get, url, opts)
               end
               -- a value of the wrong type is refused where the options are read
               local function wrong(opts)
                 bad("vnd:hyper.get: argument #2 'opts'", opts)
               end
               for _, name in ipairs({{ "timeout", "idle_timeout", "connect_timeout" }}) do
                 wrong({{ [name] = "5" }})
                 wrong({{ [name] = true }})
               end
               wrong({{ max_body = 1.5 }})
               wrong({{ max_body = -1 }})
               wrong({{ max_body = "big" }})
               wrong({{ max_redirects = 2.5 }})
               wrong({{ max_redirects = -1 }})
               wrong({{ follow_redirects = 3 }})
               wrong({{ follow_redirects = "yes" }})
               wrong({{ follow_redirects = {{}} }})
               wrong({{ headers = "Accept: text/plain" }})
               wrong({{ headers = {{ "Accept: text/plain" }} }})
               wrong({{ headers = {{ Accept = true }} }})
               wrong({{ headers = {{ Accept = {{}} }} }})
               -- a number is not a header value: write it as text
               wrong({{ headers = {{ ["x-n"] = 5 }} }})
               wrong({{ ca_pem = 1 }})
               bad("unknown field `stream_body`", {{ stream_body = true }})
               bad("unknown field `method`", {{ method = "GET" }})
               bad("vnd:hyper.get: argument #2 'opts'", {{ [1] = true }})
               bad("vnd:hyper.get: argument #2 'opts'", "timeout = 5")
               -- the URL, the method and the body are arguments, checked as such
               refused("vnd:hyper.get: argument #1 'url'", hyper.get)
               refused("vnd:hyper.get: argument #1 'url'", hyper.get, {{}})
               refused("vnd:hyper.post: argument #1 'url'", hyper.post, {{ url = url }})
               refused("vnd:hyper.post: argument #2 'body'", hyper.post, url, {{}})
               refused("vnd:hyper.post: argument #2 'body'", hyper.post, url, true)
               refused("vnd:hyper.post: argument #2 'body'", hyper.post, url, 42)
               refused("vnd:hyper.post: argument #2 'body'", hyper.post, url, buffer.fromstring("bytes"))
               refused("vnd:hyper.request: argument #1 'method'", hyper.request, 1, url)
               refused("vnd:hyper.request: argument #2 'url'", hyper.request, "GET")
               refused("vnd:hyper.request: argument #3 'body'", hyper.request, "POST", url, {{ body = "x" }})
               refused("vnd:hyper.request: argument #4 'opts'", hyper.request, "POST", url, "x", {{ timeuot = 5 }})
               return "ok""#
        ),
    )
    .await;
    assert_eq!(out, "ok");
    assert!(log.lock().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn the_limits_can_be_raised_and_a_huge_timeout_does_not_panic() {
    let (port, _) = http_server(|_| response("200 OK", &[], b"ok")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local r, e = hyper.get("http://127.0.0.1:{port}/", {{ timeout = 1e9, idle_timeout = 1e9, connect_timeout = 1e9,
                                                                   max_body = 2^53, max_redirects = 1e9 }})
               assert(r, e)
               return r.body"#
        ),
    )
    .await;
    assert_eq!(out, "ok");
}

// ---------------------------------------------------------------------------
// what a request holds is released
// ---------------------------------------------------------------------------

/// A server that holds the connection and tells the test when it is accepted and when the client is gone.
async fn holding_server() -> (u16, tokio::sync::mpsc::UnboundedReceiver<&'static str>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let port = listen(move |mut stream| {
        let tx = tx.clone();
        async move {
            let _ = read_request(&mut stream).await;
            let _ = tx.send("accepted");
            let mut sink = [0_u8; 16];
            let _ = stream.read(&mut sink).await;
            let _ = tx.send("closed");
        }
    })
    .await;
    (port, rx)
}

/// The engine shut down while a request is waiting: the connection is closed, and the call fails instead of waiting
/// for ever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutting_the_engine_down_closes_the_connection_of_a_waiting_request() {
    let (port, mut rx) = holding_server().await;
    let lua = engine().await;
    let call = {
        let lua = lua.clone();
        tokio::spawn(async move {
            run(&lua, &format!(r#"return (failed(hyper.get("http://127.0.0.1:{port}/")))"#)).await
        })
    };
    assert_eq!(rx.recv().await, Some("accepted"));
    Life::of(&lua).shutdown();
    assert_eq!(rx.recv().await, Some("closed"));
    let kind =
        tokio::time::timeout(Duration::from_secs(10), call).await.expect("the call ended").expect("task");
    assert_eq!(kind, "connection_lost");
}

/// What the server does after it has written the start of a response.
#[derive(Clone, Copy)]
enum Then {
    /// Closes the connection.
    Close,
    /// Keeps it open and silent.
    Hold,
    /// Sends one more byte of a chunked body every 30 ms, for ever.
    Trickle,
}

/// What the test does to the engine once the start of the body has been written.
#[derive(Clone, Copy)]
enum Engine {
    Nothing,
    StopBackground,
    Shutdown,
}

/// Invariant: a response body reaches the script whole, or the call fails with a kind that says why; a body that
/// ended early is never returned as if it were complete. One row per way a body ends early: the server closing
/// before its `Content-Length`, a chunked body cut mid-chunk or between chunks (before the terminating chunk), the
/// idle timeout or the whole-request timeout firing mid-body, the cap reached, and the engine aborting the
/// connection's driver (hyper ends a body the same way then as when it is complete). A call the caller abandons
/// returns nothing to anyone (`dropping_a_waiting_request_closes_its_connection`), and a redirect's body is never
/// read at all (`a_redirect_with_a_body_over_the_cap_is_followed_without_reading_it`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_that_ends_early_is_an_error_never_a_short_body() {
    const CHUNKED: &[u8] = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n";
    // (name, the head written, the start of the body, then, options, what the engine does, the kind)
    type Row = (&'static str, &'static [u8], &'static [u8], Then, &'static str, Engine, &'static str);
    let cases: [Row; 9] = [
        (
            "closed before its length",
            b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n",
            b"only ten b",
            Then::Close,
            "{}",
            Engine::Nothing,
            "connection_lost",
        ),
        (
            "chunked, closed mid-chunk",
            CHUNKED,
            b"8\r\npart",
            Then::Close,
            "{}",
            Engine::Nothing,
            "connection_lost",
        ),
        (
            "chunked, closed between chunks",
            CHUNKED,
            b"4\r\npart\r\n",
            Then::Close,
            "{}",
            Engine::Nothing,
            "connection_lost",
        ),
        (
            "idle timeout mid-body",
            b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n",
            b"part",
            Then::Hold,
            "{ idle_timeout = 0.3 }",
            Engine::Nothing,
            "timed_out",
        ),
        (
            "whole-request timeout mid-body",
            CHUNKED,
            b"4\r\npart\r\n",
            Then::Trickle,
            "{ timeout = 0.4 }",
            Engine::Nothing,
            "timed_out",
        ),
        (
            "the cap",
            CHUNKED,
            b"4\r\npart\r\n",
            Then::Trickle,
            "{ max_body = 6 }",
            Engine::Nothing,
            "too_large",
        ),
        (
            "engine stops its background work, chunked",
            CHUNKED,
            b"4\r\npart\r\n",
            Then::Hold,
            "{}",
            Engine::StopBackground,
            "connection_lost",
        ),
        (
            "engine shuts down, with a length",
            b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\n",
            b"part",
            Then::Hold,
            "{}",
            Engine::Shutdown,
            "connection_lost",
        ),
        (
            "engine shuts down, chunked",
            CHUNKED,
            b"4\r\npart\r\n",
            Then::Hold,
            "{}",
            Engine::Shutdown,
            "connection_lost",
        ),
    ];
    for (name, head, part, then, opts, action, want) in cases {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let port = listen(move |mut stream| {
            let tx = tx.clone();
            async move {
                let _ = read_request(&mut stream).await;
                let _ = stream.write_all(head).await;
                let _ = stream.write_all(part).await;
                let _ = tx.send(());
                match then {
                    Then::Close => {}
                    Then::Hold => {
                        let mut sink = [0_u8; 64];
                        while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {}
                    }
                    Then::Trickle => {
                        while stream.write_all(b"1\r\nx\r\n").await.is_ok() {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                        }
                    }
                }
            }
        })
        .await;
        let lua = engine().await;
        let call = {
            let lua = lua.clone();
            tokio::spawn(async move {
                run(
                    &lua,
                    &format!(
                        r#"local kind, e = failed(hyper.get("http://127.0.0.1:{port}/", {opts}))
                                return kind"#
                    ),
                )
                .await
            })
        };
        // the start of the body was written; whether the client has read it yet or not, the body is cut
        rx.recv().await.expect("the server wrote the start of the body");
        match action {
            Engine::Nothing => {}
            Engine::StopBackground => Life::of(&lua).stop_background(),
            Engine::Shutdown => Life::of(&lua).shutdown(),
        }
        let kind = tokio::time::timeout(Duration::from_secs(10), call)
            .await
            .unwrap_or_else(|_| panic!("{name}: the call did not end"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(kind, want, "{name}");
    }
}

/// A call that is dropped (its caller gave up) closes its connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_waiting_request_closes_its_connection() {
    let (port, mut rx) = holding_server().await;
    let lua = engine().await;
    let call = {
        let lua = lua.clone();
        tokio::spawn(async move {
            run(&lua, &format!(r#"return (failed(hyper.get("http://127.0.0.1:{port}/")))"#)).await
        })
    };
    assert_eq!(rx.recv().await, Some("accepted"));
    call.abort();
    assert_eq!(rx.recv().await, Some("closed"));
}

/// Whatever ends a request, its connection is closed: the idle timeout, the whole-request timeout, and (after
/// the headers) the body cap. The server sees the close each time; nothing is left open behind an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_way_a_request_fails_closes_its_connection() {
    let cases: [(&str, &[u8]); 3] = [
        ("{ idle_timeout = 0.2 }", b""),
        ("{ timeout = 0.2 }", b""),
        ("{ max_body = 4 }", b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n8\r\n01234567\r\n"),
    ];
    for (opts, reply) in cases {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let port = listen(move |mut stream| {
            let tx = tx.clone();
            async move {
                let _ = read_request(&mut stream).await;
                let _ = stream.write_all(reply).await;
                let _ = tx.send("answered");
                // reads until the client closes its end
                let mut sink = [0_u8; 64];
                while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {}
                let _ = tx.send("closed");
            }
        })
        .await;
        let lua = engine().await;
        let kind =
            run(&lua, &format!(r#"return (failed(hyper.get("http://127.0.0.1:{port}/", {opts})))"#)).await;
        assert!(matches!(kind.as_str(), "timed_out" | "too_large"), "{opts}: {kind}");
        assert_eq!(rx.recv().await, Some("answered"), "{opts}");
        assert_eq!(rx.recv().await, Some("closed"), "{opts}: the connection was left open");
    }
}

/// The request's own socket is what the idle timer watches, and it is unaffected by another request's: two requests
/// at once, one idle, one finishing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_in_flight_together_time_out_on_their_own() {
    let (slow, _rx) = holding_server().await;
    let (fast, _) = http_server(|_| response("200 OK", &[], b"fast")).await;
    let lua = engine().await;
    let out = run(
        &lua,
        &format!(
            r#"local task = require("std:task")
               local a = task.spawn(function() return (failed(hyper.get("http://127.0.0.1:{slow}/", {{ idle_timeout = 0.2 }}))) end)
               local b = task.spawn(function() return assert(hyper.get("http://127.0.0.1:{fast}/")).body end)
               return a:await() .. " " .. b:await()"#
        ),
    )
    .await;
    assert_eq!(out, "timed_out fast");
}
