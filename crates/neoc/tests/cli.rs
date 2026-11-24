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

/// The authoring language, end to end: `types/fixtures/syntax.luau` is type-checked in strict mode against the
/// generated declarations and is run here by `neoc run`, with type annotations, a
/// type alias, a generic function, `continue`, compound assignment, string interpolation and an `if` expression, and
/// typed values of the modules it requires (`std:time`, `lib:url`, `lib:semver`, `lib:json`).
#[test]
fn run_a_typed_luau_script_with_annotations_interpolation_and_continue() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../types/fixtures/syntax.luau");
    let out = Command::new(BIN).arg("run").arg(&script).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // `continue` skips "a"; `total += #word * index` is 2 * 2 + 3 * 3; the string is interpolated; the version,
    // the host and the 30 second budget come from typed module values; the buffer holds 258 little-endian (its
    // second byte is 1) and then "neoc"
    // (`print` writes the platform's line ending)
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        "total 13 is big\nexample.com\tv1.2.3\t30\n8\t1\tneoc\n"
    );
}

/// A script that is wrong the way the declarations flag it still runs: types are not checked at run time, and
/// nothing about the annotations changes what the engine does.
#[test]
fn annotations_are_not_checked_at_run_time() {
    let dir = tempdir().unwrap();
    let src = write(
        dir.path(),
        "lie.luau",
        "local n: number = \"not a number\"\nprint(`{n}`)\n",
    );
    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "not a number");
}

