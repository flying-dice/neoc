//! `lib:http`: a glob-scoped HTTP/1.1 static file server.
//!
//! ```lua
//! local http = require("lib:http")
//! -- serves ./screenshots/**/*.png with an index page at /, until Ctrl-C
//! assert(http.serve_static("screenshots/**/*.png", { host = "127.0.0.1", port = 8080 }))
//! ```
//!
//! The files are the ones the pattern matches when the server starts. `/` is an index of them; a file is served
//! under its path below the pattern's fixed prefix (`screenshots/a/b.png` at `/a/b.png`). Only `GET` and `HEAD`
//! are answered; a path with `..` is refused, and an encoded slash stays encoded, so no request reaches a file the
//! pattern did not match.
//!
//! The API's rules:
//! - A server that cannot start returns `(nil, err)` with a typed `ServeError` (`invalid_glob`, `walk`, `bind` with its
//!   `cause`). A server that ran and was stopped returns `true`.
//! - An option that is misspelt or of the wrong type is refused as a wrong argument.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use globset::GlobSetBuilder;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use walkdir::WalkDir;

const MAX_HEADER_LINE: u64 = 8192;
const MAX_HEADERS: usize = 100;
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

struct StaticFile {
    abs_path: PathBuf,
    size: u64,
    modified: SystemTime,
    mime: &'static str,
}

type FileMap = HashMap<String, StaticFile>;

/// Why the files of a pattern could not be listed.
#[derive(Debug)]
enum GlobError {
    /// The pattern is not a glob.
    Pattern(String),
    /// A directory under the pattern's root could not be read.
    Walk { path: Option<PathBuf>, reason: String },
}

