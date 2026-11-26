#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! End-to-end test for the Sendable concurrency model: the `neoc` binary runs a fixture that uses `thread.spawn` to
//! fan out HTTP workers sharing a listener and a counter, the test hits it with requests, and the shared counter
//! aggregates them. This is the replacement for the old `--workers` subprocess tests.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_neoc");

static PORT_CTR: AtomicU16 = AtomicU16::new(29000);

fn next_port() -> u16 {
    PORT_CTR.fetch_add(1, Ordering::Relaxed)
}

/// The fixture: `script` with its port, as `server.lua` in a temporary directory of its own.
fn write_fixture(port: u16, script: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rendered = script.replace("__PORT__", &port.to_string());
    std::fs::write(dir.path().join("server.lua"), rendered).expect("write server.lua");
    dir
}

struct ServerProcess(Option<Child>);

impl Drop for ServerProcess {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn spawn_server(fixture: &Path) -> ServerProcess {
    let child = Command::new(BIN)
        .arg("run")
        .arg(fixture.join("server.lua"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn neoc");

    ServerProcess(Some(child))
}

fn wait_ready(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let url = format!("http://127.0.0.1:{port}/__readyz");
    while Instant::now() < deadline {
        if blocking_get(&url).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server on port {port} never became reachable");
}

fn blocking_get(url: &str) -> Result<(u16, String), String> {
    blocking_request("GET", url, None)
}

fn blocking_request(method: &str, url: &str, body: Option<&[u8]>) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let url = url.strip_prefix("http://").ok_or("only http://")?;
    let (authority, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };
    let mut stream = TcpStream::connect(authority).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;

    let body_bytes = body.unwrap_or(&[]);
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nContent-Length: {len}\r\n\r\n",
        len = body_bytes.len()
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    if !body_bytes.is_empty() {
        stream.write_all(body_bytes).map_err(|e| e.to_string())?;
    }

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf).to_string();

    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("no status code")?
        .parse()
        .map_err(|_| "bad status code")?;
    let body_start = text.find("\r\n\r\n").ok_or("no header/body split")? + 4;
    Ok((status, text[body_start..].to_string()))
}

fn shutdown(mut server: ServerProcess) {
    let mut child = match server.0.take() {
        Some(c) => c,
        None => return,
    };
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

#[cfg(unix)]
mod libc {
    pub const SIGINT: i32 = 2;
    unsafe extern "C" {
        pub fn kill(pid: i32, sig: i32) -> i32;
    }
}

/// Multi-worker HTTP server using thread.spawn + shared listener.
/// Validates the Sendable concurrency model end-to-end.
#[test]
fn thread_spawn_multi_worker_http() {
    let port = next_port();
    let fixture = write_fixture(
        port,
        r#"
local thread      = require("std:thread")
local TcpListener = require("std:net").TcpListener
local Counter     = require("std:collections").Counter
local hyper       = require("vnd:hyper")

local listener = assert(TcpListener.bind("127.0.0.1:__PORT__"))
local hits     = Counter.new(0)

for i = 1, 4 do
assert(thread.spawn([[
    local hyper  = require("vnd:hyper")
    local shared = require("std:thread").shared()

    local server = assert(hyper.serve(shared.listener, function(req)
        if req.method == "GET" and req.path == "/__readyz" then
            return { status = 200, body = "ok" }
        end
        if req.method == "GET" and req.path == "/" then
            shared.hits:increment()
            return "OK"
        end
        if req.method == "GET" and req.path == "/hits" then
            return { status = 200, body = tostring(shared.hits:get()) }
        end
        return { status = 404 }
    end))
    server:wait()
]], nil, { listener = listener, hits = hits }))
end

require("std:workers").on_shutdown():wait()
"#,
    );

    let server = spawn_server(fixture.path());
    wait_ready(port, Duration::from_secs(10));

    // Hit the server 20 times — requests fan out across 4 workers.
    for _ in 0..20 {
        let (status, body) = blocking_get(&format!("http://127.0.0.1:{port}/")).expect("GET /");
        assert_eq!(status, 200);
        assert_eq!(body, "OK");
    }

    // The shared counter should reflect all 20 hits.
    let (status, body) = blocking_get(&format!("http://127.0.0.1:{port}/hits")).expect("GET /hits");
    assert_eq!(status, 200);
    assert_eq!(
        body, "20",
        "shared counter should aggregate across all workers"
    );

    shutdown(server);
}
