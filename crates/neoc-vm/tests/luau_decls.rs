#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Holds the generated Luau declarations (`types/neoc.d.luau`) to these things.
//!
//! Always run (no luau-lsp needed):
//! - the committed file is what the modules record now (`committed_declarations_match_the_modules`), and the
//!   example that writes it exits 0, 1 or 2 as `--check` and a bad argument say it should;
//! - the matcher that holds the negative fixtures to their markers does its job (`matcher_*`);
//! - no file name has a colon, and the positive fixture requires every module.
//!
//! With luau-lsp (`NEOC_REQUIRE_LUAU_LSP=1`, as the pre-PR checks set it, makes a missing or other-version
//! tool a failure; without it these skip with a notice):
//! - every file of `types/fixtures/` outside `negative/` checks clean in strict mode (no type error, no lint
//!   warning), and every line of `types/fixtures/negative/**` carries exactly the findings its
//!   `-- expect-error: <text>` markers name: one finding per marker, each of whose message (up to its
//!   `caused by:` lines) contains the text, and none on an unmarked line. The negative half also fails when the
//!   declarations or the overloaded `require` do not load: `require("lib:json")` is then `any` and nothing is
//!   flagged.
//! - the editor path: the language server, started as the extension starts it (`lsp --stdio` with
//!   `--definitions:@neoc=<file>`), types `require("lib:base64")`, flags a wrong member and a wrong assignment,
//!   and shows the recorded doc on hover.
//!
//! luau-lsp is a release binary from <https://github.com/JohnnyMorganz/luau-lsp/releases> on `PATH`, pinned
//! below: its messages change between releases.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::{env, fs, thread};

use neoc_vm::lua::modules::{self, LUAU_DECLS_PATH};
use neoc_vm::lua::sandbox::make_engine;

/// The luau-lsp release the fixtures are pinned to.
const LUAU_LSP_VERSION: &str = "1.70.1";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[test]
fn committed_declarations_match_the_modules() {
    let text = modules::luau_decls(&make_engine().unwrap()).unwrap();
    let current = fs::read_to_string(root().join(LUAU_DECLS_PATH)).unwrap_or_default();
    assert!(
        current == text,
        "{LUAU_DECLS_PATH} is stale: run `cargo run -p neoc-vm --example luau_decls` and commit the result"
    );
}

/// A colon is not allowed in a Windows file name: no tracked file may have one, so the repository clones there.
#[test]
fn no_file_name_in_the_repository_has_a_colon() {
    let output = Command::new("git")
        .args(["ls-files"])
        .current_dir(root())
        .output();
    let Ok(output) = output else {
        return; // not a git checkout, nothing to check
    };
    if !output.status.success() {
        return;
    }
    let listed = String::from_utf8_lossy(&output.stdout);
    let bad: Vec<&str> = listed.lines().filter(|l| l.contains(':')).collect();
    assert!(bad.is_empty(), "file names with a colon: {bad:?}");
}

#[test]
fn every_module_is_required_by_the_positive_fixture() {
    let usage = fs::read_to_string(root().join("types/fixtures/usage.luau")).unwrap();
    // a commented-out `require` does not count
    let code: Vec<&str> = usage
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect();
    for key in modules::names() {
        assert!(
            code.iter()
                .any(|l| l.contains(&format!("= require(\"{key}\")"))),
            "types/fixtures/usage.luau never requires {key}"
        );
    }
}

// ---- the example: `--check` and the arguments ----

