#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! The editor path of the app declarations (`types/generated/vos.d.luau`): `luau-lsp lsp --stdio`, started as the
//! editor extension starts it (with `--definitions:@vos=<file>`), types `require("vos:app")`,
//! flags a wrong member and a refused module, and shows the recorded doc on hover. (`bun run lint:luau` runs the
//! same declarations through `luau-lsp analyze` over the apps and the negative fixtures; this is the other way
//! luau-lsp is run, which the gate would otherwise never touch.)
//!
//! luau-lsp is a release binary from <https://github.com/JohnnyMorganz/luau-lsp/releases> on `PATH`, pinned below
//! (neoc's own tests pin the same one): its messages change between releases. Like `bun run lint:luau`, the test
//! skips with a notice when it is missing or another version, and FAILS in the gate (`VOS_GATE=1`, or `CI` set).

use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{json, Value};

/// The luau-lsp release the declarations are pinned to.
const LUAU_LSP_VERSION: &str = "1.70.1";

fn root() -> &'static Path {
    vos_codegen::repo_root()
}

/// Is this run the quality gate (`VOS_GATE=1`, or a `CI` that is set and not `false`/`0`)? As `scripts/gate.ts`.
fn gate_run() -> bool {
    env::var("VOS_GATE").is_ok_and(|v| v == "1")
        || env::var("CI").is_ok_and(|v| !matches!(v.as_str(), "" | "false" | "0"))
}

/// `luau-lsp` (`luau-lsp.exe` on Windows) on `PATH`, or `None`.
fn find_luau_lsp() -> Option<PathBuf> {
    let name = format!("luau-lsp{}", env::consts::EXE_SUFFIX);
    env::split_paths(&env::var_os("PATH")?).map(|dir| dir.join(&name)).find(|candidate| candidate.is_file())
}

/// The pinned luau-lsp, or `None` with a notice when there is none and the gate is not running.
fn pinned_luau_lsp() -> Option<PathBuf> {
    let Some(luau_lsp) = find_luau_lsp() else {
        assert!(
            !gate_run(),
            "luau-lsp {LUAU_LSP_VERSION} not found on PATH (release binaries: https://github.com/JohnnyMorganz/luau-lsp/releases)"
        );
        eprintln!(
            "SKIPPED: luau-lsp not found; the declarations were not checked through the language server"
        );
        return None;
    };
    let version = Command::new(&luau_lsp).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if version != LUAU_LSP_VERSION {
        assert!(
            !gate_run(),
            "{} is {version:?}, the declarations are pinned to {LUAU_LSP_VERSION}",
            luau_lsp.display()
        );
        eprintln!("SKIPPED: luau-lsp is {version:?}, the declarations are pinned to {LUAU_LSP_VERSION}");
        return None;
    }
    Some(luau_lsp)
}

/// A folder under the system temp folder, removed when dropped, so a failing run leaves nothing behind.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("vos-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `file:///...` for an absolute path (percent-encoding the few characters a temp path may hold).
fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.replace('%', "%25").replace(' ', "%20");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// Kills the server when dropped.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A language-server client of just what the test needs: framed JSON-RPC over the child's stdio.
struct Client {
    server: Server,
    messages: mpsc::Receiver<Value>,
    next_id: u64,
}

