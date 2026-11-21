//! `vnd:hyper` client path — the `get`/`post`/`request` entry points and the
//! one-shot HTTP/1.1 request execution behind them. The server and request
//! halves live in `super::server` / `super`.
//!
//! `http://` and `https://` both work. TLS is rustls over the `ring` provider: the certificate is checked by
//! the platform verifier (the operating system's trust store). A service with its own CA or a self-signed
//! certificate passes `ca_pem = "<PEM>"` in the options: then exactly those certificates are trusted for
//! that request.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request as HyperRequest, Uri};
use hyper_util::rt::TokioIo;
use mlua::{Lua, Table, Value};
use std::future::Future;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{
    TlsConnector,
    rustls::{
        self, ClientConfig, RootCertStore,
        pki_types::{CertificateDer, ServerName, pem::PemObject},
    },
};
use tracing::Instrument;

use crate::lua::sandbox::{from_err, ok};

// Each entry point reads its options table in the synchronous part of the call, before it builds the future that
// performs the request. A script table can run script code when it is read (a `__index`), which can call the same
// entry point again: read inside the future, every level would stack another polled future on the C stack (about
// 20 KiB a level in release, about 150 KiB in debug, with Luau's C-call limit at 200 levels), and read here each
// level is a few hundred bytes. `thread.spawn` reads its options the same way (`prepare_spawn`).

type Request = (
    &'static str,
    String,
    String,
    Option<Vec<u8>>,
    Vec<(String, String)>,
    Option<String>,
);

pub(super) fn get(
    lua: Lua,
    (url, opts): (String, Option<Table>),
) -> impl Future<Output = mlua::Result<(Value, Value)>> + Send + 'static {
    let prepared: Result<Request, String> = opts.as_ref().map(ca_from_opts).transpose().map(|ca| {
        (
            "hyper.get",
            "GET".into(),
            url,
            None,
            Vec::new(),
            ca.flatten(),
        )
    });
    send(lua, "hyper.get", prepared)
}

pub(super) fn post(
    lua: Lua,
    opts: Table,
) -> impl Future<Output = mlua::Result<(Value, Value)>> + Send + 'static {
    let prepared = extract_opts(&opts).map(|(url, body, headers, ca_pem)| {
        ("hyper.post", "POST".into(), url, body, headers, ca_pem)
    });
    send(lua, "hyper.post", prepared)
}

pub(super) fn request(
    lua: Lua,
    opts: Table,
) -> impl Future<Output = mlua::Result<(Value, Value)>> + Send + 'static {
    let prepared = opts
        .get::<String>("method")
        .map_err(|e| e.to_string())
        .and_then(|method| {
            extract_opts(&opts).map(|(url, body, headers, ca_pem)| {
                ("hyper.request", method, url, body, headers, ca_pem)
            })
        });
    send(lua, "hyper.request", prepared)
}

/// The future of a prepared request, or the `(nil, err)` of options that could not be read. Holds no script table.
/// It runs inside a span named after the entry point (`hyper.get`, `hyper.post`, `hyper.request`: a span name is a
/// literal, so each is written out), at target `lua::lib::hyper`, as the `#[instrument]` it replaces opened.
fn send(
    lua: Lua,
    prefix: &'static str,
    prepared: Result<Request, String>,
) -> impl Future<Output = mlua::Result<(Value, Value)>> + Send + 'static {
    let span = match prefix {
        "hyper.get" => tracing::info_span!(target: "lua::lib::hyper", "hyper.get"),
        "hyper.post" => tracing::info_span!(target: "lua::lib::hyper", "hyper.post"),
        _ => tracing::info_span!(target: "lua::lib::hyper", "hyper.request"),
    };
    async move {
        match prepared {
            Ok((prefix, method, url, body, headers, ca_pem)) => {
                perform(&lua, prefix, method, url, body, headers, ca_pem).await
            }
            Err(e) => from_err(&lua, prefix, e),
        }
    }
    .instrument(span)
}

// ---------------------------------------------------------------------------
// Lua → Rust extraction
// ---------------------------------------------------------------------------

/// Pull the shared `url` / `body` / `headers` / `ca_pem` fields out of an opts table.
/// Errors stringify so each caller can wrap them under its own prefix.
type Opts = (
    String,
    Option<Vec<u8>>,
    Vec<(String, String)>,
    Option<String>,
);

fn extract_opts(opts: &Table) -> Result<Opts, String> {
    let url: String = opts.get("url").map_err(|e| e.to_string())?;
    let body = body_from_opts(opts)?;
    let headers = headers_from_opts(opts)?;
    let ca_pem = ca_from_opts(opts)?;
    Ok((url, body, headers, ca_pem))
}