/// Runs `cargo run -p neoc-vm --example luau_decls -- <args>`; `(exit code, stdout, stderr)`.
fn run_example(args: &[&str]) -> (i32, String, String) {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(root())
        .args([
            "run",
            "-q",
            "-p",
            "neoc-vm",
            "--example",
            "luau_decls",
            "--",
        ])
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A folder under the system temp folder, removed when dropped, so a failing run leaves nothing behind.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("neoc-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_example_writes_and_checks() {
    let scratch = Scratch::new("decls-example");
    let file = scratch.0.join("neoc.d.luau");
    let file_arg = file.to_str().unwrap();
    let committed = fs::read_to_string(root().join(LUAU_DECLS_PATH)).unwrap();

    // a missing file is stale
    let (code, _, stderr) = run_example(&["--check", "--out", file_arg]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("--check FAILED"), "{stderr}");

    // writing it makes the committed text
    let (code, stdout, stderr) = run_example(&["--out", file_arg]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains(&format!(
            "wrote the declarations of {} modules",
            modules::names().count()
        )),
        "{stdout}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), committed);

    // and then `--check` is content
    let (code, stdout, stderr) = run_example(&["--check", "--out", file_arg]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("--check OK"), "{stdout}");

    // a stale file fails with the way to fix it, and is left as it was
    fs::write(&file, format!("{committed}-- stale\n")).unwrap();
    let (code, _, stderr) = run_example(&["--check", "--out", file_arg]);
    assert_eq!(code, 1);
    assert!(stderr.contains("--check FAILED"), "{stderr}");
    assert!(
        stderr.contains("cargo run -p neoc-vm --example luau_decls"),
        "{stderr}"
    );
    assert!(fs::read_to_string(&file).unwrap().ends_with("-- stale\n"));
}

/// A file that cannot be written (here: a folder) is exit 1 with the reason, not a panic and not exit 0.
#[test]
fn the_example_reports_a_file_it_cannot_write() {
    let scratch = Scratch::new("decls-unwritable");
    let (code, stdout, stderr) = run_example(&["--out", scratch.0.to_str().unwrap()]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.starts_with("luau_decls: "), "{stderr}");
    assert!(!stdout.contains("wrote"), "{stdout}");
}

#[test]
fn the_example_refuses_bad_arguments() {
    for args in [&["--bogus"][..], &["--out"], &["--check", "extra"]] {
        let (code, _, stderr) = run_example(args);
        assert_eq!(code, 2, "{args:?}: {stderr}");
        assert!(stderr.contains("usage: luau_decls"), "{args:?}: {stderr}");
    }
}

// ---- the matcher that holds the negative fixtures to their markers ----

/// One diagnostic of `luau-lsp analyze`: the 1-based line, the message up to its `caused by:` lines (the
/// part a marker is matched against) and the whole message.
#[derive(Debug)]
struct Finding {
    line: usize,
    primary: String,
    full: String,
}

impl Finding {
    fn new(line: usize, message: &str) -> Self {
        let primary: Vec<&str> = message
            .lines()
            .take_while(|l| !l.starts_with("caused by:"))
            .map(str::trim)
            .collect();
        Finding {
            line,
            primary: primary.join(" "),
            full: message.to_owned(),
        }
    }
}

/// The markers on each marked line of `source`: `-- expect-error: <text>`, as many as the line has findings.
fn expected_errors(name: &str, source: &str) -> Vec<(usize, Vec<String>)> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("-- expect-error"))
        .map(|(i, line)| {
            let markers = line
                .split("-- expect-error")
                .skip(1)
                .map(|marker| {
                    let text = marker.strip_prefix(':').map(str::trim);
                    text.filter(|t| !t.is_empty())
                        .unwrap_or_else(|| {
                            panic!("{name}:{}: `-- expect-error` needs a `: <text>`", i + 1)
                        })
                        .to_owned()
                })
                .collect();
            (i + 1, markers)
        })
        .collect()
}

