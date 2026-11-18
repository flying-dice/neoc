//! `vnd:hyper` client path — the `get`/`post`/`request` entry points and the
//! one-shot HTTP/1.1 request execution behind them. The server and request
//! halves live in `super::server` / `super`.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request as HyperRequest, Uri};
use hyper_util::rt::TokioIo;
use mlua::{Lua, Table, Value};
use tokio::net::TcpStream;
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};

#[instrument(name = "hyper.get", target = "lua::lib::hyper", skip_all)]
pub(super) async fn get(lua: Lua, url: String) -> mlua::Result<(Value, Value)> {
    perform(&lua, "hyper.get", "GET".into(), url, None, Vec::new()).await
}

#[instrument(name = "hyper.post", target = "lua::lib::hyper", skip_all)]
pub(super) async fn post(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let (url, body, headers) = match extract_opts(&opts) {
        Ok(parts) => parts,
        Err(e) => return from_err(&lua, "hyper.post", e),
    };
    perform(&lua, "hyper.post", "POST".into(), url, body, headers).await
}

#[instrument(name = "hyper.request", target = "lua::lib::hyper", skip_all)]
pub(super) async fn request(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let method: String = match opts.get("method") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    let (url, body, headers) = match extract_opts(&opts) {
        Ok(parts) => parts,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    perform(&lua, "hyper.request", method, url, body, headers).await
}

// ---------------------------------------------------------------------------
// Lua → Rust extraction
// ---------------------------------------------------------------------------

/// Pull the shared `url` / `body` / `headers` fields out of an opts table.
/// Errors stringify so each caller can wrap them under its own prefix.
type Opts = (String, Option<Vec<u8>>, Vec<(String, String)>);

fn extract_opts(opts: &Table) -> Result<Opts, String> {
    let url: String = opts.get("url").map_err(|e| e.to_string())?;
    let body = body_from_opts(opts);
    let headers = headers_from_opts(opts).map_err(|e| e.to_string())?;
    Ok((url, body, headers))
}

fn body_from_opts(opts: &Table) -> Option<Vec<u8>> {
    let s: Option<mlua::LuaString> = opts.get("body").ok();
    s.map(|v| v.as_bytes().to_vec())
}

fn headers_from_opts(opts: &Table) -> mlua::Result<Vec<(String, String)>> {
    let h: Option<Table> = opts.get("headers").ok();
    match h {
        None => Ok(Vec::new()),
        Some(t) => t.pairs::<String, String>().collect(),
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
) -> mlua::Result<(Value, Value)> {
    match do_request(method, url, body, headers).await {
        Ok(resp) => {
            let t = lua.create_table()?;
            t.set("status", i64::from(resp.status))?;
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
) -> Result<ClientResponse, String> {
    let (uri, authority) = uri_parts(&url)?;
    let mut sender = connect(&authority).await?;
    let req = build_req(&method, &uri, &authority, &headers, body)?;
    let resp = sender.send_request(req).await.map_err(|e| e.to_string())?;
    collect_resp(resp).await
}

/// Parse and validate the request URL, returning the parsed `Uri` plus its
/// `host:port` authority. Only `http://` is supported.
fn uri_parts(url: &str) -> Result<(Uri, String), String> {
    let uri: Uri = url.parse().map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;
    if uri.scheme_str() != Some("http") {
        return Err("only http:// URLs are supported".into());
    }
    let host = uri.host().ok_or_else(|| "URL has no host".to_string())?;
    let port = uri.port_u16().unwrap_or(80);
    let authority = format!("{host}:{port}");
    Ok((uri, authority))
}

/// Open a TCP connection to `authority` and complete the HTTP/1.1 handshake,
/// spawning the connection driver. Returns the request sender.
async fn connect(authority: &str) -> Result<hyper::client::conn::http1::SendRequest<Full<Bytes>>, String> {
    let stream = TcpStream::connect(authority).await.map_err(|e| e.to_string())?;
    let io = TokioIo::new(stream);
    let (sender, conn) = hyper::client::conn::http1::handshake(io).await.map_err(|e| e.to_string())?;
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
    let path_and_query = uri.path_and_query().map_or("/", hyper::http::uri::PathAndQuery::as_str);
    let mut builder = HyperRequest::builder().method(method).uri(path_and_query).header("host", authority);
    for (k, v) in headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let bytes = body.map(Bytes::from).unwrap_or_default();
    builder.body(Full::new(bytes)).map_err(|e| e.to_string())
}

/// Read the response status, headers, and full body into a `ClientResponse`.
async fn collect_resp(resp: hyper::Response<hyper::body::Incoming>) -> Result<ClientResponse, String> {
    let status = resp.status().as_u16();
    let headers = super::collect_headers(resp.headers());
    let collected = resp.into_body().collect().await.map_err(|e| e.to_string())?;
    Ok(ClientResponse { status, headers, body: collected.to_bytes().to_vec() })
}
