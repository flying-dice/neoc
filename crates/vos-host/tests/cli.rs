#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The `vos-host` binary itself: the banner, `mcp.json`, `pair`, and graceful shutdown on SIGTERM
//! and SIGINT.

use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const BIN: &str = env!("CARGO_BIN_EXE_vos");

struct HostProcess {
    child: Child,
    lines: mpsc::Receiver<String>,
}

fn host_command(data: &Path) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(data)
        .env("VOS_DATA", data)
        .env("VOS_BIND", "127.0.0.1")
        .env("VOS_PORT", "0")
        .env("RUST_LOG", "warn")
        .env_remove("VOS_CONFIG")
        .env_remove("VOS_CLIENT_DIR");
    cmd
}

fn spawn_host(data: &Path) -> HostProcess {
    let mut child = host_command(data).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    HostProcess { child, lines }
}

impl HostProcess {
    /// Read banner lines until one contains `needle`.
    fn line_with(&self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if let Ok(line) = self.lines.recv_timeout(left) {
                if line.contains(needle) {
                    return line;
                }
            }
        }
        panic!("no line containing {needle:?} within 20s");
    }

    fn wait_for_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the host did not exit after the signal");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn signal(&self, name: &str) {
        let status =
            Command::new("kill").arg(format!("-{name}")).arg(self.child.id().to_string()).status().unwrap();
        assert!(status.success());
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        let _ = self.child.kill(); // never leave a host behind, even when a test fails
        let _ = self.child.wait();
    }
}

fn code_in(line: &str) -> String {
    let after = line.split("Pairing code:").nth(1).unwrap().trim();
    after.split_whitespace().next().unwrap().to_owned()
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("agentos-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn base_url(host: &HostProcess) -> String {
    host.line_with("http://127.0.0.1:").trim().to_owned()
}

#[tokio::test]
async fn the_host_prints_a_code_writes_mcp_json_pairs_a_device_and_stops_on_sigterm() {
    let data = temp_dir("serve");
    let mut host = spawn_host(&data);
    let base = base_url(&host);
    let code = code_in(&host.line_with("Pairing code:"));

    // mcp.json: private, with the MCP url and a bearer token
    let mcp: Value = serde_json::from_slice(&std::fs::read(data.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["url"], format!("{base}/mcp"));
    assert!(
        mcp["headers"]["Authorization"].as_str().unwrap().starts_with("Bearer agentos_"),
        "the local client's token is minted like every agent token"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(std::fs::metadata(data.join("mcp.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }

    // the printed code pairs a device over HTTP; `/rpc` is refused to anyone who has not paired
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(format!("{base}/agui"))
            .json(&json!({"threadId":"t","runId":"r"}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let res = client
        .post(format!("{base}/pair"))
        .json(&json!({"code": code, "deviceName": "test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // the MCP token from mcp.json is an agent credential
    let bearer = mcp["headers"]["Authorization"].as_str().unwrap();
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}});
    let res = client
        .post(format!("{base}/mcp"))
        .header("authorization", bearer)
        .header("accept", "application/json, text/event-stream")
        .json(&init)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    host.signal("TERM");
    assert!(host.wait_for_exit().success(), "SIGTERM is a clean shutdown");
    let _ = std::fs::remove_dir_all(data);
}

#[tokio::test]
async fn sigint_stops_the_host_cleanly_too() {
    let data = temp_dir("sigint");
    let mut host = spawn_host(&data);
    host.line_with("Pairing code:");
    host.signal("INT");
    assert!(host.wait_for_exit().success());
    let _ = std::fs::remove_dir_all(data);
}

#[tokio::test]
async fn pair_prints_a_fresh_code_that_a_running_host_accepts() {
    let data = temp_dir("pair");
    let host = spawn_host(&data);
    let base = base_url(&host);
    host.line_with("Pairing code:"); // the host is up

    for flag in ["pair", "--new-pairing-code"] {
        let out = host_command(&data).arg(flag).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8(out.stdout).unwrap();
        let code = code_in(stdout.lines().find(|l| l.contains("Pairing code:")).unwrap());
        let res = reqwest::Client::new()
            .post(format!("{base}/pair"))
            .json(&json!({"code": code}))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "a code issued by `vos-host {flag}` pairs against the running host");
    }
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn an_unknown_argument_is_an_error_and_help_is_not() {
    let bad = Command::new(BIN).arg("--frobnicate").output().unwrap();
    assert!(!bad.status.success());
    let help = Command::new(BIN).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("pair"));
}