/// What is wrong between the markers of a negative fixture and what luau-lsp found in it: a marked line must
/// carry exactly one finding per marker, each marker matching a different finding's primary message, and a
/// line without a marker must carry none.
fn check_negative(name: &str, source: &str, findings: &[Finding]) -> Vec<String> {
    let mut problems = Vec::new();
    let expected = expected_errors(name, source);
    if expected.is_empty() {
        problems.push(format!("{name} has no `-- expect-error` marker"));
    }
    for (line, markers) in &expected {
        let mut on_line: Vec<&Finding> = findings.iter().filter(|f| f.line == *line).collect();
        if on_line.len() != markers.len() {
            let got: Vec<&str> = on_line.iter().map(|f| f.primary.as_str()).collect();
            problems.push(format!(
                "{name}:{line}: {} marker(s), luau-lsp gave {} finding(s): {got:?}",
                markers.len(),
                on_line.len()
            ));
            continue;
        }
        for marker in markers {
            if let Some(at) = on_line
                .iter()
                .position(|f| f.primary.contains(marker.as_str()))
            {
                on_line.remove(at);
            } else {
                let got: Vec<&str> = on_line.iter().map(|f| f.primary.as_str()).collect();
                problems.push(format!("{name}:{line}: expected `{marker}`, got {got:?}"));
            }
        }
    }
    for f in findings
        .iter()
        .filter(|f| !expected.iter().any(|(line, _)| *line == f.line))
    {
        problems.push(format!(
            "{name}:{}: {} (a line not marked expect-error)",
            f.line, f.full
        ));
    }
    problems
}

#[test]
fn matcher_accepts_exactly_the_marked_findings() {
    let source =
        "a() -- expect-error: not found\nb()\nc() -- expect-error: one -- expect-error: two\n";
    let findings = [
        Finding::new(1, "TypeError: not found"),
        Finding::new(3, "TypeError: number one"),
        Finding::new(3, "TypeError: two"),
    ];
    assert_eq!(check_negative("f", source, &findings), Vec::<String>::new());
}

/// A required parameter that turns optional: luau-lsp then names `'string?'` first and repeats the plain
/// `'string'` under `caused by:`. The marker for the required parameter must not match that.
#[test]
fn matcher_refuses_a_marker_that_only_the_caused_by_lines_satisfy() {
    let source = "f(1) -- expect-error: Expected this to be 'string', but got 'number'\n";
    let optional = Finding::new(
        1,
        "TypeError: Expected this to be 'string?', but got 'number'\ncaused by:\n  None of the union options are compatible. For example:\nExpected this to be 'string', but got 'number'",
    );
    let required = Finding::new(
        1,
        "TypeError: Expected this to be 'string', but got 'number'",
    );
    assert!(!check_negative("f", source, &[optional]).is_empty());
    assert!(check_negative("f", source, &[required]).is_empty());
}

#[test]
fn matcher_refuses_extra_missing_and_unmarked_findings() {
    let source = "a() -- expect-error: first\nb()\nc() -- expect-error: third\n";
    // a second finding on a marked line
    let findings = [
        Finding::new(1, "first"),
        Finding::new(1, "another"),
        Finding::new(3, "third"),
    ];
    assert_eq!(check_negative("f", source, &findings).len(), 1);
    // a marked line luau-lsp accepted
    assert_eq!(
        check_negative("f", source, &[Finding::new(1, "first")]).len(),
        1
    );
    // a finding on an unmarked line
    let findings = [
        Finding::new(1, "first"),
        Finding::new(2, "surprise"),
        Finding::new(3, "third"),
    ];
    let problems = check_negative("f", source, &findings);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("not marked"), "{problems:?}");
    // two markers need two different findings
    let source = "a() -- expect-error: first -- expect-error: first\n";
    assert_eq!(
        check_negative(
            "f",
            source,
            &[Finding::new(1, "first"), Finding::new(1, "other")]
        )
        .len(),
        1
    );
    // a fixture without markers expects nothing
    assert_eq!(check_negative("f", "a()\n", &[]).len(), 1);
}

