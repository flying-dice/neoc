#![allow(dead_code)] // each test binary uses some of these
//! Helpers the lifetime and signal tests share.

/// An address nothing is bound to.
pub fn free_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

/// The whole response of a `GET path`, or `None` when nothing answers at `addr`.
pub async fn http_get(addr: &str, path: &str) -> Option<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .ok()?;
    let mut out = String::new();
    stream.read_to_string(&mut out).await.ok()?;
    Some(out)
}

/// Lua source that binds `addr` and serves `200 ok` on it with `vnd:hyper`, keeping the server in `_G.server`.
pub fn serve_ok(addr: &str) -> String {
    format!(
        r#"local listener = require("std:net").TcpListener.bind({addr:?})
           _G.server = require("vnd:hyper").serve(listener, function() return {{ status = 200, body = "ok" }} end)"#
    )
}

/// A directory with one file, `a.txt`, to serve with `serve_static`; the glob that matches it is `<dir>/*.txt`.
pub fn static_site(name: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "static a").unwrap();
    let pattern = format!("{}/*.txt", dir.display());
    (dir, pattern)
}

/// Run a Lua `serve_static` of `pattern` on `addr` as a task of its own; it resolves to `"ended"` when it returns.
pub fn start_static(
    lua: &mlua::Lua,
    pattern: &str,
    addr: &str,
) -> tokio::task::JoinHandle<mlua::Result<String>> {
    let (host, port) = addr.split_once(':').unwrap();
    let source = format!(
        "require('lib:http').serve_static({pattern:?}, {{ host = {host:?}, port = {port} }}) return 'ended'"
    );
    let lua = lua.clone();
    tokio::spawn(async move { lua.load(source).eval_async::<String>().await })
}

/// Wait for a spawned script to finish within five seconds and give its result.
pub async fn ends<T: Send + std::fmt::Debug + 'static>(
    task: tokio::task::JoinHandle<mlua::Result<T>>,
    what: &str,
) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap_or_else(|_| panic!("{what} did not end"))
        .unwrap()
        .unwrap()
}
