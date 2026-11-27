#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
#![allow(clippy::cast_possible_truncation)] // offsets and base64 digits: small numbers
#![cfg(unix)] // the commands typed here are a Unix shell's
//! The Terminal (`apps/terminal`, a Vite project on the vos plugin) under the real runtime: its sessions on the bus
//! (private to the page: `_open`, `_write`, ...), each a real shell on a pseudo-terminal (`vnd:portable_pty`), held to
//! the app's contract (`openrpc.json`), and its output as the notifications the shell records.
//!
//! The shell is whatever the machine gives the user who runs the tests (`$SHELL`), so the tests type only what every
//! Unix shell takes and look for marked text in the output, never for a prompt.

use crate::common;

use common::{db, id, Served};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vos_application::{
    ports::{AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

const APP: &str = "terminal";
const TIMEOUT: Duration = Duration::from_secs(20);
/// The backend's numbers (apps/terminal/server/sessions.luau).
const CHUNK: usize = 32 * 1024;
const WINDOW: usize = 256 * 1024;
const MAX_SESSIONS: usize = 8;

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

struct Rig {
    served: Served,
    shell: Arc<RecordingShell>,
}

/// The Terminal, built by its own `vite build` and served as the bus serves it (its contract holds every call).
async fn terminal() -> Rig {
    let shell = Arc::new(RecordingShell::default());
    let runtime = LuaRuntime::new(Arc::new(Quiet), shell.clone());
    Rig { served: Served::load(runtime, APP, "apps/terminal", db()).await.unwrap(), shell }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { ALPHABET[(n >> shift) as usize & 63] as char } else { '=' });
        }
    }
    out
}

fn decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut bits, mut have) = (0u32, 0);
    for byte in text.bytes().filter(|b| *b != b'=') {
        let value = ALPHABET.iter().position(|a| *a == byte).expect("base64") as u32;
        bits = bits << 6 | value;
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
        }
    }
    out
}

impl Rig {
    /// The page's call of `_<method>` (all of the app's methods are private: only the page uses them).
    async fn call(&self, method: &str, args: Value) -> PortResult<Value> {
        self.served.page(&format!("_{method}"), args).await
    }

    async fn open(&self) -> String {
        let opened = self.call("open", json!({ "rows": 24, "cols": 80 })).await.unwrap();
        opened["session"].as_str().expect("a session id").to_owned()
    }

    async fn type_line(&self, session: &str, line: &str) {
        let data = encode(format!("{line}\n").as_bytes());
        self.call("write", json!({ "session": session, "data": data })).await.unwrap();
    }

