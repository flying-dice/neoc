#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The `vos` binary itself: the banner, `mcp.json`, `pair`, and graceful shutdown on SIGTERM
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

/// A `vos` command with no `AGENTOS_*` variable inherited from the developer's shell (the host refuses to start with one).
fn vos_command() -> Command {
    let mut cmd = Command::new(BIN);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("AGENTOS_") {
            cmd.env_remove(name);
        }
    }
    cmd
}

fn host_command(data: &Path) -> Command {
    let mut cmd = vos_command();
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
    let dir = std::env::temp_dir().join(format!("vos-cli-{tag}-{}", std::process::id()));
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
        mcp["headers"]["Authorization"].as_str().unwrap().starts_with("Bearer vos_"),
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
        assert_eq!(res.status(), 200, "a code issued by `vos {flag}` pairs against the running host");
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

// -- the rename's one-time data directory move (`vos_host::migrate`) --------------------------------------------

/// The host the way a person runs it: no `VOS_DATA`, so the data directory is `$HOME/.vos`.
fn host_in_home(home: &Path) -> Command {
    let mut cmd = vos_command();
    cmd.current_dir(home)
        .env("HOME", home)
        .env_remove("USERPROFILE")
        .env_remove("VOS_DATA")
        .env_remove("VOS_CONFIG")
        .env_remove("VOS_CLIENT_DIR")
        .env_remove("VOS_BIND")
        .env_remove("VOS_PORT")
        .env("RUST_LOG", "warn");
    cmd
}

/// A pre-rename `~/.agentos`: made by running the binary against it (so it is a real system database that holds a
/// pairing code), plus the old config file name. Returns the code.
fn legacy_home(tag: &str) -> (std::path::PathBuf, String) {
    let home = temp_dir(tag);
    let old = home.join(".agentos");
    std::fs::create_dir_all(&old).unwrap();
    let out = host_command(&old).arg("pair").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let code = code_in(stdout.lines().find(|l| l.contains("Pairing code:")).unwrap());
    // the old config sets what the environment does not here, so a start that did not read it would show
    std::fs::write(old.join("agentos.toml"), "bind = \"127.0.0.1\"\nport = 0\n").unwrap();
    (home, code)
}

#[tokio::test]
async fn the_first_start_after_the_rename_moves_the_old_data_directory_and_its_pairing_survives() {
    let (home, code) = legacy_home("migrate-start");

    let mut child = host_in_home(&home).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let host = HostProcess { child, lines };
    let new = home.join(".vos");
    assert!(host.line_with("listening").contains(new.to_str().unwrap()), "it serves from ~/.vos");
    let base = base_url(&host);
    assert!(
        !base.ends_with(":7400"),
        "{base}: the port 0 of the migrated vos.toml was read (the default is 7400)"
    );
    assert!(!home.join(".agentos").exists(), "moved, not copied");
    assert!(new.join("system.db").is_file(), "the system database came along");
    assert!(
        new.join("vos.toml").is_file() && !new.join("agentos.toml").exists(),
        "the config has its new name"
    );

    // the code issued before the move is still in the moved database: it pairs
    let res = reqwest::Client::new()
        .post(format!("{base}/pair"))
        .json(&json!({"code": code, "deviceName": "migrated"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "the pairing state moved with the directory");

    host.signal("TERM");
    let mut host = host;
    assert!(host.wait_for_exit().success());
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_explicit_data_directory_leaves_the_old_default_where_it_is() {
    let (home, _) = legacy_home("migrate-explicit");
    let elsewhere = home.join("elsewhere");

    let out = host_in_home(&home).env("VOS_DATA", &elsewhere).arg("pair").output().unwrap();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(home.join(".agentos/system.db").is_file(), "not moved");
    assert!(!home.join(".vos").exists());
    assert!(elsewhere.join("system.db").is_file(), "it used the directory it was told to");
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn a_move_that_fails_is_reported_and_stops_the_start_with_the_data_where_it_was() {
    use std::os::unix::fs::PermissionsExt as _;
    let (home, _) = legacy_home("migrate-fails");
    // a home directory nobody can write to: renaming `.agentos` to `.vos` inside it is refused
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o555)).unwrap();
    let can_write_anyway = std::fs::write(home.join("probe"), "x").is_ok(); // root ignores the mode
    if can_write_anyway {
        eprintln!("skipped: this process can write to a read-only directory (running as root?)");
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(home);
        return;
    }

    let out = host_in_home(&home).arg("pair").output().unwrap();

    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!out.status.success(), "the start stops rather than beginning on an empty directory");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("moving the data directory") && stderr.contains("still in"), "{stderr}");
    assert!(home.join(".agentos/system.db").is_file(), "nothing was lost");
    assert!(home.join(".agentos/agentos.toml").is_file(), "the config name is back");
    assert!(!home.join(".vos").exists());
    let _ = std::fs::remove_dir_all(home);
}

// -- the old AGENTOS_* variables are refused, not ignored ------------------------------------------------------

#[test]
fn an_old_environment_variable_stops_serve_and_pair_naming_its_replacement() {
    let data = temp_dir("old-env");
    for args in [&["pair"][..], &[][..]] {
        let out = host_command(&data).env("AGENTOS_BIND", "127.0.0.1").args(args).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "`vos {args:?}` must not start with AGENTOS_BIND set");
        assert!(stderr.contains("AGENTOS_BIND (now VOS_BIND)"), "{stderr}");
        assert!(out.stdout.is_empty(), "no banner or pairing code was printed");
    }
    assert!(!data.join("system.db").exists(), "it stopped before touching the data directory");
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn a_clean_environment_starts_normally() {
    let data = temp_dir("clean-env");
    let out = host_command(&data).arg("pair").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Pairing code:"));
    let _ = std::fs::remove_dir_all(data);
}