/// The optional `ca_pem` string: the PEM certificates to trust instead of the platform's.
fn ca_from_opts(opts: &Table) -> Result<Option<String>, String> {
    opts.get::<Option<String>>("ca_pem")
        .map_err(|e| format!("ca_pem must be a string: {e}"))
}

/// The optional `body`: a string's bytes or a buffer's (a number is sent as its text, as Lua converts it). Anything
/// else is an error, never an empty body.
fn body_from_opts(opts: &Table) -> Result<Option<Vec<u8>>, String> {
    match opts.get::<Value>("body").map_err(|e| e.to_string())? {
        Value::Nil => Ok(None),
        Value::String(s) => Ok(Some(s.as_bytes().to_vec())),
        Value::Buffer(b) => Ok(Some(b.to_vec())),
        // Lua's own number-to-string conversion, as the body was read before
        Value::Integer(_) | Value::Number(_) => opts
            .get::<mlua::LuaString>("body")
            .map(|s| Some(s.as_bytes().to_vec()))
            .map_err(|e| e.to_string()),
        other => Err(format!(
            "body must be a string or a buffer, got {}",
            other.type_name()
        )),
    }
}

/// The optional `headers` table of name to value strings. Anything else is an error, never no headers.
fn headers_from_opts(opts: &Table) -> Result<Vec<(String, String)>, String> {
    match opts.get::<Value>("headers").map_err(|e| e.to_string())? {
        Value::Nil => Ok(Vec::new()),
        Value::Table(t) => t
            .pairs::<String, String>()
            .collect::<mlua::Result<_>>()
            .map_err(|e| format!("headers must map strings to strings: {e}")),
        other => Err(format!(
            "headers must be a table, got {}",
            other.type_name()
        )),
    }
}

// ---------------------------------------------------------------------------
// Client — hyper request
// ---------------------------------------------------------------------------

struct ClientResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn perform(
    lua: &Lua,
    prefix: &str,
    method: String,
    url: String,
    body: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
    ca_pem: Option<String>,
) -> mlua::Result<(Value, Value)> {
    match do_request(method, url, body, headers, ca_pem).await {
        Ok(resp) => {
            let t = lua.create_table()?;
            t.set("status", resp.status as i64)?;
            let h = lua.create_table()?;
            for (k, v) in resp.headers {
                h.set(k, v)?;
            }
            t.set("headers", h)?;
            t.set("body", lua.create_string(&resp.body)?)?;
            ok(lua, t)
        }
        Err(e) => from_err(lua, prefix, e),
    }
}

async fn do_request(
    method: String,
    url: String,
    body: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
    ca_pem: Option<String>,
) -> Result<ClientResponse, String> {
    let target = Target::parse(&url)?;
    let tls = if target.tls {
        Some(tls_connector(ca_pem.as_deref())?)
    } else {
        None
    };
    let mut sender = connect(&target, tls).await?;
    let req = build_req(&method, &target.uri, &target.authority, &headers, body)?;
    let resp = sender.send_request(req).await.map_err(|e| e.to_string())?;
    collect_resp(resp).await
}

/// A parsed request URL.
struct Target {
    uri: Uri,
    host: String,
    /// `host:port`, to connect to and for the `host` header.
    authority: String,
    tls: bool,
}

impl Target {
    /// Parse and validate the request URL: `http://` or `https://`, with a host.
    fn parse(url: &str) -> Result<Self, String> {
        let uri: Uri = url
            .parse()
            .map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;
        let (tls, default_port) = match uri.scheme_str() {
            Some("http") => (false, 80),
            Some("https") => (true, 443),
            _ => return Err("only http:// and https:// URLs are supported".into()),
        };
        let host = uri
            .host()
            .ok_or_else(|| "URL has no host".to_string())?
            .to_owned();
        let port = uri.port_u16().unwrap_or(default_port);
        let authority = format!("{host}:{port}");
        Ok(Self {
            uri,
            host,
            authority,
            tls,
        })
    }
}