/// How many `-- expect-error` markers each negative fixture holds. Deleting a wrong line together with its
/// marker would otherwise shrink what is checked without a failure: it changes the count, which is edited
/// here on purpose, in the same change.
const NEGATIVE_MARKERS: &[(&str, usize)] = &[
    ("types/fixtures/negative/result_handling.luau", 5),
    ("types/fixtures/negative/wrong_use.luau", 56),
];

#[test]
fn negative_fixtures_keep_their_markers() {
    let mut files = Vec::new();
    fixtures_under("types/fixtures/negative", &mut files);
    let listed: Vec<&str> = NEGATIVE_MARKERS.iter().map(|(file, _)| *file).collect();
    assert_eq!(
        files.iter().map(String::as_str).collect::<Vec<_>>(),
        listed,
        "a negative fixture is not in NEGATIVE_MARKERS (or one listed is gone)"
    );
    for (file, expected) in NEGATIVE_MARKERS {
        let source = fs::read_to_string(root().join(file)).unwrap();
        let markers: usize = expected_errors(file, &source)
            .iter()
            .map(|(_, markers)| markers.len())
            .sum();
        assert_eq!(
            markers, *expected,
            "{file} holds {markers} markers, NEGATIVE_MARKERS says {expected}"
        );
    }
}

// ---- luau-lsp ----

/// `luau-lsp` (`luau-lsp.exe` on Windows) on `PATH`, or `None`.
fn find_luau_lsp() -> Option<PathBuf> {
    let name = format!("luau-lsp{}", env::consts::EXE_SUFFIX);
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
}

/// The pinned luau-lsp, or `None` with a notice when there is none and the checks are not required.
fn pinned_luau_lsp() -> Option<PathBuf> {
    let required = env::var("NEOC_REQUIRE_LUAU_LSP").is_ok_and(|v| !v.is_empty() && v != "0");
    let Some(luau_lsp) = find_luau_lsp() else {
        assert!(
            !required,
            "luau-lsp {LUAU_LSP_VERSION} not found on PATH (release binaries: https://github.com/JohnnyMorganz/luau-lsp/releases)"
        );
        eprintln!("SKIPPED: luau-lsp not found; the declarations were not checked with it");
        return None;
    };
    let version = Command::new(&luau_lsp).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if version != LUAU_LSP_VERSION {
        assert!(
            !required,
            "{} is {version:?}, the fixtures are pinned to {LUAU_LSP_VERSION}",
            luau_lsp.display()
        );
        eprintln!(
            "SKIPPED: luau-lsp is {version:?}, the fixtures are pinned to {LUAU_LSP_VERSION}"
        );
        return None;
    }
    Some(luau_lsp)
}

/// The `.luau` files under `dir` (a path under the repository root, `types/fixtures/...`), as paths relative
/// to the root, sorted: the whole tree, subfolders included.
fn fixtures_under(dir: &str, found: &mut Vec<String>) {
    for entry in fs::read_dir(root().join(dir)).unwrap().flatten() {
        let name = format!("{dir}/{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            fixtures_under(&name, found);
        } else if entry.path().extension().is_some_and(|x| x == "luau") {
            found.push(name);
        }
    }
    found.sort();
}