/// Accept connections and serve them. The connections are tasks of this loop: when the loop ends, because
/// the call that runs it was dropped, they are aborted and their sockets closed, not left to outlive the
/// server that accepted them.
async fn serve_loop(listener: TcpListener, files: Arc<FileMap>, index: Arc<String>) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _peer)) => {
                    let files = files.clone();
                    let index = index.clone();
                    connections.spawn(async move {
                        if let Err(e) = handle_connection(stream, &files, &index).await {
                            tracing::debug!("connection error: {e}");
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("accept error: {e}");
                    // a persistent error (out of descriptors) must not spin
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            // reap the finished ones, so a long-lived server does not collect them
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

/// A fully-decided HTTP response: routing produces one of these, then
/// `handle_connection` writes it (suppressing the body for `HEAD`).
struct Response {
    status: u16,
    reason: &'static str,
    mime: &'static str,
    modified: Option<SystemTime>,
    body: Vec<u8>,
}

impl Response {
    /// A plain-text status response (`400`/`403`/`404`/`405`) whose body is the
    /// reason phrase.
    fn text(status: u16, reason: &'static str) -> Self {
        Response { status, reason, mime: "text/plain", modified: None, body: reason.as_bytes().to_vec() }
    }

    async fn write(
        &self,
        writer: &mut (impl AsyncWriteExt + Unpin),
        send_body: bool,
    ) -> tokio::io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.status,
            self.reason,
            self.mime,
            self.body.len()
        );
        if let Some(mtime) = self.modified {
            head.push_str(&format!("Last-Modified: {}\r\n", fmt_http_date(mtime)));
        }
        head.push_str("\r\n");
        writer.write_all(head.as_bytes()).await?;
        if send_body {
            writer.write_all(&self.body).await?;
        }
        writer.flush().await
    }
}

/// Maps a parsed request to the response it earns: `405` for a non-`GET`/`HEAD`
/// method, the index page for `/`, `403` for a traversal attempt, then the
/// served file or `404`. Only disk I/O can fail, so other arms are infallible.
async fn route(method: &str, path: &str, files: &FileMap, index: &str) -> Result<Response, RequestError> {
    if method != "GET" && method != "HEAD" {
        return Ok(Response::text(405, "Method Not Allowed"));
    }
    if path == "/" {
        return Ok(Response {
            status: 200,
            reason: "OK",
            mime: "text/html; charset=utf-8",
            modified: None,
            body: index.as_bytes().to_vec(),
        });
    }
    let rel = path.trim_start_matches('/');
    if rel.contains("..") {
        return Ok(Response::text(403, "Forbidden"));
    }
    match files.get(rel) {
        Some(f) => Ok(Response {
            status: 200,
            reason: "OK",
            mime: f.mime,
            modified: Some(f.modified),
            body: tokio::fs::read(&f.abs_path).await?,
        }),
        None => Ok(Response::text(404, "Not Found")),
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    files: &FileMap,
    index: &str,
) -> Result<(), RequestError> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // parse: read the request line and the headers, mapping a bad percent-escape to 400.
    let head = tokio::time::timeout(HEADER_TIMEOUT, read_request_head(&mut reader))
        .await
        .map_err(|_| RequestError::Malformed("header read timeout"))?;
    let (method, path) = match head {
        Ok(v) => v,
        Err(RequestError::MalformedPercentEncoding) => {
            Response::text(400, "Bad Request").write(&mut writer, true).await?;
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    // route, then respond.
    let response = route(&method, &path, files, index).await?;
    response.write(&mut writer, method != "HEAD").await?;
    Ok(())
}

/// Read one line bounded to `MAX_HEADER_LINE + 1` bytes. An over-long line is
/// returned without a trailing `\n`, so callers detect truncation by checking
/// `ends_with('\n')`.
async fn read_bounded_line(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> std::io::Result<String> {
    let mut line = String::new();
    let mut limited = (&mut *reader).take(MAX_HEADER_LINE + 1);
    limited.read_line(&mut line).await?;
    Ok(line)
}

async fn read_request_head(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Result<(String, String), RequestError> {
    let request_line = read_bounded_line(reader).await?;
    if !request_line.ends_with('\n') {
        return Err(RequestError::Malformed("request line too long"));
    }
    let (method, path) = parse_request_line(&request_line)?;

    let mut header_count = 0usize;
    loop {
        let header_line = read_bounded_line(reader).await?;
        if header_line == "\r\n" || header_line.is_empty() {
            break;
        }
        if !header_line.ends_with('\n') {
            return Err(RequestError::Malformed("header line too long"));
        }
        header_count += 1;
        if header_count > MAX_HEADERS {
            return Err(RequestError::Malformed("too many headers"));
        }
    }

    Ok((method, path))
}

/// Why parsing or serving a request failed. `handle_connection` answers a
/// `MalformedPercentEncoding` with `400`; everything else drops the connection
/// (logged at debug). A typed enum so the caller matches instead of downcasting.
#[derive(Debug, thiserror::Error)]
enum RequestError {
    /// A `%`-escape in the request path was malformed.
    #[error("malformed percent-encoding")]
    MalformedPercentEncoding,
    /// A size bound was exceeded or the request line was unparseable.
    #[error("{0}")]
    Malformed(&'static str),
    /// The socket read or response write failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> Result<String, RequestError> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if i + 2 >= bytes.len() {
            return Err(RequestError::MalformedPercentEncoding);
        }
        let hi = hex_nibble(bytes[i + 1]).ok_or(RequestError::MalformedPercentEncoding)?;
        let lo = hex_nibble(bytes[i + 2]).ok_or(RequestError::MalformedPercentEncoding)?;
        let byte = (hi << 4) | lo;
        if byte == b'/' {
            // %2F/%2f: keep encoded to block encoded-slash path traversal.
            out.extend_from_slice(&bytes[i..i + 3]);
        } else {
            out.push(byte);
        }
        i += 3;
    }
    String::from_utf8(out).map_err(|_| RequestError::MalformedPercentEncoding)
}

fn parse_request_line(line: &str) -> Result<(String, String), RequestError> {
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(RequestError::Malformed("empty request line"))?.to_uppercase();
    let raw_path = parts.next().ok_or(RequestError::Malformed("missing path"))?.to_string();
    let path_without_qs = raw_path.split('?').next().unwrap_or("/");
    let path = percent_decode(path_without_qs)?;
    Ok((method, path))
}

// ---------------------------------------------------------------------------
// Glob resolution
// ---------------------------------------------------------------------------

fn glob_root(pattern: &str) -> PathBuf {
    let cleaned = pattern.trim_start_matches("./");
    let mut parts: Vec<&str> = Vec::new();
    for seg in cleaned.split('/') {
        if seg.contains(['*', '?', '[', '{']) {
            break;
        }
        parts.push(seg);
    }
    if parts.is_empty() { PathBuf::from(".") } else { PathBuf::from(parts.join("/")) }
}

fn resolve_glob(pattern: &str) -> Result<FileMap, GlobError> {
    let mut builder = GlobSetBuilder::new();
    builder.add(neoc_vm_lib_glob::compile(pattern).map_err(GlobError::Pattern)?);
    let set = builder.build().map_err(|e| GlobError::Pattern(e.kind().to_string()))?;

    let root = glob_root(pattern);
    let mut files: FileMap = HashMap::new();
    let walk =
        |e: walkdir::Error| GlobError::Walk { path: e.path().map(PathBuf::from), reason: e.to_string() };

    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry.map_err(walk)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !set.is_match(path) {
            continue;
        }
        let meta = entry.metadata().map_err(walk)?;
        let rel = path
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

        files.insert(
            rel,
            StaticFile {
                abs_path: path.to_path_buf(),
                size: meta.len(),
                modified: meta.modified().unwrap_or(UNIX_EPOCH),
                mime: mime_for_ext(&ext),
            },
        );
    }

    Ok(files)
}

// ---------------------------------------------------------------------------
// MIME map (inline: a static server's few types, each with the charset a browser needs)
// ---------------------------------------------------------------------------

fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Index page
// ---------------------------------------------------------------------------

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn build_index(files: &FileMap, glob_pattern: &str) -> String {
    let mut entries: Vec<(&str, &StaticFile)> = files.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by_key(|(rel, _)| *rel);

    let rows: String = entries.iter().map(|(rel, f)| render_row(rel, f)).collect();
    render_index_page(&html_escape(glob_pattern), entries.len(), &rows)
}

/// Render one file's table row: an inline thumbnail for images, then a link to
/// the file, its human-readable size, and its modified time.
fn render_row(rel: &str, f: &StaticFile) -> String {
    let rel_escaped = html_escape(rel);
    let size = human_size(f.size);
    let mtime = fmt_http_date(f.modified);
    let preview = if f.mime.starts_with("image/") {
        format!(
            r#"<img src="/{rel_escaped}" loading="lazy" alt="" style="max-height:64px;max-width:128px;vertical-align:middle;margin-right:8px">"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<tr><td>{preview}<a href="/{rel_escaped}">{rel_escaped}</a></td><td>{size}</td><td>{mtime}</td></tr>"#
    )
}

/// Wrap the rendered `rows` in the directory-listing page chrome: document
/// head, inline stylesheet, heading, and table shell.
fn render_index_page(glob_escaped: &str, count: usize, rows: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Index of {glob_escaped}</title>
<style>
:root{{color-scheme:light dark}}
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:system-ui,sans-serif;padding:1.5rem;max-width:960px;margin:auto}}
h1{{font-size:1.1rem;font-weight:600;margin-bottom:1rem;word-break:break-all}}
table{{width:100%;border-collapse:collapse;font-size:.875rem}}
th,td{{text-align:left;padding:.4rem .6rem;border-bottom:1px solid color-mix(in srgb,currentColor 12%,transparent)}}
th{{font-weight:600;white-space:nowrap}}
td:nth-child(2),td:nth-child(3){{white-space:nowrap;color:color-mix(in srgb,currentColor 60%,transparent)}}
a{{color:inherit;text-decoration:none}}
a:hover{{text-decoration:underline}}
</style>
</head>
<body>
<h1>Index of {glob_escaped} <small style="font-weight:normal;color:color-mix(in srgb,currentColor 50%,transparent)">({count} files)</small></h1>
<table>
<thead><tr><th>File</th><th>Size</th><th>Modified</th></tr></thead>
<tbody>{rows}</tbody>
</table>
</body>
</html>
"#
    )
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ---------------------------------------------------------------------------
// HTTP date formatting (RFC 7231 §7.1.1.1)
// ---------------------------------------------------------------------------

fn fmt_http_date(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs();
    let days = secs / 86400;
    // 1970-01-01 was a Thursday (index 4 in Sun=0 week).
    let dow = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][((days + 4) % 7) as usize];
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = days_to_ymd(days);
    let h = ((secs / 3600) % 24) as u32;
    let min = ((secs / 60) % 60) as u32;
    let sec = (secs % 60) as u32;
    format!("{dow}, {d:02} {} {y} {h:02}:{min:02}:{sec:02} GMT", months[(m - 1) as usize])
}

fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Gregorian calendar via Julian Day Number algorithm.
    let jdn = days + 2_440_588u64; // JDN of 1970-01-01
    let f = jdn + 1401 + (((4 * jdn + 274_277) / 146_097) * 3) / 4 - 38;
    let e = 4 * f + 3;
    let g = (e % 1461) / 4;
    let h = 5 * g + 2;
    let day = (h % 153) / 5 + 1;
    let month = (h / 153 + 2) % 12 + 1;
    let year = e / 1461 - 4716 + (14 - month) / 12;
    (year as u32, month as u32, day as u32)
}

#[mlua_ir::module("lib:http")]
pub mod http {
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{LuaError, Options, Shape};
    use serde::{Deserialize, Serialize};
    use tokio::net::TcpListener;

    use super::{GlobError, build_index, resolve_glob, serve_loop};
    use neoc_vm_core::lifecycle::Life;

    /// Where `serve_static` listens.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct ServeOptions {
        /// The address to bind: `"127.0.0.1"` for this machine only.
        pub host: String,
        /// The port to bind.
        pub port: u16,
    }

    impl Default for ServeOptions {
        fn default() -> Self {
            Self { host: "0.0.0.0".to_owned(), port: 8080 }
        }
    }

    /// Why the operating system refused an address.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum BindCause {
        /// Another socket is bound to the address.
        AddrInUse,
        /// The address is not one of this machine's.
        AddrNotAvailable,
        /// The process may not bind the address (a port below 1024, say).
        PermissionDenied,
        /// Any other failure (a host name that does not resolve, say).
        Other,
    }

    /// Why a static server did not start.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ServeError {
        /// The pattern is not a glob.
        #[error("{pattern}: {reason}")]
        InvalidGlob {
            /// The pattern given.
            pattern: String,
            /// What is wrong with it.
            reason: String,
        },
        /// The files under the pattern's fixed prefix could not be listed: the directory is not there, or one
        /// under it cannot be read.
        #[error("{reason}")]
        Walk {
            /// The path that could not be read, when the failure is about one.
            path: Option<String>,
            /// What the operating system said.
            reason: String,
        },
        /// The address could not be bound.
        #[error("bind {addr}: {reason}")]
        Bind {
            /// The address asked for, as `host:port`.
            addr: String,
            /// Why, as a script branches on it.
            cause: BindCause,
            /// What the operating system said.
            reason: String,
        },
    }

    impl ServeError {
        fn of_glob(error: GlobError, pattern: String) -> Self {
            match error {
                GlobError::Pattern(reason) => ServeError::InvalidGlob { pattern, reason },
                GlobError::Walk { path, reason } => {
                    ServeError::Walk { path: path.map(|p| p.to_string_lossy().into_owned()), reason }
                }
            }
        }

        fn of_bind(error: &std::io::Error, addr: String) -> Self {
            use std::io::ErrorKind as Kind;
            let cause = match error.kind() {
                Kind::AddrInUse => BindCause::AddrInUse,
                Kind::AddrNotAvailable => BindCause::AddrNotAvailable,
                Kind::PermissionDenied => BindCause::PermissionDenied,
                _ => BindCause::Other,
            };
            ServeError::Bind { addr, cause, reason: error.to_string() }
        }
    }

    /// Serve the files matching `glob_pattern` over HTTP/1.1, with an index of them at `/`. The call blocks while
    /// the server runs: until Ctrl-C, or until an embedder stops the script's background work or shuts it down;
    /// then it returns `true`. A server that cannot start returns the error instead.
    ///
    /// ```luau
    /// local served, err = http.serve_static("screenshots/**/*.png", { host = "127.0.0.1", port = 8080 })
    /// if not served and err ~= nil and err.kind == "bind" and err.cause == "addr_in_use" then
    ///     print("port 8080 is taken")
    /// end
    /// ```
    pub async fn serve_static(lua: Lua, glob_pattern: String, opts: ServeOptions) -> Result<(), ServeError> {
        let files = match resolve_glob(&glob_pattern) {
            Ok(files) => Arc::new(files),
            Err(e) => return Err(ServeError::of_glob(e, glob_pattern)),
        };
        let index = Arc::new(build_index(&files, &glob_pattern));

        let addr = format!("{}:{}", opts.host, opts.port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(e) => return Err(ServeError::of_bind(&e, addr)),
        };

        tracing::info!("http static server on http://{addr} ({} files)", files.len());

        // Runs until Ctrl-C (a fresh SIGINT, as ever: this call listens for it itself), until the script's registry
        // stops its background work or shuts down (an embedder unloading or replacing the script) or until the
        // call is dropped. In each case the listener and every open connection go with it (the connections
        // belong to the serve loop, see `serve_loop`).
        let life = Life::of(&lua);
        let stopped = life.stopped();
        tokio::select! {
            () = serve_loop(listener, files, index) => {},
            () = stopped => {},
            () = life.closed() => {},
            result = tokio::signal::ctrl_c() => {
                if let Err(e) = result {
                    tracing::error!("signal error: {e}");
                }
            },
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpStream;

    use super::*;
    use neoc_vm_core::lifecycle::Life;

    /// The connections of a static server belong to its serve loop: when the loop is dropped (the call running
    /// it was cancelled) an open connection is closed with it, not left running.
    #[tokio::test]
    async fn dropping_the_serve_loop_closes_its_connections_and_its_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve_loop(listener, Arc::new(HashMap::new()), Arc::new(String::new())));
        // a client that connects and says nothing: the connection waits for a request line
        let mut idle = TcpStream::connect(addr).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        server.abort();
        let _ = server.await;
        let mut buf = [0_u8; 8];
        let read = tokio::time::timeout(Duration::from_secs(2), idle.read(&mut buf))
            .await
            .expect("the connection was closed with the loop");
        assert!(matches!(read, Ok(0) | Err(_)), "{read:?}");
        assert!(TcpStream::connect(addr).await.is_err(), "the listener is closed");
    }

    #[test]
    fn mime_known_types() {
        assert_eq!(mime_for_ext("png"), "image/png");
        assert_eq!(mime_for_ext("jpg"), "image/jpeg");
        assert_eq!(mime_for_ext("jpeg"), "image/jpeg");
        assert_eq!(mime_for_ext("html"), "text/html; charset=utf-8");
        assert_eq!(mime_for_ext("js"), "application/javascript");
        assert_eq!(mime_for_ext("json"), "application/json");
        assert_eq!(mime_for_ext("wasm"), "application/wasm");
    }

    #[test]
    fn mime_unknown_falls_back() {
        assert_eq!(mime_for_ext("xyz"), "application/octet-stream");
        assert_eq!(mime_for_ext(""), "application/octet-stream");
    }

    #[test]
    fn days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-15 = 19737 days since epoch
        let (y, m, d) = days_to_ymd(19737);
        assert_eq!((y, m, d), (2024, 1, 15));
    }

    #[test]
    fn fmt_http_date_epoch() {
        let t = UNIX_EPOCH;
        assert_eq!(fmt_http_date(t), "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn fmt_http_date_known() {
        // 2024-01-15 11:30:45 UTC = 19737*86400 + 11*3600 + 30*60 + 45 = 1705318245
        let t = UNIX_EPOCH + Duration::from_secs(1_705_318_245);
        assert_eq!(fmt_http_date(t), "Mon, 15 Jan 2024 11:30:45 GMT");
    }

    #[test]
    fn human_size_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
    }

    #[test]
    fn human_size_kib() {
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(2048), "2.0 KiB");
    }

    #[test]
    fn human_size_mib() {
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn glob_root_simple() {
        assert_eq!(glob_root("screenshots/**/*.png"), PathBuf::from("screenshots"));
        assert_eq!(glob_root("./**/*.png"), PathBuf::from("."));
        assert_eq!(glob_root("**/*.png"), PathBuf::from("."));
    }

    #[test]
    fn glob_root_nested() {
        assert_eq!(glob_root("assets/images/**/*.png"), PathBuf::from("assets/images"));
    }

    #[test]
    fn parse_request_line_get() {
        let (method, path) = parse_request_line("GET / HTTP/1.1\r\n").unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/");
    }

    #[test]
    fn parse_request_line_strips_query() {
        let (method, path) = parse_request_line("GET /foo.png?v=1 HTTP/1.1\r\n").unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/foo.png");
    }

    #[test]
    fn build_index_empty() {
        let files: FileMap = HashMap::new();
        let html = build_index(&files, "*.png");
        assert!(html.contains("0 files"));
        assert!(html.contains("Index of *.png"));
    }

    #[test]
    fn build_index_has_links() {
        let mut files: FileMap = HashMap::new();
        files.insert(
            "shot.png".to_string(),
            StaticFile {
                abs_path: PathBuf::from("/tmp/shot.png"),
                size: 1024,
                modified: UNIX_EPOCH,
                mime: "image/png",
            },
        );
        let html = build_index(&files, "*.png");
        assert!(html.contains(r#"href="/shot.png""#));
        assert!(html.contains("1.0 KiB"));
    }

    /// A temp directory's path with forward slashes, as a glob pattern is written.
    fn pattern_in(dir: &tempfile::TempDir, rest: &str) -> String {
        format!("{}/{rest}", dir.path().display().to_string().replace('\\', "/"))
    }

    #[test]
    fn resolve_glob_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let files = resolve_glob(&pattern_in(&dir, "**/*.png")).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn resolve_glob_finds_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), b"PNG").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"TXT").unwrap();
        let files = resolve_glob(&pattern_in(&dir, "**/*.png")).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files.contains_key("a.png"));
    }

    #[test]
    fn resolve_glob_says_what_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(resolve_glob(&pattern_in(&dir, "a[")), Err(GlobError::Pattern(_))));
        let missing = resolve_glob(&pattern_in(&dir, "nope/**/*.png"));
        assert!(matches!(missing, Err(GlobError::Walk { path: Some(_), .. })), "{:?}", missing.err());
    }

    #[test]
    fn percent_decode_space() {
        assert_eq!(percent_decode("/my%20file.png").unwrap(), "/my file.png");
    }

    #[test]
    fn percent_decode_non_ascii_utf8() {
        // é = U+00E9 = 0xC3 0xA9 in UTF-8
        assert_eq!(percent_decode("/%C3%A9.png").unwrap(), "/\u{00e9}.png");
    }

    #[test]
    fn percent_decode_already_decoded() {
        assert_eq!(percent_decode("/plain.png").unwrap(), "/plain.png");
    }

    #[test]
    fn percent_decode_malformed_non_hex() {
        assert!(percent_decode("/%GG").is_err());
    }

    #[test]
    fn percent_decode_malformed_truncated() {
        assert!(percent_decode("/%2").is_err());
        assert!(percent_decode("/%").is_err());
    }

    #[test]
    fn percent_decode_slash_stays_encoded() {
        assert_eq!(percent_decode("/my%2Fetc/passwd").unwrap(), "/my%2Fetc/passwd",);
        assert_eq!(percent_decode("/my%2fetc/passwd").unwrap(), "/my%2fetc/passwd",);
    }

    #[test]
    fn percent_decode_invalid_utf8_sequence() {
        // 0x80 alone is not valid UTF-8
        assert!(percent_decode("/%80").is_err());
    }

    #[test]
    fn parse_request_line_decodes_space() {
        let (_, path) = parse_request_line("GET /my%20file.png HTTP/1.1\r\n").unwrap();
        assert_eq!(path, "/my file.png");
    }

    #[test]
    fn parse_request_line_malformed_percent_is_err() {
        let result = parse_request_line("GET /%GG HTTP/1.1\r\n");
        assert!(matches!(result.unwrap_err(), RequestError::MalformedPercentEncoding));
    }

    // -----------------------------------------------------------------------
    // the server, over a socket
    // -----------------------------------------------------------------------

    /// Send `request` to `addr` and read the whole answer (the server closes after one).
    async fn ask(addr: &str, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut answer = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut answer))
            .await
            .expect("the server answered and closed");
        String::from_utf8_lossy(&answer).into_owned()
    }

    fn status(answer: &str) -> &str {
        answer.lines().next().unwrap_or_default()
    }

    fn body(answer: &str) -> &str {
        answer.split_once("\r\n\r\n").map_or("", |(_, body)| body)
    }

    /// A directory with two files, and the files and the index `serve_static` would serve for it.
    fn site() -> (tempfile::TempDir, Arc<FileMap>, Arc<String>) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("hello world.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("sub").join("page.html"), b"<p>page</p>").unwrap();
        std::fs::write(dir.path().join("secret.key"), b"secret").unwrap();
        let pattern = pattern_in(&dir, "**/*.{txt,html}");
        let files = Arc::new(resolve_glob(&pattern).unwrap());
        let index = Arc::new(build_index(&files, &pattern));
        (dir, files, index)
    }

    #[tokio::test]
    async fn a_request_is_routed_to_the_index_a_file_or_a_refusal() {
        let (_dir, files, index) = site();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(serve_loop(listener, files, index));

        let page = ask(&addr, "GET / HTTP/1.1\r\n\r\n").await;
        assert_eq!(status(&page), "HTTP/1.1 200 OK");
        assert!(page.contains("Content-Type: text/html; charset=utf-8\r\n"), "{page}");
        assert!(page.contains("(2 files)") && page.contains(r#"href="/sub/page.html""#), "{page}");
        assert!(!page.contains("secret.key"), "a file the pattern does not match is not listed");

        // a file, by its path under the pattern's root; the query is ignored and an escape is decoded
        let file = ask(&addr, "GET /hello%20world.txt?v=1 HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert_eq!(status(&file), "HTTP/1.1 200 OK");
        assert!(file.contains("Content-Type: text/plain; charset=utf-8\r\n"), "{file}");
        assert!(file.contains("Content-Length: 5\r\n") && file.contains("Connection: close\r\n"), "{file}");
        assert!(file.contains("Last-Modified: ") && file.contains(" GMT\r\n"), "{file}");
        assert_eq!(body(&file), "hello");
        assert_eq!(body(&ask(&addr, "get /sub/page.html HTTP/1.1\r\n\r\n").await), "<p>page</p>");

        // HEAD: the headers of the GET, and no body
        let head = ask(&addr, "HEAD /hello%20world.txt HTTP/1.1\r\n\r\n").await;
        assert_eq!(status(&head), "HTTP/1.1 200 OK");
        assert!(head.contains("Content-Length: 5\r\n"), "{head}");
        assert_eq!(body(&head), "");

        // what the pattern did not match is not there, whatever is on the disk
        let missing = ask(&addr, "GET /secret.key HTTP/1.1\r\n\r\n").await;
        assert_eq!((status(&missing), body(&missing)), ("HTTP/1.1 404 Not Found", "Not Found"));
        let traversal = ask(&addr, "GET /sub/../secret.key HTTP/1.1\r\n\r\n").await;
        assert_eq!((status(&traversal), body(&traversal)), ("HTTP/1.1 403 Forbidden", "Forbidden"));
        // an encoded slash stays encoded, so it names no file
        assert_eq!(
            status(&ask(&addr, "GET /sub%2Fpage.html HTTP/1.1\r\n\r\n").await),
            "HTTP/1.1 404 Not Found"
        );
        assert_eq!(
            status(&ask(&addr, "POST /hello%20world.txt HTTP/1.1\r\n\r\n").await),
            "HTTP/1.1 405 Method Not Allowed"
        );
        assert_eq!(status(&ask(&addr, "GET /%GG HTTP/1.1\r\n\r\n").await), "HTTP/1.1 400 Bad Request");

        // a request line or a header past the bound, or too many headers, drops the connection without an answer
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(9000));
        assert_eq!(ask(&addr, &long).await, "");
        let many = format!("GET / HTTP/1.1\r\n{}\r\n", "x: y\r\n".repeat(101));
        assert_eq!(ask(&addr, &many).await, "");
        server.abort();
    }

    // -----------------------------------------------------------------------
    // `serve_static`, as a script calls it
    // -----------------------------------------------------------------------

    /// A port nothing listens on (it was free a moment ago).
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    /// The call blocks while the server runs, and returns `true` when the engine stops its background work or
    /// shuts down; the port is free again then.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serve_static_serves_until_the_engine_stops_it() {
        for stop in [Life::stop_background, Life::shutdown] {
            let (dir, _, _) = site();
            let life = Life::new();
            let lua = neoc_vm_core::make_engine_in(life.clone()).unwrap();
            let port = free_port();
            let addr = format!("127.0.0.1:{port}");
            lua.globals().set("pattern", pattern_in(&dir, "**/*.txt")).unwrap();
            lua.globals().set("port", port).unwrap();
            let serving = tokio::spawn(
                lua.load(
                    r#"
                    local served, err = require("lib:http").serve_static(pattern, { host = "127.0.0.1", port = port })
                    return served == true and err == nil
                    "#,
                )
                .eval_async::<bool>(),
            );
            // the server is up once it accepts a connection
            let mut answer = String::new();
            for _ in 0..200 {
                if TcpStream::connect(&addr).await.is_ok() {
                    answer = ask(&addr, "GET /hello%20world.txt HTTP/1.1\r\n\r\n").await;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(body(&answer), "hello");
            assert!(!serving.is_finished(), "the call blocks while the server runs");

            stop(&life);
            let ended = tokio::time::timeout(Duration::from_secs(10), serving).await.expect("the call ended");
            assert!(ended.expect("task").expect("script"), "a server that was stopped returns true");
            assert!(neoc_vm_std::net::port_is_free(&addr), "the port is free");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_server_that_cannot_start_returns_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let lua = neoc_vm_core::make_engine().unwrap();
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        lua.globals().set("dir", dir.path().display().to_string().replace('\\', "/")).unwrap();
        lua.globals().set("taken", taken.local_addr().unwrap().port()).unwrap();
        lua.load(
            r#"
            local http = require("lib:http")
            local served, err = http.serve_static(dir .. "/a[")
            assert(served == nil and err.kind == "invalid_glob" and err.pattern == dir .. "/a[", tostring(err))
            assert(err.op == "lib:http.serve_static" and err.message == err.pattern .. ": " .. err.reason)

            served, err = http.serve_static(dir .. "/nope/**/*.png")
            assert(served == nil and err.kind == "walk" and type(err.path) == "string", tostring(err))
            assert(err.message == err.reason)

            served, err = http.serve_static(dir .. "/**/*.png", { host = "127.0.0.1", port = taken })
            assert(served == nil and err.kind == "bind" and err.cause == "addr_in_use", tostring(err))
            assert(err.addr == "127.0.0.1:" .. taken)
            assert(tostring(err) == "lib:http.serve_static: bind: bind " .. err.addr .. ": " .. err.reason)

            -- an option that is misspelt or of the wrong type is refused, not ignored
            for _, opts in { { prot = 1 }, { port = "8080" }, { port = 70000 }, { port = -1 }, { host = 1 }, "8080" } do
                local ok, refused = pcall(http.serve_static, dir .. "/**/*.png", opts)
                assert(not ok and tostring(refused):find("lib:http.serve_static: argument #2 'opts'", 1, true), tostring(refused))
            end
            local ok, refused = pcall(http.serve_static)
            assert(not ok and tostring(refused):find("lib:http.serve_static: argument #1 'glob_pattern'", 1, true))
            "#,
        )
        .exec_async()
        .await
        .unwrap();
    }
}