/// The TLS client settings: the platform verifier, or just the certificates of `ca_pem`.
fn tls_connector(ca_pem: Option<&str>) -> Result<TlsConnector, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls: {e}"))?;
    let config = match ca_pem {
        None => {
            use rustls_platform_verifier::BuilderVerifierExt;
            builder
                .with_platform_verifier()
                .map_err(|e| format!("tls: the platform certificate verifier is unavailable: {e}"))?
                .with_no_client_auth()
        }
        Some(pem) => {
            let mut roots = RootCertStore::empty();
            for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
                let cert = cert.map_err(|e| format!("ca_pem is not valid PEM: {e}"))?;
                roots
                    .add(cert)
                    .map_err(|e| format!("ca_pem holds an unusable certificate: {e}"))?;
            }
            if roots.is_empty() {
                return Err("ca_pem holds no certificate".into());
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Open a connection to the target (over TLS for `https`) and complete the HTTP/1.1 handshake,
/// spawning the connection driver. Returns the request sender.
async fn connect(
    target: &Target,
    tls: Option<TlsConnector>,
) -> Result<hyper::client::conn::http1::SendRequest<Full<Bytes>>, String> {
    let stream = TcpStream::connect(&target.authority)
        .await
        .map_err(|e| e.to_string())?;
    match tls {
        None => handshake(TokioIo::new(stream)).await,
        Some(connector) => {
            // an IPv6 literal arrives bracketed (`[::1]`); the certificate name is the bare address
            let host = target.host.trim_start_matches('[').trim_end_matches(']');
            let name = ServerName::try_from(host.to_owned()).map_err(|e| format!("tls: {e}"))?;
            let stream = connector
                .connect(name, stream)
                .await
                .map_err(|e| format!("tls: {e}"))?;
            handshake(TokioIo::new(stream)).await
        }
    }
}

async fn handshake<T>(
    io: TokioIo<T>,
) -> Result<hyper::client::conn::http1::SendRequest<Full<Bytes>>, String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(sender)
}

/// Build the outgoing request: method, path + query, the `host` header, every
/// caller header, and the body.
fn build_req(
    method: &str,
    uri: &Uri,
    authority: &str,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
) -> Result<HyperRequest<Full<Bytes>>, String> {
    let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let mut builder = HyperRequest::builder()
        .method(method)
        .uri(path_and_query)
        .header("host", authority);
    for (k, v) in headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let bytes = body.map(Bytes::from).unwrap_or_default();
    builder.body(Full::new(bytes)).map_err(|e| e.to_string())
}

/// Read the response status, headers, and full body into a `ClientResponse`.
async fn collect_resp(
    resp: hyper::Response<hyper::body::Incoming>,
) -> Result<ClientResponse, String> {
    let status = resp.status().as_u16();
    let headers = super::collect_headers(resp.headers());
    let collected = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| e.to_string())?;
    Ok(ClientResponse {
        status,
        headers,
        body: collected.to_bytes().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mlua::Lua;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;
    use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio_rustls::rustls::{self, ServerConfig};

    use crate::lua::sandbox::make_engine;

    /// A TLS server on a local port with a self-signed certificate for `localhost`: answers every request on
    /// every connection with `hello`. Returns the port and the certificate (PEM).
    async fn tls_server() -> (u16, String) {
        tls_server_on("127.0.0.1:0", "localhost").await
    }

    /// [`tls_server`] bound to `bind`, with a certificate whose subject alternative name is `name` (a DNS
    /// name, or an IP address, which rcgen turns into an IP SAN).
    async fn tls_server_on(bind: &str, name: &str) -> (u16, String) {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![name.to_owned()]).expect("certificate");
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
        let listener = TcpListener::bind(bind).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    // a client that refuses the certificate fails the handshake: nothing to serve then
                    let Ok(mut tls) = acceptor.accept(stream).await else {
                        return;
                    };
                    let mut seen = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        match tls.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => seen.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&seen).into_owned();
                    let body = format!("hello {}", head.lines().next().unwrap_or_default());
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = tls.write_all(reply.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        (port, cert.pem())
    }

    async fn run(lua: &Lua, source: &str) -> (Option<String>, Option<String>) {
        lua.load(source).eval_async().await.expect("script")
    }

    #[tokio::test]
    async fn https_works_against_a_server_whose_certificate_is_given_as_ca_pem() {
        let (port, pem) = tls_server().await;
        let lua = make_engine().expect("engine");
        lua.globals().set("PEM", pem).expect("pem");
        let (body, err) = run(
            &lua,
            &format!(
                r#"local hyper = require("vnd:hyper")
                   local r, e = hyper.get("https://localhost:{port}/some/path", {{ ca_pem = PEM }})
                   if not r then return nil, e end
                   return r.status .. " " .. r.body"#
            ),
        )
        .await;
        assert_eq!(err, None);
        assert_eq!(body.as_deref(), Some("200 hello GET /some/path HTTP/1.1"));

        // post and request carry the option too
        let (body, err) = run(
            &lua,
            &format!(
                r#"local hyper = require("vnd:hyper")
                   local r, e = hyper.post {{ url = "https://localhost:{port}/p", body = "x", ca_pem = PEM }}
                   if not r then return nil, e end
                   local q, f = hyper.request {{ method = "PUT", url = "https://localhost:{port}/q", ca_pem = PEM }}
                   if not q then return nil, f end
                   return r.body .. " | " .. q.body"#
            ),
        )
        .await;
        assert_eq!(err, None);
        assert_eq!(
            body.as_deref(),
            Some("hello POST /p HTTP/1.1 | hello PUT /q HTTP/1.1")
        );
    }

    /// An IPv6 literal URL: the URL's host keeps its brackets (`[::1]`), the certificate names the bare address.
    #[tokio::test]
    async fn https_to_an_ipv6_literal_checks_the_ip_san() {
        let (port, pem) = tls_server_on("[::1]:0", "::1").await;
        let lua = make_engine().expect("engine");
        lua.globals().set("PEM", pem).expect("pem");
        let (body, err) = run(
            &lua,
            &format!(
                r#"local r, e = require("vnd:hyper").get("https://[::1]:{port}/v6", {{ ca_pem = PEM }})
                   if not r then return nil, e end
                   return r.body"#
            ),
        )
        .await;
        assert_eq!(err, None);
        assert_eq!(body.as_deref(), Some("hello GET /v6 HTTP/1.1"));
    }

    #[tokio::test]
    async fn a_certificate_the_platform_does_not_trust_is_refused_without_ca_pem() {
        let (port, _pem) = tls_server().await;
        let lua = make_engine().expect("engine");
        let (body, err) = run(
            &lua,
            &format!(r#"return require("vnd:hyper").get("https://localhost:{port}/")"#),
        )
        .await;
        assert_eq!(
            body, None,
            "the self-signed certificate must not be accepted"
        );
        let err = err.expect("an error");
        assert!(err.contains("hyper.get") && err.contains("tls"), "{err}");
    }

    #[tokio::test]
    async fn a_ca_pem_that_is_not_the_servers_certificate_is_refused() {
        let (port, _pem) = tls_server().await;
        let (_, other) = tls_server().await;
        let lua = make_engine().expect("engine");
        lua.globals().set("PEM", other).expect("pem");
        let (body, err) = run(
            &lua,
            &format!(
                r#"return require("vnd:hyper").get("https://localhost:{port}/", {{ ca_pem = PEM }})"#
            ),
        )
        .await;
        assert_eq!(body, None);
        assert!(err.expect("an error").contains("tls"));
    }

    #[tokio::test]
    async fn bad_ca_pem_and_other_schemes_are_errors_not_panics() {
        let lua = make_engine().expect("engine");
        for (script, needle) in [
            (
                r#"return require("vnd:hyper").get("https://localhost:1/", { ca_pem = "nonsense" })"#,
                "ca_pem holds no certificate",
            ),
            (
                r#"return require("vnd:hyper").get("https://localhost:1/", { ca_pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n" })"#,
                "ca_pem holds an unusable certificate",
            ),
            (
                r#"return require("vnd:hyper").get("ftp://localhost/")"#,
                "only http:// and https://",
            ),
            (
                r#"return require("vnd:hyper").get("https://localhost:1/", { ca_pem = {} })"#,
                "ca_pem must be a string",
            ),
            (
                r#"return require("vnd:hyper").get("http:///no-host")"#,
                "hyper.get:",
            ),
        ] {
            let (body, err) = run(&lua, script).await;
            assert_eq!(body, None, "{script}");
            assert!(err.expect("an error").contains(needle), "{script}");
        }
    }

    /// A real HTTPS request to a public host, verified by the platform trust store (no `ca_pem`). It needs
    /// network access (it is not ignored); offline it fails with a `tls` or connect error.
    #[tokio::test]
    async fn https_request_to_a_public_host_is_verified_by_the_platform() {
        let lua = make_engine().expect("engine");
        let (summary, err) = run(
            &lua,
            r#"local r, e = require("vnd:hyper").get("https://example.com/")
               if not r then return nil, e end
               return r.status .. " " .. tostring(r.body:find("Example Domain", 1, true) ~= nil)"#,
        )
        .await;
        assert_eq!(err, None, "request failed: {err:?}");
        assert_eq!(summary.as_deref(), Some("200 true"));
    }
}
