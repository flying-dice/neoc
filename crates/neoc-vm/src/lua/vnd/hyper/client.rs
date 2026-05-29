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

#[instrument(name = "hyper.get", target = "luau::lib::hyper", skip_all)]
pub(super) async fn get(lua: Lua, url: String) -> mlua::Result<(Value, Value)> {
    perform(&lua, "hyper.get", "GET".into(), url, None, Vec::new()).await
}

// TODO: clean-code - 0.75 - DRY: post/request repeat the same opts.get+from_err pattern; factor into an `extract_opts` helper returning (method,url,body,headers) or Err.
#[instrument(name = "hyper.post", target = "luau::lib::hyper", skip_all)]
pub(super) async fn post(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let url: String = match opts.get("url") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.post", e),
    };
    let body = body_from_opts(&opts);
    let headers = match headers_from_opts(&opts) {
        Ok(h) => h,
        Err(e) => return from_err(&lua, "hyper.post", e),
    };
    perform(&lua, "hyper.post", "POST".into(), url, body, headers).await
}

#[instrument(name = "hyper.request", target = "luau::lib::hyper", skip_all)]
pub(super) async fn request(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let method: String = match opts.get("method") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    let url: String = match opts.get("url") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    let body = body_from_opts(&opts);
    let headers = match headers_from_opts(&opts) {
        Ok(h) => h,
        Err(e) => return from_err(&lua, "hyper.request", e),
    };
    perform(&lua, "hyper.request", method, url, body, headers).await
}

// ---------------------------------------------------------------------------
// Lua → Rust extraction
// ---------------------------------------------------------------------------

fn body_from_opts(opts: &Table) -> Option<Vec<u8>> {
    let s: Option<mlua::String> = opts.get("body").ok();
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

// TODO: clean-code - 0.7 - SRP: do_request does URI parse, scheme check, connect, handshake, builder, send, collect — split into uri_parts/connect/build_req/collect_resp.
async fn do_request(
    method: String,
    url: String,
    body: Option<Vec<u8>>,
    headers: Vec<(String, String)>,
) -> Result<ClientResponse, String> {
    let uri: Uri = url
        .parse()
        .map_err(|e: hyper::http::uri::InvalidUri| e.to_string())?;

    if uri.scheme_str() != Some("http") {
        return Err("only http:// URLs are supported".into());
    }

    let host = uri.host().ok_or_else(|| "URL has no host".to_string())?;
    let port = uri.port_u16().unwrap_or(80);
    let authority = format!("{host}:{port}");

    let stream = TcpStream::connect(&authority)
        .await
        .map_err(|e| e.to_string())?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");

    let mut builder = HyperRequest::builder()
        .method(method.as_str())
        .uri(path_and_query)
        .header("host", authority.as_str());

    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    let bytes = body.map(Bytes::from).unwrap_or_default();
    let req = builder.body(Full::new(bytes)).map_err(|e| e.to_string())?;

    let resp = sender.send_request(req).await.map_err(|e| e.to_string())?;

    let status = resp.status().as_u16();
    // TODO: clean-code - 0.6 - DRY: identical header-to-Vec<(String,String)> conversion appears in invoke_lua_handler; extract `collect_headers(&HeaderMap)`.
    let resp_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();

    let collected = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| e.to_string())?;
    let body = collected.to_bytes().to_vec();

    Ok(ClientResponse {
        status,
        headers: resp_headers,
        body,
    })
}
