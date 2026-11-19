//! Integration tests driving the `neoc` binary: `run` of Lua scripts, globs and
//! `--eval`, the test-runner summary, and the refusal of retired `.neoc` source.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

const BIN: &str = env!("CARGO_BIN_EXE_neoc");

fn write(dir: &Path, name: &str, src: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    path
}

#[test]
fn run_lua_script_executes() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "app.lua", "print(\"ran lua\")\n");

    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ran lua");
}

#[test]
fn run_passes_script_args_as_neoc_args() {
    let dir = tempdir().unwrap();
    let src = write(
        dir.path(),
        "args.lua",
        "print(_NEOC_ARGS[1] .. \",\" .. _NEOC_ARGS[2])\n",
    );

    let out = Command::new(BIN)
        .arg("run")
        .arg(&src)
        .arg("--")
        .arg("a")
        .arg("b")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "a,b");
}

#[test]
fn run_glob_runs_every_match() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.lua", "print(\"from a\")\n");
    write(dir.path(), "b.lua", "print(\"from b\")\n");

    let pattern = format!("{}/*.lua", dir.path().display());
    let out = Command::new(BIN).arg("run").arg(&pattern).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("from a") && stdout.contains("from b"),
        "{stdout}"
    );
}

#[test]
fn run_glob_with_no_match_is_an_error() {
    let dir = tempdir().unwrap();
    let pattern = format!("{}/none-*.lua", dir.path().display());

    let out = Command::new(BIN).arg("run").arg(&pattern).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no matches for pattern"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_neoc_path_is_rejected_with_a_plain_message() {
    let dir = tempdir().unwrap();
    let src = write(dir.path(), "app.neoc", "fn main() {}\n");

    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("runs Lua (`.lua`) scripts only"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_neoc_path_among_lua_paths_runs_nothing() {
    let dir = tempdir().unwrap();
    let ok = write(dir.path(), "a.lua", "print(\"must not run\")\n");
    let old = write(dir.path(), "old.neoc", "fn main() {}\n");

    let out = Command::new(BIN)
        .arg("run")
        .arg(&ok)
        .arg(&old)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("must not run"),
        "no script may run when a `.neoc` path is given: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("old.neoc"), "stderr: {stderr}");
}

#[test]
fn run_runtime_fault_names_the_script_and_cause() {
    let dir = tempdir().unwrap();
    write(dir.path(), "fault.lua", "local n = explode()\n");

    // Relative path: Lua truncates a long chunk name in its error text.
    let out = Command::new(BIN)
        .arg("run")
        .arg("fault.lua")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("fault.lua")
            && stderr.contains("attempt to call global 'explode' (a nil value)"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_eval_executes_inline_lua() {
    let out = Command::new(BIN)
        .arg("run")
        .arg("-e")
        .arg(r#"print("from lua eval")"#)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "from lua eval");
}

#[test]
fn run_eval_fault_reports_eval_chunk() {
    let out = Command::new(BIN)
        .arg("run")
        .arg("--eval")
        .arg("explode()")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("<eval>"), "stderr: {stderr}");
}

#[test]
fn run_eval_conflicts_with_paths() {
    // `--eval` and file paths are mutually exclusive: one source of program
    // text, not two.
    let out = Command::new(BIN)
        .arg("run")
        .arg("-e")
        .arg("print(1)")
        .arg("script.lua")
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn removed_language_flags_and_subcommands_are_refused() {
    // clap's usage-error exit code is 2.
    for args in [
        &["run", "--lua", "-e", "print(1)"][..],
        &["run", "--neoc", "-e", "print(1)"][..],
        &["build", "x.neoc"][..],
        &["check", "x.neoc"][..],
        &["fmt", "x.neoc"][..],
        &["lsp"][..],
        &["autosql"][..],
        &["emit-headers"][..],
    ] {
        let out = Command::new(BIN).args(args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "`neoc {}` must be a usage error",
            args.join(" ")
        );
    }
}

/// `lib:compile` bundles a script into a standalone executable; running it
/// passes its arguments through as `_NEOC_ARGS`, with no subcommand.
#[test]
fn compile_bundle_runs_standalone_with_arguments() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "main.lua",
        "print(\"standalone:\" .. #_NEOC_ARGS .. \":\" .. table.concat(_NEOC_ARGS, \",\"))\n",
    );
    write(
        dir.path(),
        "build.lua",
        "local ok, err = require(\"lib:compile\").bundle({ entrypoint = \"main.lua\", output = \"app\" })\nassert(ok, err)\n",
    );

    let build = Command::new(BIN)
        .arg("run")
        .arg("build.lua")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let app = dir
        .path()
        .join(if cfg!(windows) { "app.exe" } else { "app" });
    let out = Command::new(&app).arg("one").arg("two").output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "standalone:2:one,two"
    );
}

// LuaJIT bytecode is unverified — a crafted chunk escapes the sandbox — so
// `neoc run` loads `.lua` files as text only. The fixture is a stripped dump of
// `return 7`: a `\x1bLJ` header the text loader must refuse.
#[test]
fn run_lua_file_rejects_bytecode() {
    let dir = tempdir().unwrap();
    let lua = mlua::Lua::new();
    let dump: mlua::LuaString = lua
        .load("return string.dump(function() return 7 end, true)")
        .eval()
        .unwrap();
    let path = dir.path().join("payload.lua");
    fs::write(&path, dump.as_bytes()).unwrap();

    let out = Command::new(BIN).arg("run").arg(&path).output().unwrap();
    assert!(!out.status.success(), "bytecode must not run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("binary"), "got: {stderr}");
}

const FAILING_TEST: &str = "local _t = require(\"lib:test\")\n_t.test(\"fails\", function()\n  _t.expect(1).toBe(2)\nend)\n";

#[test]
fn run_test_lua_with_failing_expect_reports_and_exits_nonzero() {
    for name in ["foo.test.lua", "foo.spec.lua"] {
        let dir = tempdir().unwrap();
        let path = write(dir.path(), name, FAILING_TEST);

        let out = Command::new(BIN).arg("run").arg(&path).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{name} must exit 1");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("0 passed, 1 failed"), "{name}: {stdout}");
    }
}

#[test]
fn run_non_test_suffix_is_not_summarised() {
    let dir = tempdir().unwrap();
    let path = write(dir.path(), "foo.test.txt", FAILING_TEST);

    let out = Command::new(BIN).arg("run").arg(&path).output().unwrap();
    assert!(out.status.success(), "`.test.txt` must not be summarised");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("failed"), "got: {stdout}");
}

#[test]
fn run_kills_a_hung_test_after_the_per_test_timeout() {
    // NEOC-27: a test that never returns must fail fast and name the test,
    // not stall until the CI job timeout.
    let dir = tempdir().unwrap();
    let test = write(
        dir.path(),
        "hang.test.lua",
        "local t = require(\"lib:test\")\nt.test(\"spins forever\", function() while true do end end)\n",
    );

    let out = Command::new(BIN)
        .arg("run")
        .arg(&test)
        .env("NEOC_TEST_TIMEOUT_SECS", "1")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(124));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("spins forever"), "got: {stdout}");
    assert!(stdout.contains("timed out after 1s"), "got: {stdout}");
}