/// luau-lsp's findings by file, from `analyze --formatter=plain` over `files` with the declarations loaded
/// the way an editor names them (`--definitions:@neoc=<file>`).
///
/// The plain format is `path:line:col-col: (W0) Kind: message`, a message of several lines continuing on the
/// lines after it. Any other output (`[ERROR]` when the definitions file does not load, a syntax error in it)
/// fails the run: a broken declarations file must not look like a clean one.
fn analyze(luau_lsp: &Path, files: &[String]) -> Vec<(String, Vec<Finding>)> {
    let output = Command::new(luau_lsp)
        .current_dir(root())
        .args(["analyze", "--formatter=plain"])
        .arg(format!("--definitions:@neoc={LUAU_DECLS_PATH}"))
        .args(files)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut found: Vec<(String, Vec<(usize, String)>)> = Vec::new();
    for line in stderr.lines().chain(stdout.lines()) {
        if line.starts_with("[INFO]") {
            continue;
        }
        assert!(
            !line.starts_with("[ERROR]") && !line.contains("neoc.d.luau"),
            "luau-lsp could not use {LUAU_DECLS_PATH}: {line}"
        );
        let diagnostic = files.iter().find_map(|file| {
            let rest = line.strip_prefix(file.as_str())?.strip_prefix(':')?;
            let (number, rest) = rest.split_once(':')?;
            let message = rest.split_once(") ")?.1;
            Some((file, number.parse::<usize>().ok()?, message))
        });
        if let Some((file, number, message)) = diagnostic {
            if !found.iter().any(|(name, _)| name == file) {
                found.push((file.clone(), Vec::new()));
            }
            let (_, findings) = found.iter_mut().find(|(name, _)| name == file).unwrap();
            findings.push((number, message.to_owned()));
        } else if let Some((_, findings)) = found.last_mut() {
            // the rest of the message above
            let last = findings.last_mut().expect("a finding opens a file");
            last.1.push('\n');
            last.1.push_str(line);
        } else {
            panic!("unexpected luau-lsp output: {line}");
        }
    }
    found
        .into_iter()
        .map(|(file, findings)| {
            let findings = findings
                .into_iter()
                .map(|(line, message)| Finding::new(line, &message))
                .collect();
            (file, findings)
        })
        .collect()
}

#[test]
fn fixtures_check_against_the_generated_declarations() {
    let Some(luau_lsp) = pinned_luau_lsp() else {
        return;
    };
    let mut all = Vec::new();
    fixtures_under("types/fixtures", &mut all);
    let (negatives, positives): (Vec<String>, Vec<String>) = all
        .iter()
        .cloned()
        .partition(|f| f.starts_with("types/fixtures/negative/"));
    assert!(
        positives.len() >= 2 && negatives.len() >= 2,
        "fixtures missing: {positives:?} {negatives:?}"
    );
    let found = analyze(&luau_lsp, &all);
    let mut problems = Vec::new();

    // positive: every fixture outside `negative/` checks clean
    for file in &positives {
        let source = fs::read_to_string(root().join(file)).unwrap();
        assert!(
            source.starts_with("--!strict\n"),
            "{file} must opt in to strict mode"
        );
        if let Some((_, findings)) = found.iter().find(|(name, _)| name == file) {
            for f in findings {
                problems.push(format!("{file}:{}: {}", f.line, f.full));
            }
        }
    }

    // negative: exactly the marked findings
    for file in &negatives {
        let source = fs::read_to_string(root().join(file)).unwrap();
        assert!(
            source.starts_with("--!strict\n"),
            "{file} must opt in to strict mode"
        );
        let findings = found
            .iter()
            .find(|(name, _)| name == file)
            .map_or(&[][..], |(_, findings)| findings.as_slice());
        problems.extend(check_negative(file, &source, findings));
    }
    assert!(
        problems.is_empty(),
        "luau-lsp {LUAU_LSP_VERSION}:\n{}",
        problems.join("\n")
    );
}

// ---- the editor path: `luau-lsp lsp --stdio` as the extension starts it ----

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
    messages: mpsc::Receiver<serde_json::Value>,
    next_id: u64,
}