    /// The `output` notifications of `session` so far: `(offset, bytes)` in the order they were sent.
    fn output(&self, session: &str) -> Vec<(usize, Vec<u8>)> {
        self.shell
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, name, data)| name == "output" && data["session"] == session)
            .map(|(_, _, data)| {
                (data["offset"].as_u64().unwrap() as usize, decode(data["data"].as_str().unwrap()))
            })
            .collect()
    }

    fn text(&self, session: &str) -> String {
        let bytes: Vec<u8> = self.output(session).into_iter().flat_map(|(_, bytes)| bytes).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn sent(&self, session: &str) -> usize {
        self.output(session).iter().map(|(_, bytes)| bytes.len()).sum()
    }

    fn exit(&self, session: &str) -> Option<Value> {
        self.shell
            .events
            .lock()
            .unwrap()
            .iter()
            .find(|(_, name, data)| name == "exit" && data["session"] == session)
            .map(|(_, _, data)| data.clone())
    }

    /// Wait until `found` says something about the session.
    async fn until<T>(&self, what: &str, found: impl Fn(&Self) -> Option<T>) -> T {
        let started = Instant::now();
        loop {
            if let Some(value) = found(self) {
                return value;
            }
            assert!(started.elapsed() < TIMEOUT, "waited too long for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// What the shell printed between `label:` and the next `:`. The typed line has a `$` there, so only the answer
    /// matches.
    async fn printed(&self, session: &str, label: &str) -> String {
        let label = format!("{label}:");
        self.until(&label, |rig| {
            let text = rig.text(session);
            text.match_indices(&label).find_map(|(at, _)| {
                let rest = &text[at + label.len()..];
                let value = &rest[..rest.find(':')?];
                (!value.contains('$') && !value.contains('\n')).then(|| value.to_owned())
            })
        })
        .await
    }

    /// The process id of the session's shell.
    async fn pid(&self, session: &str) -> u32 {
        self.type_line(session, "echo pid:$$:").await;
        self.printed(session, "pid").await.parse().expect("a process id")
    }
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

async fn gone(pid: u32) {
    let started = Instant::now();
    while alive(pid) {
        assert!(started.elapsed() < TIMEOUT, "the shell {pid} is still running");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn refusal(result: PortResult<Value>) -> String {
    format!("{:?}", result.expect_err("the call is refused"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_method_is_the_pages_and_no_tool_is_offered_to_agents() {
    let rig = terminal().await;
    let methods = rig.served.handlers();
    assert_eq!(
        methods,
        ["_ack", "_close", "_open", "_replay", "_resize", "_write"],
        "a `_name` is the page's"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_is_a_shell_whose_output_arrives_in_order_and_can_be_asked_for_again() {
    let rig = terminal().await;
    let session = rig.open().await;
    rig.type_line(&session, "echo sum:$((6 * 7)):; echo term:$TERM:").await;
    assert_eq!(rig.printed(&session, "sum").await, "42");
    assert_eq!(rig.printed(&session, "term").await, "xterm-256color");
    rig.type_line(&session, "cd; echo home:$([ \"$PWD\" = \"$HOME\" ] && echo yes):").await;
    assert_eq!(rig.printed(&session, "home").await, "yes", "the shell starts in the home folder");

    // every notification starts where the one before ended, and none is larger than one may be
    let events = rig.output(&session);
    let mut next = 0;
    for (offset, bytes) in &events {
        assert_eq!(*offset, next);
        assert!(!bytes.is_empty() && bytes.len() <= CHUNK);
        next += bytes.len();
    }

    // the page says what it has drawn, and hears where the output ends
    let status = rig.call("ack", json!({ "session": session, "offset": next })).await.unwrap();
    assert!(status["offset"].as_u64().unwrap() as usize >= next);
    assert!(status.get("exit").is_none_or(Value::is_null));

    // what was sent can be asked for again, from any offset
    let sent: Vec<u8> = events.into_iter().flat_map(|(_, bytes)| bytes).collect();
    let replay = rig.call("replay", json!({ "session": session, "from": 5 })).await.unwrap();
    assert_eq!(replay["from"], 5);
    let again = decode(replay["data"].as_str().unwrap());
    assert!(again.starts_with(&sent[5..]) || sent[5..].starts_with(&again));

    // the terminal follows the size it is told
    rig.call("resize", json!({ "session": session, "rows": 40, "cols": 120 })).await.unwrap();
    rig.type_line(&session, "echo size:$(stty size):").await;
    assert_eq!(rig.printed(&session, "size").await, "40 120");

    rig.call("close", json!({ "session": session })).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_prints_without_end_waits_for_the_page() {
    let rig = terminal().await;
    let session = rig.open().await;
    let pid = rig.pid(&session).await;
    rig.type_line(&session, "yes 0123456789").await;

    // with nothing acknowledged the session stops reading: what it sent stays the same
    let settled = || async {
        let mut last = usize::MAX;
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let now = rig.sent(&session);
            if now == last {
                return now;
            }
            last = now;
        }
    };
    let held = tokio::time::timeout(TIMEOUT, settled()).await.expect("the flood stops");
    assert!((WINDOW..WINDOW + CHUNK).contains(&held), "{held} bytes were sent with none acknowledged");

    // other calls are answered meanwhile, and an acknowledgement lets exactly one more window through
    rig.call("ack", json!({ "session": session, "offset": held })).await.unwrap();
    let more = tokio::time::timeout(TIMEOUT, settled()).await.expect("the flood stops again");
    assert!(
        (held + WINDOW..held + WINDOW + CHUNK).contains(&more),
        "{more} bytes after {held} were acknowledged"
    );

    // closing ends the shell although its command never would
    rig.call("close", json!({ "session": session })).await.unwrap();
    gone(pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shell_that_exits_is_said_once_its_output_is_and_takes_no_more_input() {
    let rig = terminal().await;
    let session = rig.open().await;
    rig.type_line(&session, "echo last:words:; exit 3").await;
    let exit = rig.until("the exit notification", |rig| rig.exit(&session)).await;
    assert_eq!(exit["code"], 3);
    assert!(exit.get("signal").is_none_or(Value::is_null));
    // the notification says where the output ended, and everything before it was sent first
    assert_eq!(exit["offset"].as_u64().unwrap() as usize, rig.sent(&session));
    assert!(rig.text(&session).contains("last:words:"));

    let refused = refusal(rig.call("write", json!({ "session": session, "data": encode(b"ls\n") })).await);
    assert!(refused.contains("The shell has exited."), "{refused}");
    // the session is still there for a page that missed the end
    let status = rig.call("ack", json!({ "session": session, "offset": 0 })).await.unwrap();
    assert_eq!(status["exit"]["code"], 3);
    let replay = rig.call("replay", json!({ "session": session, "from": 0 })).await.unwrap();
    assert_eq!(replay["exit"]["code"], 3);

    rig.call("close", json!({ "session": session })).await.unwrap();
    let ended = refusal(rig.call("ack", json!({ "session": session, "offset": 0 })).await);
    assert!(ended.contains("This session has ended."), "{ended}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_is_not_there_is_refused_and_closing_it_is_not_an_error() {
    let rig = terminal().await;
    for (method, args) in [
        ("write", json!({ "session": "nobody", "data": "" })),
        ("resize", json!({ "session": "nobody", "rows": 24, "cols": 80 })),
        ("ack", json!({ "session": "nobody", "offset": 0 })),
        ("replay", json!({ "session": "nobody", "from": 0 })),
    ] {
        let refused = refusal(rig.call(method, args).await);
        assert!(refused.contains("This session has ended."), "{method}: {refused}");
    }
    rig.call("close", json!({ "session": "nobody" })).await.unwrap();
    // what the contract does not allow never reaches the backend
    assert!(rig.call("open", json!({ "rows": 0, "cols": 80 })).await.is_err());
    assert!(rig.call("open", json!({ "rows": 24, "cols": 80, "program": "sh" })).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_are_limited_have_ids_of_their_own_and_end_with_the_app() {
    let rig = terminal().await;
    let mut sessions = Vec::new();
    for _ in 0..MAX_SESSIONS {
        sessions.push(rig.open().await);
    }
    let mut distinct = sessions.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), MAX_SESSIONS);
    assert!(sessions.iter().all(|session| session.len() >= 22), "16 random bytes: {sessions:?}");

    // every one was heard a moment ago: none is presumed gone, so a ninth is refused
    let refused = refusal(rig.call("open", json!({ "rows": 24, "cols": 80 })).await);
    assert!(refused.contains("8 terminals are open already"), "{refused}");
    // closing one makes room
    rig.call("close", json!({ "session": sessions[0] })).await.unwrap();
    sessions[0] = rig.open().await;

    let first = rig.pid(&sessions[0]).await;
    let last = rig.pid(&sessions[MAX_SESSIONS - 1]).await;
    assert!(alive(first) && alive(last));
    // the app is unloaded (removed, or the host stops): no shell outlives it
    rig.served.rt.unload(&id(APP)).await.unwrap();
    gone(first).await;
    gone(last).await;
}