/// LuaJIT's loader skipped a first `#` line and a UTF-8 byte-order mark; Luau's lexer rejects both, so neoc's chunk
/// loader (`neoc_vm::load_source`) does it: an executable script and a file saved with a BOM keep running, and an
/// error on line 3 of a file with a shebang still says line 3.
#[test]
fn run_lua_script_with_a_shebang_or_a_bom_executes_and_keeps_its_line_numbers() {
    let dir = tempdir().unwrap();
    for (name, source) in [
        (
            "shebang.lua",
            "#!/usr/bin/env neoc\nprint(\"shebang ok\")\n",
        ),
        ("bom.lua", "\u{feff}print(\"bom ok\")\n"),
        (
            "both.lua",
            "\u{feff}#!/usr/bin/env neoc\nprint(\"both ok\")\n",
        ),
    ] {
        let src = write(dir.path(), name, source);
        let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(" ok"),
            "{name}"
        );
    }
    let src = write(
        dir.path(),
        "line3.lua",
        "#!/usr/bin/env neoc\nlocal x = 1\nerror(\"on line 3\")\n",
    );
    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(":3: on line 3"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `neoc run` builds its runtime with `neoc_vm::VM_THREAD_STACK` and runs the script on a worker: a script that
/// recurses to Luau's 200-level C limit through Rust host functions ends in an error it can catch, in a debug build
/// too (on tokio's 2 MiB workers, or the main thread, a `lib:cache` `on_evict` that re-enters the cache aborts the
/// process there).
#[test]
fn run_survives_recursion_through_rust_host_functions_on_its_own_threads() {
    let dir = tempdir().unwrap();
    let src = write(
        dir.path(),
        "nest.lua",
        r#"local cache
local evicted = 0
cache = require("lib:cache").Cache.new(1, nil, function() evicted = evicted + 1 cache:set("k" .. evicted, evicted) end)
cache:set("a", 1)
local ok = pcall(function() cache:set("b", 2) end)
print("on_evict", ok, evicted > 100)
"#,
    );
    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "status {:?}\nstdout: {stdout}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("on_evict\tfalse\ttrue"), "{stdout}");
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
            // Luau names no global: "attempt to call a nil value" (LuaJIT said "global 'explode'")
            && stderr.contains("attempt to call a nil value"),
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
        "#!/usr/bin/env neoc\nprint(\"standalone:\" .. #_NEOC_ARGS .. \":\" .. table.concat(_NEOC_ARGS, \",\"))\n",
    );
    // `bundle` writes exactly the path it is given, so name the output as the
    // platform expects an executable.
    let app_name = if cfg!(windows) { "app.exe" } else { "app" };
    write(
        dir.path(),
        "build.lua",
        &format!(
            "local ok, err = require(\"lib:compile\").bundle({{ entrypoint = \"main.lua\", output = \"{app_name}\" }})\nassert(ok, err)\n"
        ),
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

    let app = dir.path().join(app_name);
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

// Luau bytecode is not verified when it is loaded — a crafted chunk escapes the sandbox — so
// `neoc run` loads `.lua` files as text only. The fixture is the compiled form of
// `return 7`: a version byte the text loader must refuse.
#[test]
fn run_lua_file_rejects_bytecode() {
    let dir = tempdir().unwrap();
    let dump = mlua::chunk::Compiler::new().compile("return 7").unwrap();
    let path = dir.path().join("payload.lua");
    fs::write(&path, dump).unwrap();

    let out = Command::new(BIN).arg("run").arg(&path).output().unwrap();
    assert!(!out.status.success(), "bytecode must not run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("binary"), "got: {stderr}");
}

const FAILING_TEST: &str = "local _t = require(\"lib:test\")\n_t.test(\"fails\", function()\n  _t.expect(1):toBe(2)\nend)\n";

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

/// Starts `sh`, which starts a `sleep` of its own, prints the two process ids and holds on to the handle, so that
/// nothing but the end of `neoc` ends them. What follows it is how the script ends.
#[cfg(unix)]
const STARTS_A_PROCESS: &str = r#"local process = require("std:process")
local child = assert(process.spawn({ program = "sh", args = { "-c", "sleep 300 & echo $$ $!; wait" } }))
local ids = ""
while not string.find(ids, "\n", 1, true) do
    ids = ids .. assert(child:read())
end
print("ids " .. ids)
_G.kept = child
"#;

/// Whether the process `id` still runs. One that ended and that nobody has reaped yet (a zombie) does not.
#[cfg(unix)]
fn runs(id: &str) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", id])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&out.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

/// Run `neoc run` on a script that starts a process and then ends as `ending` says, and check that the process and
/// the one it started are gone with it.
#[cfg(unix)]
fn assert_no_process_outlives(name: &str, ending: &str, exit_code: i32) {
    let dir = tempdir().unwrap();
    let script = write(dir.path(), name, &format!("{STARTS_A_PROCESS}{ending}\n"));
    let out = Command::new(BIN)
        .arg("run")
        .arg(&script)
        .env("NEOC_TEST_TIMEOUT_SECS", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(exit_code), "{name}: {stdout}");
    let ids: Vec<&str> = stdout
        .lines()
        .find_map(|line| line.strip_prefix("ids "))
        .unwrap_or_else(|| panic!("{name}: the script did not print the ids: {stdout}"))
        .split_whitespace()
        .collect();
    assert_eq!(ids.len(), 2, "{name}: {stdout}");
    // the kill was sent before `neoc` ended; the processes are gone a moment after it
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while ids.iter().any(|id| runs(id)) {
        assert!(
            std::time::Instant::now() < deadline,
            "{name}: a process outlived the script (ids {ids:?})"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// NEOC-366: `neoc run` ends by `exit` when a script fails, which runs no destructor. A process the script started
/// must not live on, however the run ends.
#[cfg(unix)]
#[test]
fn a_process_a_script_started_does_not_outlive_the_run() {
    assert_no_process_outlives("ends.lua", "", 0);
    assert_no_process_outlives("raises.lua", "error(\"boom\")", 1);
    assert_no_process_outlives(
        "fails.test.lua",
        "local t = require(\"lib:test\")\nt.test(\"fails\", function() t.expect(1):toBe(2) end)",
        1,
    );
    assert_no_process_outlives(
        "hangs.test.lua",
        "local t = require(\"lib:test\")\nt.test(\"spins\", function() while true do end end)",
        124,
    );
}

/// Every module the runtime declares (`#[mlua_ir::module("<name>")]` in the source of `neoc-vm-std`, `neoc-vm-lib-*` and
/// `neoc-vm-vnd-*`) is in the built binary. A module registers itself when its object code is linked, so a module the
/// linker dropped would be missing from `require` with nothing else to say so. The names are read from the source,
/// not from the registry of this test binary, which the same linker built.
#[test]
fn the_binary_links_every_module_of_the_runtime() {
    fn declared(dir: &Path, names: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                declared(&path, names);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = fs::read_to_string(&path).unwrap();
                // the attribute itself, at the start of a line: a doc comment that shows one is not a module
                for line in source.lines() {
                    if let Some(rest) = line.strip_prefix("#[mlua_ir::module(\"") {
                        names.push(rest.split('"').next().unwrap().to_owned());
                    }
                }
            }
        }
    }
    let mut names = Vec::new();
    // the modules of `neoc-vm-std`, of the `neoc-vm-lib-*` crates and of the `neoc-vm-vnd-*` crates
    for dir in ["../neoc-vm-std/src", "../neoc-vm-lib", "../neoc-vm-vnd"] {
        declared(&Path::new(env!("CARGO_MANIFEST_DIR")).join(dir), &mut names);
    }
    names.sort();
    assert!(
        names.len() > 50 && names.iter().any(|n| n == "vnd:hyper"),
        "{names:?}"
    );

    let dir = tempdir().unwrap();
    let list = names
        .iter()
        .map(|n| format!("{n:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let src = write(
        dir.path(),
        "all.lua",
        &format!(
            "for _, name in {{ {list} }} do assert(type(require(name)) == \"table\", name) end\nprint(\"all\")\n"
        ),
    );
    let out = Command::new(BIN).arg("run").arg(&src).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "all");
}