impl Client {
    fn start(luau_lsp: &Path) -> Client {
        let mut child = Command::new(luau_lsp)
            .current_dir(root())
            .args(["lsp", "--stdio"])
            .arg(format!(
                "--definitions:@neoc={}",
                root().join(LUAU_DECLS_PATH).display()
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let (tx, messages) = mpsc::channel();
        thread::spawn(move || {
            loop {
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
                    let _ = tx.send(serde_json::json!({ "transport_error": "not JSON" }));
                    return;
                };
                if tx.send(message).is_err() {
                    return;
                }
            }
        });
        Client {
            server: Server(child),
            messages,
            next_id: 0,
        }
    }

    fn send(&mut self, message: &serde_json::Value) {
        let body = message.to_string();
        let stdin = self.server.0.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: &serde_json::Value) {
        self.send(&serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Sends a request and waits for its response, answering what the server asks of a client on the way
    /// (its configuration, registrations): empty answers, as an editor with default settings gives.
    fn request(&mut self, method: &str, params: &serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(
            &serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        );
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(60))
                .unwrap_or_else(|e| panic!("no answer to {method} from luau-lsp: {e}"));
            assert!(
                message.get("transport_error").is_none(),
                "luau-lsp sent {message}"
            );
            if message.get("method").is_some() && message.get("id").is_some() {
                let result = if message["method"] == "workspace/configuration" {
                    let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                    serde_json::Value::Array(vec![serde_json::json!({}); items])
                } else {
                    serde_json::Value::Null
                };
                self.send(
                    &serde_json::json!({ "jsonrpc": "2.0", "id": message["id"], "result": result }),
                );
            } else if message.get("id") == Some(&serde_json::json!(id)) {
                assert!(message.get("error").is_none(), "{method}: {message}");
                return message["result"].clone();
            }
        }
    }
}

#[test]
fn the_language_server_types_require_as_the_editor_starts_it() {
    let Some(luau_lsp) = pinned_luau_lsp() else {
        return;
    };
    let scratch = Scratch::new("decls-lsp");
    let script = "--!strict\nlocal base64 = require(\"lib:base64\")\nlocal n: number = base64.encode(\"a\")\nbase64.nope()\nprint(base64.encode(\"x\"), n)\n";
    let file = scratch.0.join("main.luau");
    fs::write(&file, script).unwrap();
    let folder = url::Url::from_directory_path(&scratch.0)
        .unwrap()
        .to_string();
    let uri = url::Url::from_file_path(&file).unwrap().to_string();

    let mut client = Client::start(&luau_lsp);
    client.request(
        "initialize",
        &serde_json::json!({
            "processId": null,
            "rootUri": folder,
            "workspaceFolders": [{ "uri": folder, "name": "scratch" }],
            "capabilities": {
                "workspace": { "configuration": true },
                "textDocument": { "hover": { "contentFormat": ["markdown", "plaintext"] } }
            }
        }),
    );
    client.notify("initialized", &serde_json::json!({}));
    client.notify(
        "textDocument/didOpen",
        &serde_json::json!({ "textDocument": { "uri": uri, "languageId": "luau", "version": 1, "text": script } }),
    );

    // diagnostics: a type error on line 3, a missing member on line 4, and nothing else (above all no
    // `Unknown require`, which is what a server without the definitions says about `require("lib:base64")`)
    let report = client.request(
        "textDocument/diagnostic",
        &serde_json::json!({ "textDocument": { "uri": uri } }),
    );
    let findings: Vec<(u64, String)> = report["items"]
        .as_array()
        .unwrap_or_else(|| panic!("no diagnostics report: {report}"))
        .iter()
        .map(|d| {
            (
                d["range"]["start"]["line"].as_u64().unwrap() + 1,
                d["message"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(
        findings
            .iter()
            .any(|(line, m)| *line == 3
                && m.contains("Expected this to be 'number', but got 'string'")),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|(line, m)| *line == 4
            && m.contains("Key 'nope' not found in table 'lib_base64'")),
        "{findings:?}"
    );

    // hover on `encode` shows its type and the doc the module recorded
    let hover = client.request(
        "textDocument/hover",
        &serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 4, "character": 14 } }),
    );
    let text = hover["contents"]["value"].as_str().unwrap_or_default();
    assert!(
        text.contains("function lib_base64.encode(input: string): string"),
        "{hover}"
    );
    assert!(text.contains("standard alphabet"), "{hover}");
}