impl Client {
    fn start(luau_lsp: &Path) -> Client {
        let mut child = Command::new(luau_lsp)
            .current_dir(root())
            .args(["lsp", "--stdio"])
            .arg(format!("--definitions:@vos={}", root().join("types/generated/vos.d.luau").display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let (tx, messages) = mpsc::channel();
        thread::spawn(move || loop {
            let mut length = 0_usize;
            loop {
                let mut header = String::new();
                if out.read_line(&mut header).unwrap_or(0) == 0 {
                    return;
                }
                let header = header.trim();
                if header.is_empty() {
                    break;
                }
                if let Some(n) = header.to_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0_u8; length];
            if out.read_exact(&mut body).is_err() {
                return;
            }
            let Ok(message) = serde_json::from_slice(&body) else {
                // surfaces as a failure of the request being waited for, not as a timeout
                let _ = tx.send(json!({ "transport_error": "not JSON" }));
                return;
            };
            if tx.send(message).is_err() {
                return;
            }
        });
        Client { server: Server(child), messages, next_id: 0 }
    }

    fn send(&mut self, message: &Value) {
        let body = message.to_string();
        let stdin = self.server.0.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: &Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Sends a request and waits for its response, answering what the server asks of a client on the way (its
    /// configuration, registrations): the standard platform for the settings, empty answers for the rest, as an
    /// editor does that has `"luau-lsp.platform.type": "standard"`.
    fn request(&mut self, method: &str, params: &Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(60))
                .unwrap_or_else(|e| panic!("no answer to {method} from luau-lsp: {e}"));
            assert!(message.get("transport_error").is_none(), "luau-lsp sent {message}");
            if message.get("method").is_some() && message.get("id").is_some() {
                let result = if message["method"] == "workspace/configuration" {
                    let items = message["params"]["items"].as_array().cloned().unwrap_or_default();
                    Value::Array(
                        items
                            .iter()
                            .map(|item| match item["section"].as_str() {
                                Some("luau-lsp") => json!({ "platform": { "type": "standard" } }),
                                _ => json!({}),
                            })
                            .collect(),
                    )
                } else {
                    Value::Null
                };
                self.send(&json!({ "jsonrpc": "2.0", "id": message["id"], "result": result }));
            } else if message.get("id") == Some(&json!(id)) {
                assert!(message.get("error").is_none(), "{method}: {message}");
                return message["result"].clone();
            }
        }
    }
}

/// The script the editor has open: a few right uses and the wrong ones the language server must flag.
const SCRIPT: &str = r#"--!strict
local app = require("vos:app")
local kv = app.kv
local test = require("lib:test")
local json = require("lib:json")
local _n: number = app.id
app.nope()
test.describe("x")
local RPC = require("vos:rpc")
RPC.send_notification("changed", { count = 1 })
kv:set("count", 1)
json.object():set("k", 1)
RPC.on_request("_ping", function() return 1 end)
"#;

#[test]
fn the_language_server_types_the_vos_modules_as_the_editor_starts_it() {
    let Some(luau_lsp) = pinned_luau_lsp() else {
        return;
    };
    let scratch = Scratch::new("luau-lsp");
    let file = scratch.0.join("main.luau");
    fs::write(&file, SCRIPT).unwrap();
    let folder = file_uri(&scratch.0);
    let uri = file_uri(&file);

    let mut client = Client::start(&luau_lsp);
    client.request(
        "initialize",
        &json!({
            "processId": null,
            "rootUri": folder,
            "workspaceFolders": [{ "uri": folder, "name": "scratch" }],
            "capabilities": {
                "workspace": { "configuration": true },
                "textDocument": { "hover": { "contentFormat": ["markdown", "plaintext"] } }
            }
        }),
    );
    client.notify("initialized", &json!({}));
    client.notify(
        "textDocument/didOpen",
        &json!({ "textDocument": { "uri": uri, "languageId": "luau", "version": 1, "text": SCRIPT } }),
    );

    // diagnostics: a type error on line 6, a missing member on line 7, a refused module on line 8, and nothing else
    // (above all no `Unknown require`, which is what a server without the definitions says about `require("vos:app")`)
    let report = client.request("textDocument/diagnostic", &json!({ "textDocument": { "uri": uri } }));
    let findings: Vec<(u64, String)> = report["items"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics report: {report}"))
        .iter()
        .map(|d| {
            (d["range"]["start"]["line"].as_u64().unwrap() + 1, d["message"].as_str().unwrap().to_owned())
        })
        .collect();
    assert_eq!(findings.len(), 3, "{findings:?}");
    assert!(
        findings
            .iter()
            .any(|(line, m)| *line == 6 && m.contains("Expected this to be 'number', but got 'string'")),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|(line, m)| *line == 7 && m.contains("Key 'nope' not found in table 'vos_app'")),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|(line, m)| *line == 8
            && m.contains("Key 'describe' not found in table 'vos_NotForApps_lib_test'")),
        "{findings:?}"
    );

    // hover on `send_notification` of `RPC.send_notification` shows its type and the doc the declarations carry
    let hover = client.request(
        "textDocument/hover",
        &json!({ "textDocument": { "uri": uri }, "position": { "line": 9, "character": 6 } }),
    );
    let text = hover["contents"]["value"].as_str().unwrap_or_default();
    assert!(text.contains("name: string"), "{hover}");
    assert!(text.contains("Send one of this app's notifications on the RPC bus"), "{hover}");
}
